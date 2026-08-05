//! Two-pass disassembly driver: label discovery followed by formatting.

use std::collections::HashMap;

use m68k_core::addressing::InstructionStream;

use crate::decoder::{DecodeResult, decode_next};

/// One decoded line of output: an instruction or a data word, with its
/// address and raw bytes preserved for callers that want to render their
/// own listing format (e.g. with a raw-hex column).
#[derive(Debug, Clone)]
pub struct DisassembledLine {
    pub address: u32,
    pub raw_bytes: Vec<u8>,
    pub text: String,
    /// Label defined at this address, when one was discovered.
    ///
    /// Pass 1 assigns names to branch/jump targets and pass 2 substitutes
    /// them into operands, but nothing ever emitted the definitions — the
    /// output referenced 7271 labels and defined none of them, so it could
    /// not be reassembled at all. Callers print this ahead of `text`.
    pub label: Option<String>,
    /// True if decoding failed at this address; `text` then holds an
    /// error description instead of formatted instruction/data output.
    pub is_error: bool,
}

/// Orchestrates the two-pass disassembly of an m68k binary: pass 1 scans
/// the whole input to discover branch/jump targets and assign label names,
/// pass 2 decodes again and formats each instruction, substituting
/// discovered labels for addresses.
pub struct Disassembler {
    data: Vec<u8>,
    origin: u32,
    cpu: String,
    labels: HashMap<u32, String>,
    known_labels: HashMap<u32, String>,
}

impl Disassembler {
    pub fn new(data: Vec<u8>, origin: u32) -> Self {
        Self {
            data,
            origin,
            cpu: "68000".to_string(),
            labels: HashMap::new(),
            known_labels: HashMap::new(),
        }
    }

    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
    }

    /// Seed the label table with known names (e.g. symbols recovered from
    /// an executable's symbol table) before [`Disassembler::disassemble`]
    /// runs. These take priority over auto-generated `label<N>` names at
    /// the same address, and are shown even if pass 1 finds no branch/jump
    /// referencing that address.
    pub fn add_known_labels<I: IntoIterator<Item = (u32, String)>>(&mut self, labels: I) {
        self.known_labels.extend(labels);
    }

    /// Discovered labels after [`Disassembler::disassemble`] has run: maps
    /// absolute address to the generated label name (`label0`, `label1`, ...).
    pub fn labels(&self) -> &HashMap<u32, String> {
        &self.labels
    }

    /// Runs both passes and returns one [`DisassembledLine`] per decoded
    /// instruction or data word. Decode errors produce a line describing
    /// the error and resume two bytes past the failure point, mirroring
    /// the CLI's prior inline behavior.
    pub fn disassemble(&mut self) -> Vec<DisassembledLine> {
        self.pass1_discover_labels();
        self.pass2_format()
    }

    fn pass1_discover_labels(&mut self) {
        let mut targets: Vec<u32> = Vec::new();
        // Every address pass 2 will actually start decoding a line from —
        // used below to reject targets that land mid-instruction (never
        // get a line, and thus never get a label, of their own in pass 2)
        // in addition to targets outside the decoded range entirely.
        let mut line_starts: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut scan_stream = InstructionStream::new(&self.data, self.origin);
        while scan_stream.remaining() >= 2 {
            let line_pc = scan_stream.current_pc();
            match decode_next(&mut scan_stream, &self.cpu) {
                Ok((_, DecodeResult::Instruction(inst))) => {
                    line_starts.insert(line_pc);
                    if let Some(target) = inst.target_address {
                        targets.push(target);
                    }
                }
                Ok((addr, DecodeResult::DataWord(_))) => {
                    line_starts.insert(line_pc);
                    targets.push(addr);
                }
                Err(_) => break,
            }
        }

        // Branch/jump targets outside the decoded range, or that land
        // mid-instruction rather than on one of pass 2's actual line
        // starts, have no line of their own in pass 2's output — a
        // generated `labelN` name for one would be a dangling reference
        // (`bra labelN` with no `labelN:` anywhere in the listing).
        // format() falls back to rendering the raw address for anything
        // not in `self.labels`. `known_labels` (externally supplied, e.g.
        // from a symbol table) are exempt: the caller vouches for those
        // independently of what this binary slice covers.
        let mut label_idx = 0usize;
        targets.sort();
        targets.dedup();
        self.labels.clone_from(&self.known_labels);
        for addr in targets {
            if !line_starts.contains(&addr) {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(e) = self.labels.entry(addr) {
                e.insert(format!("label{}", label_idx));
                label_idx += 1;
            }
        }
    }

    fn pass2_format(&self) -> Vec<DisassembledLine> {
        let mut lines = Vec::new();
        let mut stream = InstructionStream::new(&self.data, self.origin);

        while stream.remaining() >= 2 {
            let inst_pc = stream.current_pc();

            match decode_next(&mut stream, &self.cpu) {
                Ok((_, DecodeResult::Instruction(inst))) => {
                    let text = inst.format(&self.labels);
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: inst.raw_bytes.clone(),
                        text,
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: false,
                    });
                }
                Ok((_, DecodeResult::DataWord(dw))) => {
                    let text = dw.format(&self.labels);
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: dw.raw_bytes.clone(),
                        text,
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: Vec::new(),
                        text: format!("error at {:08x}: {}", inst_pc, e),
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: true,
                    });
                    stream.seek(stream.offset + 2);
                }
            }
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disassemble_simple_instructions() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75]; // NOP; RTS
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].address, 0x1000);
        assert_eq!(lines[0].text.trim(), "nop");
        assert!(!lines[0].is_error);
        assert_eq!(lines[1].address, 0x1002);
        assert_eq!(lines[1].text.trim(), "rts");
    }

    #[test]
    fn test_disassemble_discovers_branch_label() {
        // BRA.w disp=$0002 targets pc_after_opword(0x2002)+2 = 0x2004,
        // i.e. the RTS immediately following this 4-byte BRA.w encoding.
        let bytes = vec![0x60, 0x00, 0x00, 0x02, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x2004), Some(&"label0".to_string()));
        assert!(lines[0].text.contains("label0"));
    }

    /// Regression: a branch target outside the decoded range (or landing
    /// mid-instruction rather than on an actual decoded line) must not
    /// get an auto-generated label — pass 2 never emits a line at that
    /// address, so `bra labelN` would reference a `labelN:` that appears
    /// nowhere in the listing. format() must fall back to the raw
    /// address instead.
    #[test]
    fn test_disassemble_out_of_range_target_has_no_dangling_label() {
        // BRA.w with disp pointing well past the end of this 4-byte input.
        let bytes = vec![0x60, 0x00, 0x10, 0x00];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        let lines = disasm.disassemble();

        assert!(disasm.labels().is_empty());
        assert!(lines[0].text.contains("$00003002"));
        assert!(!lines[0].text.contains("label"));
    }

    #[test]
    fn test_disassemble_raw_bytes_preserved() {
        let bytes = vec![0x4E, 0x71];
        let mut disasm = Disassembler::new(bytes.clone(), 0);
        let lines = disasm.disassemble();

        assert_eq!(lines[0].raw_bytes, bytes);
    }

    #[test]
    fn test_disassemble_cpu_gating() {
        // MULS.L Dn (68020+), should decode as data word on 68000
        let bytes = vec![0x4C, 0x00, 0x08, 0x00];
        let mut disasm = Disassembler::new(bytes, 0);
        disasm.set_cpu("68000");
        let lines = disasm.disassemble();

        assert!(lines[0].text.starts_with("dc.w"));
    }

    #[test]
    fn test_disassemble_unmatched_opcode_falls_back_to_data_word() {
        // 0xFFFF matches no opcode pattern; decode_next reports it as a data
        // word rather than an error.
        let bytes = vec![0xFF, 0xFF, 0x4E, 0x75]; // dc.w $ffff; rts
        let mut disasm = Disassembler::new(bytes, 0);
        let lines = disasm.disassemble();

        assert!(!lines[0].is_error);
        assert!(lines[0].text.starts_with("dc.w"));
        assert_eq!(lines[1].address, 2);
        assert_eq!(lines[1].text.trim(), "rts");
    }

    #[test]
    fn test_known_labels_take_priority_over_generated_names() {
        // BRA.S +2 (to the RTS below), then RTS — same target as
        // test_disassemble_discovers_branch_label, but the target address
        // is pre-seeded with a known symbol name.
        let bytes = vec![0x60, 0x02, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        disasm.add_known_labels([(0x2004, "myFunc".to_string())]);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x2004), Some(&"myFunc".to_string()));
        assert!(lines[0].text.contains("myFunc"));
    }
}
