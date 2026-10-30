//! Amiga custom chip registers in the editor: hover with decoded values,
//! access checks and completion.

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, NumberOrString, Position,
    Range,
};

use m68k_core::amiga_custom::{self, Access, ChipSet, Copper, Register};

use crate::document::{Document, ParsedSourceLine};

/// The custom register an operand refers to: `NAME(An)`, `(NAME,An)` or an
/// absolute `$DFFxxx`.
pub fn register_ref(operand: &str) -> Option<&'static Register> {
    let op = operand.trim();
    let op = op
        .strip_suffix(".l")
        .or_else(|| op.strip_suffix(".L"))
        .unwrap_or(op);
    if let Some(addr) = parse_number(op) {
        return amiga_custom::by_address(addr);
    }
    let name = match op.find('(') {
        Some(0) => op[1..].split(',').next()?,
        Some(i) => &op[..i],
        None => return None,
    };
    amiga_custom::by_name(name.trim())
}

/// Register named by a hovered word: a register name or a $DFFxxx address.
pub fn register_for_word(word: &str) -> Option<&'static Register> {
    let w = word.trim_end_matches(".l").trim_end_matches(".L");
    match parse_number(w) {
        Some(addr) => amiga_custom::by_address(addr),
        None => amiga_custom::by_name(w),
    }
}

pub fn parse_number(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('$').or_else(|| s.strip_prefix("0x")) {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = s.strip_prefix('%') {
        u32::from_str_radix(bin, 2).ok()
    } else if s.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

/// The value this line writes to `reg`, when it is a literal: an immediate
/// MOVE source, or the value half of a copper MOVE pair in `dc.w`.
fn written_value(line: &ParsedSourceLine, reg: &Register) -> Option<u16> {
    let ops = &line.operands;
    let mnemonic = line.mnemonic.to_ascii_lowercase();
    let is_reg = |op: &String| {
        register_ref(op).is_some_and(|r| r.offset == reg.offset)
            || amiga_custom::by_name(op.trim()).is_some_and(|r| r.offset == reg.offset)
            || parse_number(op).is_some_and(|n| n == reg.offset as u32 && mnemonic == "dc")
    };
    match mnemonic.as_str() {
        "move" if ops.len() == 2 && is_reg(&ops[1]) => {
            parse_number(ops[0].trim().strip_prefix('#')?).map(|v| v as u16)
        }
        "dc" if line.size.eq_ignore_ascii_case("w") => {
            let i = ops.iter().position(is_reg)?;
            (i % 2 == 0)
                .then(|| ops.get(i + 1))
                .flatten()
                .and_then(|v| parse_number(v))
                .map(|v| v as u16)
        }
        _ => None,
    }
}

pub fn hover(doc: &Document, word: &str, line_idx: usize) -> Option<String> {
    let reg = register_for_word(word)?;
    let access = match reg.access {
        Access::Read => "read-only",
        Access::Write => "write-only",
        Access::Strobe => "strobe (write triggers)",
        Access::EarlyRead => "DMA early read",
    };
    let chips: Vec<&str> = reg
        .chips
        .chars()
        .map(|c| match c {
            'A' => "Agnus",
            'D' => "Denise",
            'P' => "Paula",
            _ => "?",
        })
        .collect();
    let since = match reg.since {
        ChipSet::Ocs => "OCS",
        ChipSet::Ecs => "ECS",
        ChipSet::Aga => "AGA",
    };
    let copper = match reg.copper {
        Copper::Yes => "yes",
        Copper::Danger => "only with COPCON danger bit",
        Copper::No => "no",
    };
    let mut md = format!(
        "### {} — `${:06X}`\n\n{}\n\n* **Offset:** `${:03X}` · **Access:** {} · **Chip:** {} · **Since:** {}\n* **Copper may write:** {}\n",
        reg.name,
        reg.address(),
        reg.description,
        reg.offset,
        access,
        chips.join(", "),
        since,
        copper
    );
    if let Some(value) = doc
        .parsed_lines
        .get(line_idx)
        .and_then(|l| written_value(l, reg))
    {
        md.push_str(&format!(
            "\n**Value `${value:04X}`:** `{}`\n",
            amiga_custom::summarize(reg, value)
        ));
    }
    let fields = amiga_custom::fields(reg);
    if !fields.is_empty() {
        md.push_str("\n| Bits | Field | Meaning |\n|---|---|---|\n");
        for f in fields {
            let bits = if f.hi == f.lo {
                f.hi.to_string()
            } else {
                format!("{}-{}", f.hi, f.lo)
            };
            md.push_str(&format!("| {bits} | `{}` | {} |\n", f.name, f.description));
        }
    }
    Some(md)
}

/// How an instruction uses each operand: (reads, writes).
fn operand_use(mnemonic: &str, index: usize, count: usize) -> (bool, bool) {
    let last = index + 1 == count;
    match mnemonic {
        "move" | "movea" => (!last, last),
        "clr" | "scc" | "st" | "sf" => (false, true),
        "tst" | "cmp" | "cmpa" | "cmpi" | "cmpm" | "btst" | "chk" => (true, false),
        "or" | "ori" | "and" | "andi" | "eor" | "eori" | "add" | "addi" | "addq" | "sub"
        | "subi" | "subq" | "not" | "neg" | "negx" | "bset" | "bclr" | "bchg" | "asl" | "asr"
        | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr" | "addx" | "subx" | "nbcd" | "tas" => {
            (true, last)
        }
        _ => (false, false),
    }
}

fn read_counterpart(reg: &Register) -> Option<&'static Register> {
    let name = match reg.name {
        "VPOSW" => "VPOSR".to_string(),
        "VHPOSW" => "VHPOSR".to_string(),
        "POTGO" => "POTGOR".to_string(),
        n => format!("{n}R"),
    };
    amiga_custom::by_name(&name).filter(|r| r.access == Access::Read)
}

fn write_counterpart(reg: &Register) -> Option<&'static Register> {
    let name = match reg.name {
        "VPOSR" => "VPOSW",
        "VHPOSR" => "VHPOSW",
        "POTGOR" => "POTGO",
        n => n.strip_suffix('R')?,
    };
    amiga_custom::by_name(name).filter(|r| r.access == Access::Write)
}

/// Warnings for custom register accesses the hardware does not support.
pub fn diagnostics(doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in &doc.parsed_lines {
        let mnemonic = line.mnemonic.to_ascii_lowercase();
        for (i, op) in line.operands.iter().enumerate() {
            let Some(reg) = register_ref(op) else {
                continue;
            };
            let (reads, writes) = operand_use(&mnemonic, i, line.operands.len());
            let message = match reg.access {
                Access::Write | Access::Strobe if reads && writes => Some(format!(
                    "{} reads {} first, which is write-only: the result is undefined.{}",
                    mnemonic.to_ascii_uppercase(),
                    reg.name,
                    if reg.is_set_clear() {
                        " It is a SET/CLR register: write $8000|bits to set, the bits alone to clear."
                    } else {
                        ""
                    }
                )),
                Access::Write | Access::Strobe if reads => Some(format!(
                    "{} is write-only: reading it returns undefined data.{}",
                    reg.name,
                    read_counterpart(reg)
                        .map(|r| format!(" Read {} (${:06X}) instead.", r.name, r.address()))
                        .unwrap_or_default()
                )),
                Access::Read | Access::EarlyRead if writes => Some(format!(
                    "{} is read-only: writes have no effect.{}",
                    reg.name,
                    write_counterpart(reg)
                        .map(|r| format!(" Write {} (${:06X}) instead.", r.name, r.address()))
                        .unwrap_or_default()
                )),
                _ => None,
            };
            if let Some(message) = message {
                out.push(Diagnostic {
                    range: operand_range(line, op),
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String("CUSTOM_ACCESS".into())),
                    source: Some("m68k-lsp".into()),
                    message,
                    ..Default::default()
                });
            }
        }
    }
    out
}

/// Range of an operand within its line, falling back to the whole line.
fn operand_range(line: &ParsedSourceLine, op: &str) -> Range {
    let raw = &line.raw;
    let op = op.trim();
    let after_mnemonic = raw
        .to_ascii_lowercase()
        .find(&line.mnemonic.to_ascii_lowercase())
        .map_or(0, |i| i + line.mnemonic.len());
    let (start, len) = match raw[after_mnemonic..].find(op) {
        Some(i) => (after_mnemonic + i, op.len()),
        None => (0, raw.len()),
    };
    let col = |byte: usize| raw[..byte].encode_utf16().count() as u32;
    let row = line.line_idx as u32;
    Range::new(
        Position::new(row, col(start)),
        Position::new(row, col(start + len)),
    )
}

pub fn completion_items() -> impl Iterator<Item = CompletionItem> {
    amiga_custom::REGISTERS.iter().map(|r| CompletionItem {
        label: r.name.to_string(),
        kind: Some(CompletionItemKind::CONSTANT),
        detail: Some(format!("${:06X} custom register", r.address())),
        documentation: Some(tower_lsp::lsp_types::Documentation::String(
            r.description.to_string(),
        )),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn doc(text: &str) -> Document {
        Document::new(Url::parse("file:///t.s").unwrap(), 1, text.to_string())
    }

    #[test]
    fn recognises_register_operands() {
        assert_eq!(register_ref("DMACON(a6)").unwrap().name, "DMACON");
        assert_eq!(register_ref("(intena,a5)").unwrap().name, "INTENA");
        assert_eq!(register_ref("$dff180").unwrap().name, "COLOR00");
        assert!(register_ref("4(a6)").is_none());
        assert!(register_ref("$dff200").is_none());
    }

    #[test]
    fn hover_decodes_written_values() {
        let d = doc(" move.w #$8380,DMACON(a6)\ncop: dc.w COLOR00,$0f00,$0180,$00f0\n");
        let md = hover(&d, "DMACON", 0).unwrap();
        assert!(md.contains("`SET DMAEN|BPLEN|COPEN`"), "{md}");
        assert!(md.contains("| 9 | `DMAEN` |"), "{md}");
        assert!(hover(&d, "COLOR00", 1).unwrap().contains("`R=15 G=0 B=0`"));
        // The same register spelled as a raw offset in a copper list.
        assert!(
            hover(&d, "$0180", 1).is_none(),
            "an offset alone is not an address"
        );
    }

    #[test]
    fn warns_on_impossible_accesses() {
        let d = doc(concat!(
            " move.w DMACON(a6),d0\n",
            " or.w #$8000,INTENA(a6)\n",
            " move.w #0,DMACONR(a6)\n",
            " move.w DMACONR(a6),d0\n",
            " move.w #$7fff,$dff09a\n",
            " lea DMACON(a6),a0\n", // address only, no access
        ));
        let diags = diagnostics(&d);
        let lines: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
        assert_eq!(lines, vec![0, 1, 2]);
        assert!(
            diags[0].message.contains("Read DMACONR ($DFF002)"),
            "{}",
            diags[0].message
        );
        assert!(diags[1].message.contains("SET/CLR"), "{}", diags[1].message);
        assert!(
            diags[2].message.contains("Write DMACON ($DFF096)"),
            "{}",
            diags[2].message
        );
        assert_eq!(diags[0].range.start.character, 8);
    }
}
