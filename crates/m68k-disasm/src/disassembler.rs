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
    /// Half-open `[start, end)` address ranges known to hold data rather
    /// than code, from a caller that knows the container format (hunk
    /// relocations, `HUNK_DATA` sections, an explicit `--data` flag).
    data_ranges: Vec<(u32, u32)>,
    /// Addresses a caller knows execution can begin at (a hunk's entry
    /// point, a ROM's reset vector, an explicit `--entry` flag).
    entry_points: Vec<u32>,
}

impl Disassembler {
    pub fn new(data: Vec<u8>, origin: u32) -> Self {
        Self {
            data,
            origin,
            cpu: "68000".to_string(),
            labels: HashMap::new(),
            known_labels: HashMap::new(),
            data_ranges: Vec::new(),
            entry_points: Vec::new(),
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

    /// Mark half-open `[start, end)` address ranges as data rather than
    /// code.
    ///
    /// This is caller-supplied ground truth, not a guess: a `HUNK_DATA`
    /// section is data by the linker's own account, and so is every
    /// longword named by a relocation. Nothing here is inferred from the
    /// bytes themselves — a caller that only has a raw blob supplies
    /// nothing and gets the previous behavior.
    pub fn add_data_ranges<I: IntoIterator<Item = (u32, u32)>>(&mut self, ranges: I) {
        self.data_ranges
            .extend(ranges.into_iter().filter(|(s, e)| e > s));
    }

    /// Mark the four bytes at each given address as a relocated 32-bit
    /// pointer — a convenience wrapper over [`Disassembler::add_data_ranges`]
    /// for `amiga_hunk`'s reloc lists.
    pub fn add_pointer_sites<I: IntoIterator<Item = u32>>(&mut self, addresses: I) {
        let ranges: Vec<(u32, u32)> = addresses
            .into_iter()
            // A pointer within 4 bytes of the address-space end cannot be
            // represented as a half-open range; such an executable could
            // not have loaded anyway.
            .filter_map(|a| a.checked_add(4).map(|end| (a, end)))
            .collect();
        self.add_data_ranges(ranges);
    }

    /// Register addresses where execution is known to begin.
    ///
    /// Recorded now and used by the forthcoming target-following scan;
    /// the current linear pass ignores them.
    pub fn add_entry_points<I: IntoIterator<Item = u32>>(&mut self, entries: I) {
        self.entry_points.extend(entries);
    }

    /// True if `addr` falls inside any range passed to
    /// [`Disassembler::add_data_ranges`].
    fn is_data_address(&self, addr: u32) -> bool {
        self.data_ranges
            .iter()
            .any(|&(start, end)| addr >= start && addr < end)
    }

    /// How many bytes to emit as one data line starting at `addr`.
    ///
    /// A relocated pointer is exactly one longword and is emitted whole, so
    /// its four bytes read as an address rather than two unrelated words.
    /// Any other data run is emitted a word at a time, which keeps the
    /// output word-aligned and lets a range end on an odd boundary without
    /// the line straddling it. Always at least 2 and never past the end of
    /// the input.
    fn data_span_at(&self, addr: u32) -> usize {
        let remaining = (self.data.len() as u64)
            .saturating_sub((addr as u64).saturating_sub(self.origin as u64))
            as usize;
        // A 4-byte range starting exactly here is a pointer site (see
        // `add_pointer_sites`); emit it as one longword.
        let is_pointer = self
            .data_ranges
            .iter()
            .any(|&(start, end)| start == addr && end == addr.saturating_add(4));
        if is_pointer && remaining >= 4 {
            4
        } else {
            2.min(remaining)
        }
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
            // Known data must not be decoded — otherwise a pointer's bytes
            // are read as an opword, which both produces nonsense and
            // desynchronizes every following line until the stream happens
            // to realign.
            if self.is_data_address(line_pc) {
                line_starts.insert(line_pc);
                let span = self.data_span_at(line_pc);
                // A relocated pointer names an address in this image, so
                // it deserves a label at the target just as a branch does.
                if span == 4 {
                    let start = scan_stream.offset;
                    let value = u32::from_be_bytes(self.data[start..start + 4].try_into().unwrap());
                    targets.push(value);
                }
                scan_stream.seek(scan_stream.offset + span);
                continue;
            }
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

    /// Render a run of known-data bytes as a `dc.l`/`dc.w` directive.
    ///
    /// A 4-byte run is a relocated pointer, so its value is looked up in
    /// the label table — the point of tracking relocations at all is that
    /// `dc.l label3` reassembles to the same pointer, while `dc.l $21f04`
    /// hardcodes a load address.
    fn format_data(&self, raw: &[u8]) -> String {
        if raw.len() == 4 {
            let value = u32::from_be_bytes(raw.try_into().unwrap());
            return match self.labels.get(&value) {
                Some(name) => format!("dc.l     {}", name),
                None => format!("dc.l     ${:08x}", value),
            };
        }
        let value = u16::from_be_bytes([raw[0], *raw.get(1).unwrap_or(&0)]);
        format!("dc.w     ${:04x}", value)
    }

    fn pass2_format(&self) -> Vec<DisassembledLine> {
        let mut lines = Vec::new();
        let mut stream = InstructionStream::new(&self.data, self.origin);

        while stream.remaining() >= 2 {
            let inst_pc = stream.current_pc();

            // Mirrors pass 1's data check so both passes agree on where
            // lines begin; a disagreement would attach labels to addresses
            // pass 2 never emits.
            if self.is_data_address(inst_pc) {
                let span = self.data_span_at(inst_pc);
                let start = stream.offset;
                let raw = self.data[start..start + span].to_vec();
                lines.push(DisassembledLine {
                    address: inst_pc,
                    text: self.format_data(&raw),
                    raw_bytes: raw,
                    label: self.labels.get(&inst_pc).cloned(),
                    is_error: false,
                });
                stream.seek(start + span);
                continue;
            }

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

    /// A relocated pointer sitting between instructions must be emitted as
    /// a `dc.l`, not decoded. Without the data range its four bytes are
    /// read as an opword and the stream desynchronizes.
    #[test]
    fn test_pointer_site_emits_dc_l_instead_of_decoding() {
        // nop; dc.l $00002000; rts — the longword at 0x1002 is a pointer.
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x20, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text.trim(), "nop");
        assert_eq!(lines[1].address, 0x1002);
        assert_eq!(lines[1].raw_bytes, vec![0x00, 0x00, 0x20, 0x00]);
        assert!(lines[1].text.starts_with("dc.l"));
        // The instruction after the pointer must still be found — i.e. the
        // stream stayed in sync across the data.
        assert_eq!(lines[2].address, 0x1006);
        assert_eq!(lines[2].text.trim(), "rts");
    }

    /// Without the pointer range the same bytes decode as instructions —
    /// this pins down what the range actually changes.
    #[test]
    fn test_pointer_bytes_are_decoded_when_not_marked_as_data() {
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x20, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        assert!(!lines[1].text.starts_with("dc.l"));
    }

    /// A pointer into the image gets a label at its target, and the `dc.l`
    /// references it by name — so the listing reassembles to the same
    /// pointer instead of hardcoding a load address.
    #[test]
    fn test_pointer_target_gets_label_and_is_referenced_by_name() {
        // nop; dc.l $00001008; nop; rts — pointer targets the final rts.
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x10, 0x08, 0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x1008), Some(&"label0".to_string()));
        assert_eq!(lines[1].text.trim(), "dc.l     label0");
        // And the target line carries the definition.
        let target = lines.iter().find(|l| l.address == 0x1008).unwrap();
        assert_eq!(target.label, Some("label0".to_string()));
    }

    /// A pointer whose value lands outside the image must fall back to a
    /// literal, exactly like an out-of-range branch target does.
    #[test]
    fn test_pointer_outside_image_falls_back_to_literal() {
        let bytes = vec![0x4E, 0x71, 0x00, 0xFF, 0x00, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(lines[1].text.trim(), "dc.l     $00ff0000");
        assert!(disasm.labels().is_empty());
    }

    /// A multi-word data range (e.g. a whole HUNK_DATA section) is emitted
    /// word-wise, and decoding resumes at the range's end.
    #[test]
    fn test_data_range_emits_words_and_resumes_after() {
        // nop; <6 bytes of data>; rts
        let bytes = vec![0x4E, 0x71, 0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_data_ranges([(0x1002, 0x1008)]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1].text.trim(), "dc.w     $dead");
        assert_eq!(lines[2].text.trim(), "dc.w     $beef");
        assert_eq!(lines[3].text.trim(), "dc.w     $cafe");
        assert_eq!(lines[4].address, 0x1008);
        assert_eq!(lines[4].text.trim(), "rts");
    }

    /// Empty and inverted ranges are dropped rather than causing the
    /// stream to stall or skip backwards.
    #[test]
    fn test_degenerate_data_ranges_are_ignored() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_data_ranges([(0x1000, 0x1000), (0x1002, 0x1000)]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text.trim(), "nop");
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
