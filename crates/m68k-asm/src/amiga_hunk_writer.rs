//! Amiga Hunk executable output (`-f hunkexe`).
//!
//! Emits a `HUNK_HEADER`-based load file — the classic AmigaOS executable
//! format read by `dos.library`'s `LoadSeg()` — with one hunk per non-empty
//! assembler `SECTION` (`SectionKind::Text`/`Data`/`Named` become
//! `HUNK_CODE`/`HUNK_DATA`; `SectionKind::Bss` becomes `HUNK_BSS`), plus a
//! `HUNK_SYMBOL` block per hunk for symbols defined in that section.
//!
//! Record layout verified by round-tripping through `m68k_core::amiga_hunk`
//! (this crate's own Hunk reader, itself checked against real `vasm
//! -Fhunkexe` output) and manually inspected with `hexdump`.
//!
//! # Limitations
//!
//! Like the ELF32/IEEE-695 writers, the assembler resolves all symbol
//! references to absolute values before this writer ever sees them, so
//! there are no `HUNK_RELOC32`/`HUNK_EXT` records — every hunk is emitted
//! as already-relocated, self-contained code/data. This matches how
//! `generate_binary` and friends already work; it does *not* produce a
//! relinkable object, only a directly loadable executable.

use crate::assembler::SymbolTable;
use crate::directives::{Section, SectionKind, SectionManager};

const HUNK_HEADER: u32 = 0x03F3;
const HUNK_CODE: u32 = 0x03E9;
const HUNK_DATA: u32 = 0x03EA;
const HUNK_BSS: u32 = 0x03EB;
const HUNK_SYMBOL: u32 = 0x03F0;
const HUNK_END: u32 = 0x03F2;

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Write a length-prefixed name: a longword count of following longwords,
/// then the (NUL-padded to a longword boundary) name bytes. `name` must be
/// ASCII/single-byte — non-ASCII bytes are truncated by `as u8`, matching
/// how symbol names are restricted elsewhere in the assembler.
fn push_name(buf: &mut Vec<u8>, name: &str) {
    let bytes = name.as_bytes();
    let word_count = bytes.len().div_ceil(4);
    push_u32(buf, word_count as u32);
    buf.extend_from_slice(bytes);
    buf.resize(buf.len() + (word_count * 4 - bytes.len()), 0);
}

/// Generate an Amiga Hunk executable (`HUNK_HEADER` load file) with one
/// hunk per non-empty assembler `SECTION`, each carrying its own
/// `HUNK_SYMBOL` block.
///
/// Hunks are emitted in ascending base-address order, matching the ELF32
/// writer's section ordering. Returns an empty `Vec` if there are no
/// non-empty sections.
pub fn generate_hunk_exe(sections: &SectionManager, symbols: &SymbolTable) -> Vec<u8> {
    let mut ordered: Vec<(&SectionKind, &Section)> = sections
        .iter_sections()
        .filter(|(_, s)| !s.instructions.is_empty())
        .collect();
    if ordered.is_empty() {
        return Vec::new();
    }
    ordered.sort_by(|a, b| {
        a.1.base_addr()
            .cmp(&b.1.base_addr())
            .then(a.0.name().cmp(b.0.name()))
    });

    let hunk_count = ordered.len();
    let mut out = Vec::new();

    push_u32(&mut out, HUNK_HEADER);
    push_u32(&mut out, 0); // resident library name list terminator (none)
    push_u32(&mut out, hunk_count as u32);
    push_u32(&mut out, 0); // first hunk
    push_u32(&mut out, (hunk_count - 1) as u32); // last hunk

    // Hunk size table: each hunk's byte length in longwords (top 2 bits
    // reserved for memory-type flags, left at 0 = "any/public memory").
    for (_, section) in &ordered {
        let size_bytes = section.to_bytes().len();
        push_u32(&mut out, (size_bytes.div_ceil(4)) as u32);
    }

    for (kind, section) in &ordered {
        let data = section.to_bytes();
        let word_count = data.len().div_ceil(4);

        let hunk_type = match kind {
            SectionKind::Bss => HUNK_BSS,
            SectionKind::Data => HUNK_DATA,
            SectionKind::Text | SectionKind::Named(_) => HUNK_CODE,
        };
        push_u32(&mut out, hunk_type);

        if hunk_type == HUNK_BSS {
            push_u32(&mut out, word_count as u32);
        } else {
            push_u32(&mut out, word_count as u32);
            out.extend_from_slice(&data);
            // Pad to a longword boundary.
            out.resize(out.len() + (word_count * 4 - data.len()), 0);
        }

        let base = section.base_addr();
        let end = base + section.to_bytes().len() as u32;
        let section_symbols: Vec<(&str, u32)> = symbols
            .iter()
            .filter(|(_, entry)| {
                entry.defined && entry.section.as_deref() == Some(kind.name())
                    || (entry.defined
                        && entry.section.is_none()
                        && entry.value >= base
                        && entry.value < end)
            })
            .map(|(name, entry)| (name.as_str(), entry.value))
            .collect();

        if !section_symbols.is_empty() {
            push_u32(&mut out, HUNK_SYMBOL);
            for (name, value) in section_symbols {
                push_name(&mut out, name);
                push_u32(&mut out, value - base);
            }
            push_u32(&mut out, 0); // terminator
        }

        push_u32(&mut out, HUNK_END);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use m68k_core::amiga_hunk::{SectionKind as ReadSectionKind, read_hunk_executable};

    fn section_manager_with_text(bytes: &[u16], base: u32) -> (SectionManager, SymbolTable) {
        use crate::assembler::AssembledInstruction;

        let mut sections = SectionManager::new(base);
        for (i, &word) in bytes.iter().enumerate() {
            sections.add_instruction(AssembledInstruction {
                pc: base + (i as u32) * 2,
                words: vec![word],
                line_no: None,
                source: None,
            });
        }
        let mut symbols = SymbolTable::new();
        symbols
            .define_in_section("start", base, None, Some("text"))
            .unwrap();
        (sections, symbols)
    }

    #[test]
    fn empty_sections_produce_empty_output() {
        let sections = SectionManager::new(0);
        let symbols = SymbolTable::new();
        assert!(generate_hunk_exe(&sections, &symbols).is_empty());
    }

    #[test]
    fn roundtrips_through_the_hunk_reader() {
        // NOP; RTS
        let (sections, symbols) = section_manager_with_text(&[0x4E71, 0x4E75], 0x1000);
        let exe = generate_hunk_exe(&sections, &symbols);
        assert!(!exe.is_empty());

        let loaded = read_hunk_executable(&exe, 0x1000).expect("writer output must be readable");
        assert_eq!(loaded.sections.len(), 1);
        assert_eq!(loaded.sections[0].kind, ReadSectionKind::Code);
        assert_eq!(loaded.sections[0].data, vec![0x4E, 0x71, 0x4E, 0x75]);

        let syms = loaded.all_symbols();
        assert!(syms.contains(&("start".to_string(), 0x1000)));
    }
}
