//! Make `m68k-disasm --source` output reassemble to the input bytes.
//!
//! The decoder accepts some encodings the assembler rebuilds differently:
//! reserved or must-be-zero bits it does not check, valid full-format
//! extension words the assembler would shorten to the brief format, and
//! forms the assembler does not accept at all. Each one silently changed
//! the program when the source was assembled again. Instead of chasing
//! every such case in the decoder, the generated source is assembled and
//! every line whose bytes do not come back exactly is replaced by `DC.B`
//! of its original bytes.

use std::collections::{HashMap, HashSet};

use m68k_asm::assembler::Assembler;
use m68k_core::errors::Severity;
use m68k_disasm::disassembler::DisassembledLine;

/// Rounds after which a source that still differs is reported. Each round
/// only replaces lines with their exact bytes, which fixes the layout from
/// that line on, so real inputs settle in one or two.
const MAX_ROUNDS: usize = 8;

/// The source text the assembler is given, and for each of its 1-based
/// lines the index of the disassembly line it renders.
fn render(lines: &[DisassembledLine], base: u32) -> (String, Vec<Option<usize>>) {
    let mut text = format!(" org ${base:x}\n");
    let mut owner = vec![None, None]; // line 0 unused, line 1 is ORG
    for (idx, line) in lines.iter().enumerate().filter(|(_, l)| !l.is_error) {
        if let Some(label) = &line.label {
            text.push_str(label);
            text.push_str(":\n");
            owner.push(Some(idx));
        }
        text.push_str("        ");
        text.push_str(&line.text);
        text.push('\n');
        owner.push(Some(idx));
    }
    (text, owner)
}

/// Whether `text` is a relative branch (Bcc, DBcc, FBcc, FDBcc), whose
/// encoding depends on the addresses of the lines around it.
fn is_branch(text: &str) -> bool {
    let mnemonic = text.split_whitespace().next().unwrap_or("");
    let stem = mnemonic.split('.').next().unwrap_or("");
    let bcc = stem.strip_prefix('b').is_some_and(|cc| {
        matches!(
            cc,
            "ra" | "sr"
                | "hi"
                | "ls"
                | "cc"
                | "cs"
                | "hs"
                | "lo"
                | "ne"
                | "eq"
                | "vc"
                | "vs"
                | "pl"
                | "mi"
                | "ge"
                | "lt"
                | "gt"
                | "le"
        )
    });
    bcc || stem.starts_with("db") || stem.starts_with("fb") || stem.starts_with("fdb")
}

/// `DC.B` holding exactly `bytes`.
fn data_line(bytes: &[u8]) -> String {
    let list: Vec<String> = bytes.iter().map(|b| format!("${b:02x}")).collect();
    format!("dc.b    {}", list.join(","))
}

/// Replace every line of `lines` that does not reassemble to its own bytes
/// with `DC.B` of those bytes. Returns the number of lines replaced, or an
/// error when the source cannot be made to assemble.
pub fn make_reassemblable(
    lines: &mut [DisassembledLine],
    base: u32,
    cpu: &str,
) -> Result<usize, String> {
    let mut replaced = HashSet::new();
    for _ in 0..MAX_ROUNDS {
        let (text, owner) = render(lines, base);
        let mut asm = Assembler::new(base);
        asm.set_cpu(cpu);
        let mut bad = HashSet::new();
        if asm.assemble(&text).is_err() {
            let mut branches = HashSet::new();
            for d in asm
                .diagnostics()
                .iter()
                .filter(|d| d.severity == Severity::Error)
            {
                match d.line_no.and_then(|n| owner.get(n).copied().flatten()) {
                    // A line that fails emits nothing, so every branch
                    // across it measures a wrong distance. Those errors
                    // are consequences; they are kept for a later round
                    // and only replaced if they persist on their own.
                    Some(idx) if is_branch(&lines[idx].text) => {
                        branches.insert(idx);
                    }
                    Some(idx) => {
                        bad.insert(idx);
                    }
                    None => return Err(format!("generated source does not assemble: {d}")),
                }
            }
            if bad.is_empty() {
                bad = branches;
            }
        } else {
            // Bytes and address each disassembly line produced.
            let mut produced: HashMap<usize, (u32, Vec<u8>)> = HashMap::new();
            for instr in &asm.code {
                let Some(idx) = instr.line_no.and_then(|n| owner.get(n).copied().flatten()) else {
                    continue;
                };
                let bytes = instr
                    .words
                    .iter()
                    .flat_map(|w| w.to_be_bytes())
                    .take(instr.size_bytes());
                produced
                    .entry(idx)
                    .or_insert_with(|| (instr.pc, Vec::new()))
                    .1
                    .extend(bytes);
            }
            for (idx, line) in lines.iter().enumerate().filter(|(_, l)| !l.is_error) {
                let ok = match produced.get(&idx) {
                    Some((pc, bytes)) => *pc == line.address && *bytes == line.raw_bytes,
                    None => line.raw_bytes.is_empty(),
                };
                if !ok {
                    bad.insert(idx);
                }
            }
        }
        if bad.is_empty() {
            return Ok(replaced.len());
        }
        let mut progress = false;
        for idx in bad {
            let line = &mut lines[idx];
            let dc = data_line(&line.raw_bytes);
            if line.raw_bytes.is_empty() || line.text == dc {
                continue;
            }
            line.text = dc;
            line.comment = None;
            replaced.insert(idx);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    Err("generated source still differs from the input after replacing lines".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use m68k_disasm::disassembler::Disassembler;

    fn source_lines(bytes: &[u8], cpu: &str) -> Vec<DisassembledLine> {
        let mut d = Disassembler::new(bytes.to_vec(), 0);
        d.set_cpu(cpu);
        d.disassemble()
    }

    #[test]
    fn lossy_decodes_fall_back_to_their_bytes() {
        // A full-format extension word the assembler would shorten
        // (`d031 0110` = `add.b (a1,d0.w),d0`), then a plain instruction.
        let bytes = [0xD0, 0x31, 0x01, 0x10, 0x4E, 0x71];
        let mut lines = source_lines(&bytes, "68020");
        let replaced = make_reassemblable(&mut lines, 0, "68020").unwrap();
        assert_eq!(replaced, 1);
        assert!(lines[0].text.starts_with("dc.b"), "{}", lines[0].text);
        assert_eq!(lines.last().unwrap().text, "nop");
    }

    #[test]
    fn exact_decodes_are_left_alone() {
        let bytes = [0x4E, 0x71, 0x60, 0xFC];
        let mut lines = source_lines(&bytes, "68000");
        assert_eq!(make_reassemblable(&mut lines, 0, "68000").unwrap(), 0);
    }
}
