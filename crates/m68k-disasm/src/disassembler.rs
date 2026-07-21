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
}

impl Disassembler {
    pub fn new(data: Vec<u8>, origin: u32) -> Self {
        Self {
            data,
            origin,
            cpu: "68000".to_string(),
            labels: HashMap::new(),
        }
    }

    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
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
        let mut scan_stream = InstructionStream::new(&self.data, self.origin);
        while scan_stream.remaining() >= 2 {
            match decode_next(&mut scan_stream, &self.cpu) {
                Ok((_, DecodeResult::Instruction(inst))) => {
                    if let Some(target) = inst.target_address {
                        targets.push(target);
                    }
                }
                Ok((addr, DecodeResult::DataWord(_))) => {
                    targets.push(addr);
                }
                Err(_) => break,
            }
        }

        let mut label_idx = 0usize;
        targets.sort();
        targets.dedup();
        self.labels.clear();
        for addr in targets {
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
                        is_error: false,
                    });
                }
                Ok((_, DecodeResult::DataWord(dw))) => {
                    let text = dw.format(&self.labels);
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: dw.raw_bytes.clone(),
                        text,
                        is_error: false,
                    });
                }
                Err(e) => {
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: Vec::new(),
                        text: format!("error at {:08x}: {}", inst_pc, e),
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
        // BRA.S +2 (to the RTS below), then RTS
        let bytes = vec![0x60, 0x02, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x2004), Some(&"label0".to_string()));
        assert!(lines[0].text.contains("00002004"));
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
        // word rather than an error (mirrors the Python disassembler).
        let bytes = vec![0xFF, 0xFF, 0x4E, 0x75]; // dc.w $ffff; rts
        let mut disasm = Disassembler::new(bytes, 0);
        let lines = disasm.disassemble();

        assert!(!lines[0].is_error);
        assert!(lines[0].text.starts_with("dc.w"));
        assert_eq!(lines[1].address, 2);
        assert_eq!(lines[1].text.trim(), "rts");
    }
}
