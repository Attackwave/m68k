//! IEEE-695 object file output ("IEEE Std 695-1990: Standard for
//! Microprocessor Systems - Binary Object Format", historically used by
//! Motorola/OASYS 68k toolchains).
//!
//! # Verification status: verified against a real reader
//!
//! The record layout below is reverse-engineered from GNU binutils'
//! `bfd/ieee.c` reader/writer (binutils 2.30, `ieee_object_p`/
//! `ieee_write_object_contents`), not from the written spec, and manually
//! confirmed with a self-built `objdump -b ieee -m m68k` / `objcopy -I ieee`
//! against output from this writer. This differs from a typical
//! best-effort reimplementation: every record type byte and field order
//! below matches what BFD's own parser expects, byte for byte.
//!
//! This writer emits, in BFD's expected order: module begin (MB), address
//! descriptor (AD), 8 fixed W-variable slots (backpatched at the end with
//! file offsets to the parts below — this is BFD's own bookkeeping
//! mechanism, not part of the logical object; a reader needs it to
//! preallocate/seek), a fixed extension record + absolute-file marker, a
//! fixed environmental record, a timestamp (ATN) record, one section
//! definition (ST) per non-empty assembler section with a base-address
//! attribute (ASW) — since this writer always emits absolute (already
//! address-resolved) output, matching BFD's `EXEC_P` code path — one
//! external-symbol record per defined symbol with an already-resolved
//! absolute value (also the `EXEC_P` path, no relocation expressions),
//! the section contents as constant-byte-load records (chunked to 127
//! bytes, matching BFD's own chunking), and a trailing module end (ME).
//!
//! Limitations (matching the ELF32 writer): the assembler flattens output
//! into one contiguous instruction list and resolves all symbol references
//! to absolute values before this writer ever sees them, so there is
//! exactly one section and no relocation records — only value assignments
//! for already-resolved symbols. This mirrors BFD's own `EXEC_P`
//! (fully-linked/absolute) output mode, which is the only mode this writer
//! implements.

use crate::assembler::{AssembledInstruction, SymbolTable};
use crate::directives::SectionManager;
use crate::output::generate_binary;

/// Number of W-variable slots BFD reserves right after the address
/// descriptor; each backpatched with a 5-byte file offset at the end.
const N_W_VARIABLES: usize = 8;

/// First public-symbol index (`IEEE_PUBLIC_BASE + 2` in BFD, base 32).
const PUBLIC_INDEX_BASE: u32 = 34;

/// First section number (`IEEE_SECTION_NUMBER_BASE` in BFD).
const SECTION_NUMBER_BASE: u32 = 1;

/// Encode an unsigned integer using IEEE-695 variable-length number
/// encoding: values 0-127 as a single literal byte, larger values as
/// 0x80|n_bytes followed by n_bytes of big-endian data (minimal length,
/// matching BFD's `ieee_write_int`).
fn encode_number(out: &mut Vec<u8>, value: u32) {
    if value <= 127 {
        out.push(value as u8);
        return;
    }
    let mut bytes = value.to_be_bytes().to_vec();
    while bytes.len() > 1 && bytes[0] == 0 {
        bytes.remove(0);
    }
    out.push(0x80 | bytes.len() as u8);
    out.extend_from_slice(&bytes);
}

/// Encode a fixed 5-byte integer (`0x84` repeat-4 marker + 4 big-endian
/// bytes), matching BFD's `ieee_write_int5` — used only for the W-variable
/// backpatch table, which BFD always reads as a fixed 5-byte field.
fn encode_number5(out: &mut Vec<u8>, value: u32) {
    out.push(0x84);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Encode a length-prefixed string (IEEE-695 "character string" / BFD's
/// `ieee_write_id`, short form only — module/section/symbol names here are
/// always well under 127 bytes).
fn encode_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(127);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
}

// Record type bytes, matching `include/ieee.h` in GNU binutils.
mod record {
    pub const MB: u8 = 0xE0; // Module Begin
    pub const ME: u8 = 0xE1; // Module End
    pub const AD: u8 = 0xEC; // Address Descriptor
    pub const ASSIGN_VALUE_TO_VARIABLE: [u8; 2] = [0xE2, 0xD7]; // W-variable backpatch
    pub const EXTERNAL_SYMBOL: u8 = 0xE8;
    pub const SET_CURRENT_SECTION: u8 = 0xE5;
    pub const SECTION_TYPE: u8 = 0xE6;
    pub const SECTION_ALIGNMENT: u8 = 0xE7;
    pub const SECTION_SIZE: [u8; 2] = [0xE2, 0xD3];
    pub const SECTION_BASE_ADDRESS: [u8; 2] = [0xE2, 0xCC];
    pub const SET_CURRENT_PC: [u8; 2] = [0xE2, 0xD0];
    pub const LOAD_CONSTANT_BYTES: u8 = 0xED;
    pub const ATTRIBUTE_RECORD: [u8; 2] = [0xF1, 0xC9];
    pub const VALUE_RECORD: [u8; 2] = [0xE2, 0xC9];
    pub const ATN_RECORD: [u8; 2] = [0xF1, 0xCE];

    // Section attribute/kind bytes.
    pub const VAR_A: u8 = 0xC1; // Absolute
    pub const VAR_S: u8 = 0xD3; // Static (paired with A for EXEC_P sections)
    pub const VAR_P: u8 = 0xD0; // Code (SEC_CODE)
    pub const VAR_D: u8 = 0xC4; // Data
}

/// One resolved output section: a kind byte (code/data), base address, and
/// raw bytes.
struct Part {
    is_code: bool,
    base_addr: u32,
    bytes: Vec<u8>,
}

/// Generate an IEEE-695 object module with a single absolute code section
/// spanning `instructions`' address range.
///
/// Returns an empty `Vec` if `instructions` is empty. Convenience wrapper
/// around [`generate_ieee695_sections`] for callers without a
/// [`SectionManager`]. See module docs for the no-relocation limitation.
pub fn generate_ieee695(instructions: &[AssembledInstruction], symbols: &SymbolTable) -> Vec<u8> {
    let Some((code_bytes, base_addr)) = generate_binary(instructions) else {
        return Vec::new();
    };
    let parts = vec![Part {
        is_code: true,
        base_addr,
        bytes: code_bytes,
    }];
    generate_ieee695_from_parts(&parts, symbols)
}

/// Generate an IEEE-695 object module with one absolute section per
/// non-empty assembler SECTION (TEXT/DATA/BSS/named), each with its own base
/// address.
///
/// See module docs for the no-relocation limitation.
pub fn generate_ieee695_sections(sections: &SectionManager, symbols: &SymbolTable) -> Vec<u8> {
    use crate::directives::SectionKind;

    let mut ordered: Vec<_> = sections
        .iter_sections()
        .filter(|(_, s)| !s.instructions.is_empty())
        .collect();
    ordered.sort_by(|a, b| {
        a.1.base_addr()
            .cmp(&b.1.base_addr())
            .then(a.0.name().cmp(b.0.name()))
    });

    let parts: Vec<Part> = ordered
        .iter()
        .map(|(kind, section)| Part {
            is_code: matches!(kind, SectionKind::Text),
            base_addr: section.base_addr(),
            bytes: section.to_bytes(),
        })
        .collect();

    generate_ieee695_from_parts(&parts, symbols)
}

fn generate_ieee695_from_parts(parts: &[Part], symbols: &SymbolTable) -> Vec<u8> {
    if parts.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();

    // Module Begin (MB): processor name, module name.
    out.push(record::MB);
    encode_string(&mut out, "68000");
    encode_string(&mut out, "m68kasm");

    // Address Descriptor (AD): 8 bits per MAU, 4 MAUs per (32-bit)
    // address. No byte-order marker — BFD's writer omits it (defaults to
    // big-endian on read).
    out.push(record::AD);
    out.push(8);
    out.push(4);

    // 8 fixed W-variable slots (8 bytes each: 2-byte opcode + 1-byte index
    // + 5-byte value), reserved here and backpatched with file offsets
    // once the corresponding parts have been written below.
    let w_table_pos = out.len();
    out.resize(w_table_pos + 8 * N_W_VARIABLES, 0);

    // Fixed extension record (BFD `exten[]`): set format version 3.3,
    // keep symbol case, mark object type relocatable-to-absolute.
    let extension_record_offset = out.len() as u32;
    out.extend_from_slice(&[
        0xf0, 0x20, 0x00, 0xf1, 0xce, 0x20, 0x00, 37, 3, 3, 0xf1, 0xce, 0x20, 0x00, 39, 2, 0xf1,
        0xce, 0x20, 0x00, 38,
    ]);
    // Absolute (EXEC_P) marker — this writer always emits fully-resolved
    // absolute output, never relocatable.
    out.push(0x01);

    // Fixed environmental record (BFD `envi[]`): exec ok, host unix.
    let environmental_record_offset = out.len() as u32;
    out.extend_from_slice(&[
        0xf0, 0x21, 0x00, 0xf1, 0xce, 0x21, 0, 52, 0x00, 0xf1, 0xce, 0x21, 0, 53, 0x03,
    ]);

    // Timestamp (ATN) record — BFD writes the real current time; a fixed
    // placeholder is fine here, readers don't validate it.
    out.extend_from_slice(&record::ATN_RECORD);
    out.push(0x21);
    out.push(0);
    out.push(50);
    for field in [1970u32, 1, 1, 0, 0, 0] {
        encode_number(&mut out, field);
    }

    // Section part: one ST (type + attributes) + ASL (alignment) + ASZ
    // (size) + ASB (base address) group per section, numbered from
    // SECTION_NUMBER_BASE upward.
    let section_part_offset = out.len() as u32;
    for (i, part) in parts.iter().enumerate() {
        let section_number = SECTION_NUMBER_BASE + i as u32;

        out.push(record::SECTION_TYPE);
        out.push(section_number as u8);
        out.push(record::VAR_A);
        out.push(record::VAR_S);
        out.push(if part.is_code {
            record::VAR_P
        } else {
            record::VAR_D
        });
        encode_string(&mut out, if part.is_code { "text" } else { "data" });
        // BFD's reader unconditionally consumes 3 more parse_int fields
        // here (parent/brother/context section indices) after the name,
        // even though its own writer never emits them for non-nested
        // sections; without these the reader misaligns every record after
        // this point. 0 = no parent/sibling/context section.
        encode_number(&mut out, 0);
        encode_number(&mut out, 0);
        encode_number(&mut out, 0);

        out.push(record::SECTION_ALIGNMENT);
        out.push(section_number as u8);
        encode_number(&mut out, 1);

        out.extend_from_slice(&record::SECTION_SIZE);
        out.push(section_number as u8);
        encode_number(&mut out, part.bytes.len() as u32);

        out.extend_from_slice(&record::SECTION_BASE_ADDRESS);
        out.push(section_number as u8);
        encode_number(&mut out, part.base_addr);
    }

    // External part: one external-symbol definition (name + attribute +
    // resolved absolute value) per defined symbol, matching BFD's
    // `EXEC_P` path (values already fully resolved, no expressions).
    let mut defined_symbols: Vec<_> = symbols.iter().filter(|(_, e)| e.defined).collect();
    defined_symbols.sort_by(|a, b| a.0.cmp(b.0));

    let external_part_offset = out.len() as u32;
    let had_symbols = !defined_symbols.is_empty();
    for (i, (name, entry)) in defined_symbols.iter().enumerate() {
        let public_index = PUBLIC_INDEX_BASE + i as u32;

        out.push(record::EXTERNAL_SYMBOL);
        encode_number(&mut out, public_index);
        encode_string(&mut out, name);

        out.extend_from_slice(&record::ATTRIBUTE_RECORD);
        encode_number(&mut out, public_index);
        out.push(15); // instruction address
        out.push(19); // static symbol
        out.push(1);

        out.extend_from_slice(&record::VALUE_RECORD);
        encode_number(&mut out, public_index);
        encode_number(&mut out, entry.value);
    }

    // Data part: section contents as constant-byte-load records, chunked
    // to 127 bytes per record (matching BFD's own `MAXRUN`).
    let data_part_offset = out.len() as u32;
    for (i, part) in parts.iter().enumerate() {
        let section_number = SECTION_NUMBER_BASE + i as u32;

        out.push(record::SET_CURRENT_SECTION);
        out.push(section_number as u8);
        out.extend_from_slice(&record::SET_CURRENT_PC);
        out.push(section_number as u8);
        encode_number(&mut out, part.base_addr);

        let mut offset = 0usize;
        while offset < part.bytes.len() {
            let run = (part.bytes.len() - offset).min(127);
            out.push(record::LOAD_CONSTANT_BYTES);
            encode_number(&mut out, run as u32);
            out.extend_from_slice(&part.bytes[offset..offset + run]);
            offset += run;
        }
    }

    // Module End (ME).
    let trailer_part_offset = out.len() as u32;
    let me_record_offset = out.len() as u32;
    out.push(record::ME);

    // Backpatch the 8 W-variable slots with file offsets to the parts
    // above, in BFD's fixed order: extension, environmental, section,
    // external, debug (unused, 0), data, trailer, me.
    let offsets = [
        extension_record_offset,
        environmental_record_offset,
        section_part_offset,
        if had_symbols { external_part_offset } else { 0 },
        0, // debug_information_part: not emitted
        data_part_offset,
        trailer_part_offset,
        me_record_offset,
    ];
    for (i, value) in offsets.iter().enumerate() {
        let slot = &mut out[w_table_pos + i * 8..w_table_pos + i * 8 + 8];
        let mut patched = Vec::with_capacity(8);
        patched.extend_from_slice(&record::ASSIGN_VALUE_TO_VARIABLE);
        patched.push(i as u8);
        encode_number5(&mut patched, *value);
        slot.copy_from_slice(&patched);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::Assembler;

    #[test]
    fn test_ieee695_starts_with_module_begin() {
        let mut asm = Assembler::new(0x1000);
        asm.assemble("    ORG $1000\nSTART:\n    MOVEQ #1,D0\n    RTS\n")
            .unwrap();

        let out = generate_ieee695(&asm.code, &asm.symbols);
        assert_eq!(out[0], record::MB);
        assert_eq!(*out.last().unwrap(), record::ME);
    }

    #[test]
    fn test_ieee695_empty_instructions() {
        let symbols = SymbolTable::new();
        assert!(generate_ieee695(&[], &symbols).is_empty());
    }

    #[test]
    fn test_ieee695_contains_symbol_name_bytes() {
        let mut asm = Assembler::new(0x1000);
        asm.assemble("    ORG $1000\nSTART:\n    MOVEQ #1,D0\n    RTS\nCOUNT EQU 5\n")
            .unwrap();

        let out = generate_ieee695(&asm.code, &asm.symbols);
        // The symbol names should appear as raw ASCII bytes preceded by
        // their length byte (encode_string format).
        let contains_name = |name: &str| {
            let needle = name.as_bytes();
            out.windows(needle.len()).any(|w| w == needle)
        };
        assert!(contains_name("START"));
        assert!(contains_name("COUNT"));
    }

    #[test]
    fn test_encode_number_small() {
        let mut out = Vec::new();
        encode_number(&mut out, 42);
        assert_eq!(out, vec![42]);
    }

    #[test]
    fn test_encode_number_large() {
        let mut out = Vec::new();
        encode_number(&mut out, 0x1000);
        assert_eq!(out, vec![0x82, 0x10, 0x00]);
    }

    #[test]
    fn test_ieee695_sections_emits_one_st_per_section() {
        let mut asm = Assembler::new(0x1000);
        asm.assemble(
            "    SECTION text\nSTART:\n    NOP\n    SECTION data\nCOUNT:\n    DC.W $1234\n",
        )
        .unwrap();

        let out = generate_ieee695_sections(&asm.sections, &asm.symbols);
        assert_eq!(out[0], record::MB);
        assert_eq!(*out.last().unwrap(), record::ME);

        let st_count = out.iter().filter(|&&b| b == record::SECTION_TYPE).count();
        assert_eq!(st_count, 2); // one per non-empty section (text, data)

        let contains_name = |name: &str| {
            let needle = name.as_bytes();
            out.windows(needle.len()).any(|w| w == needle)
        };
        assert!(contains_name("START"));
        assert!(contains_name("COUNT"));
    }

    #[test]
    fn test_ieee695_sections_empty_when_no_sections_populated() {
        let sections = SectionManager::new(0x1000);
        let symbols = SymbolTable::new();
        assert!(generate_ieee695_sections(&sections, &symbols).is_empty());
    }

    #[test]
    fn test_ieee695_address_descriptor_matches_bfd_layout() {
        // BFD's ieee_object_p expects AD, bits-per-MAU, MAUs-per-address
        // (8, 4 for 32-bit m68k) directly after the two MB strings, with
        // no byte-order marker.
        let mut asm = Assembler::new(0x1000);
        asm.assemble("    ORG $1000\n    RTS\n").unwrap();
        let out = generate_ieee695(&asm.code, &asm.symbols);

        let ad_pos = out.iter().position(|&b| b == record::AD).unwrap();
        assert_eq!(out[ad_pos + 1], 8);
        assert_eq!(out[ad_pos + 2], 4);
    }
}
