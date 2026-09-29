//! Amiga Hunk executable output (`-f hunkexe`).
//!
//! Emits a `HUNK_HEADER`-based load file — the classic AmigaOS executable
//! format read by `dos.library`'s `LoadSeg()` — with one hunk per non-empty
//! assembler `SECTION` (`SectionKind::Text`/`Data`/`Named` become
//! `HUNK_CODE`/`HUNK_DATA`; `SectionKind::Bss` becomes `HUNK_BSS`), plus a
//! `HUNK_SYMBOL` block per hunk for symbols defined in that section.
//!
//! Record layout verified by round-tripping through `m68k_core::amiga_hunk`
//! (this crate's own Hunk reader, itself checked against reference
//! -Fhunkexe` output) and manually inspected with `hexdump`.
//!
//! # Relocations
//!
//! The assembler resolves every symbol to an absolute value and does not
//! track which values are addresses. [`generate_relocatable_hunk_exe`]
//! recovers the `HUNK_RELOC32` table by assembling once more per section
//! with that section moved by [`RELOC_PROBE`]: a longword that moves by
//! exactly the probe is an address in that section. Without these records
//! `LoadSeg()` leaves every absolute reference pointing at the assembly
//! origin instead of where the hunk was loaded. [`generate_hunk_exe`]
//! alone writes no relocations and suits position-independent code only.

use crate::assembler::{Assembler, SymbolTable};
use crate::directives::{Section, SectionKind, SectionManager};
use m68k_core::errors::AsmError;

const HUNK_HEADER: u32 = 0x03F3;
const HUNK_CODE: u32 = 0x03E9;
const HUNK_DATA: u32 = 0x03EA;
const HUNK_BSS: u32 = 0x03EB;
const HUNK_RELOC32: u32 = 0x03EC;
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
    write_hunk_exe(sections, symbols, &[])
}

/// How far a section is moved to find its relocations. 4 KiB-aligned so
/// `ALIGN`/`CNOP` padding stays the same, and it changes both words of a
/// longword, so a word-sized use of an address shows up as a change that
/// no longword relocation explains.
pub const RELOC_PROBE: u32 = 0x0001_1000;

/// Relocations of one hunk: `(target hunk, offsets into this hunk)`.
type HunkRelocs = Vec<(usize, Vec<u32>)>;

/// Generate a hunk executable with a `HUNK_RELOC32` table.
///
/// `base` is the already assembled program. `reassemble(origin, overrides)`
/// must assemble the same source again with the assembler origin set to
/// `origin` and each listed section placed via
/// [`SectionManager::set_origin_override`].
///
/// Fails when an address is used in a way a longword relocation cannot
/// express (e.g. `move.w #label,d0`), or when moving a section changes the
/// code size (absolute-short optimisation picking different sizes).
// ponytail: one extra assembly per section; fine for hand-written
// assembly, track relocatable values in the expression evaluator if a
// many-section build ever gets slow.
pub fn generate_relocatable_hunk_exe<F>(
    base: &Assembler,
    reassemble: F,
) -> Result<Vec<u8>, AsmError>
where
    F: Fn(u32, &[(SectionKind, u32)]) -> Result<Assembler, AsmError>,
{
    let ordered: Vec<(&SectionKind, &Section)> = base
        .sections
        .iter_sections()
        .filter(|(_, s)| !s.is_empty())
        .collect();
    let origin = base.origin();
    let mut relocs: Vec<HunkRelocs> = vec![Vec::new(); ordered.len()];

    for (target, (target_kind, _)) in ordered.iter().enumerate() {
        // The default text section follows the assembler origin, every
        // other section is pinned explicitly so only `target` moves.
        let moved_origin = |kind: &SectionKind| {
            if kind == *target_kind {
                origin.wrapping_add(RELOC_PROBE)
            } else {
                origin
            }
        };
        let overrides: Vec<(SectionKind, u32)> = ordered
            .iter()
            .filter(|(k, _)| **k != SectionKind::Text)
            .map(|(k, _)| ((*k).clone(), moved_origin(k)))
            .collect();
        // An address squeezed into a smaller field assembles at the original
        // origin but overflows once moved; say why instead of just "out of
        // range" for code that assembled fine.
        let moved =
            reassemble(moved_origin(&SectionKind::Text), &overrides).map_err(|e| AsmError {
                message: format!(
                    "{} (the value depends on the address of section {}, which a hunk \
                 executable can only hold in a full 32-bit field)",
                    e.message,
                    target_kind.name()
                ),
                ..e
            })?;

        for (hunk, (kind, section)) in ordered.iter().enumerate() {
            let old = section.to_bytes();
            let new = moved
                .sections
                .get_section(kind)
                .map(Section::to_bytes)
                .unwrap_or_default();
            if old.len() != new.len() {
                return Err(AsmError::new(format!(
                    "section {} changes size when relocated; hunk executables \
                     cannot use absolute-short optimisation for addresses",
                    kind.name()
                )));
            }
            let mut explained = vec![false; old.len()];
            let mut offsets = Vec::new();
            for k in (0..old.len().saturating_sub(3)).step_by(2) {
                let a = u32::from_be_bytes(old[k..k + 4].try_into().unwrap());
                let b = u32::from_be_bytes(new[k..k + 4].try_into().unwrap());
                if b.wrapping_sub(a) == RELOC_PROBE {
                    offsets.push(k as u32);
                    explained[k..k + 4].fill(true);
                }
            }
            if let Some(k) = (0..old.len()).find(|&k| old[k] != new[k] && !explained[k]) {
                let msg = format!(
                    "address in section {} used in a non-relocatable way \
                     (only full 32-bit addresses can be relocated)",
                    target_kind.name()
                );
                let line = section.instructions.iter().find(|i| {
                    let start = (i.pc - section.base_addr()) as usize;
                    (start..start + i.size_bytes()).contains(&k)
                });
                return Err(match line.and_then(|i| i.line_no) {
                    Some(line) => AsmError::with_line(msg, line),
                    None => AsmError::new(msg),
                });
            }
            if !offsets.is_empty() {
                relocs[hunk].push((target, offsets));
            }
        }
    }
    Ok(write_hunk_exe(&base.sections, &base.symbols, &relocs))
}

fn write_hunk_exe(
    sections: &SectionManager,
    symbols: &SymbolTable,
    relocs: &[HunkRelocs],
) -> Vec<u8> {
    // A BSS section holding nothing but `DS` reservations has no
    // instructions at all, so filtering on `instructions.is_empty()` dropped
    // it from the executable — code referencing a label in it then pointed
    // at nothing. `is_empty()` also accounts for reserved space.
    // Declaration order, straight from `iter_sections` — no re-sorting.
    // Sorting by base address and then by name looked reasonable, but every
    // section starts at 0 unless the source says otherwise, so it collapsed
    // to alphabetical: `SECTION zdata,DATA` before `SECTION acode,CODE`
    // would put a data hunk at index 0, which is where `LoadSeg()` enters.
    // The reference assembler emits declaration order (verified).
    let ordered: Vec<(&SectionKind, &Section)> = sections
        .iter_sections()
        .filter(|(_, s)| !s.is_empty())
        .collect();
    if ordered.is_empty() {
        return Vec::new();
    }

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
        // `reserved_size`, not `to_bytes().len()`: a BSS section's `DS`
        // space emits no bytes but still occupies the hunk.
        push_u32(&mut out, (section.reserved_size().div_ceil(4)) as u32);
    }

    for (hunk, (kind, section)) in ordered.iter().enumerate() {
        let mut data = section.to_bytes();
        let hunk_relocs = relocs.get(hunk).map(Vec::as_slice).unwrap_or(&[]);
        // A relocated longword holds an offset into its target hunk, which
        // `LoadSeg()` turns into an address by adding the load address.
        for (target, offsets) in hunk_relocs {
            let target_base = ordered[*target].1.base_addr();
            for &k in offsets {
                let k = k as usize;
                let v = u32::from_be_bytes(data[k..k + 4].try_into().unwrap());
                data[k..k + 4].copy_from_slice(&v.wrapping_sub(target_base).to_be_bytes());
            }
        }
        let word_count = section.reserved_size().div_ceil(4);

        // `effective_kind` honours an explicit `SECTION name,TYPE`: a named
        // section declared DATA or BSS must not be emitted as HUNK_CODE just
        // because its name is not one of the well-known ones.
        let hunk_type = match section.effective_kind() {
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

        if !hunk_relocs.is_empty() {
            push_u32(&mut out, HUNK_RELOC32);
            for (target, offsets) in hunk_relocs {
                push_u32(&mut out, offsets.len() as u32);
                push_u32(&mut out, *target as u32);
                for &k in offsets {
                    push_u32(&mut out, k);
                }
            }
            push_u32(&mut out, 0);
        }

        let base = section.base_addr();
        // Reserved size again, so a label pointing into a BSS section's `DS`
        // space still falls inside the range and keeps its HUNK_SYMBOL entry.
        let end = base + section.reserved_size() as u32;
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
                // The first filter arm above (entry.section == Some(...))
                // doesn't itself guarantee value >= base — only the
                // section-less address-range fallback arm does — so an
                // unusual ORG/SECTION combination could in principle
                // produce a symbol whose value is below its section's
                // base_addr(). `value - base` would then underflow
                // (panic in debug, wrap in release). saturating_sub keeps
                // this a plain (if wrong-looking) 0 offset instead.
                push_u32(&mut out, value.saturating_sub(base));
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
                byte_len: None,
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

    /// Assemble `src` the way the CLI does for `-f hunk-exe`.
    fn relocatable(src: &str) -> Result<Vec<u8>, AsmError> {
        let assemble = |origin: u32, overrides: &[(SectionKind, u32)]| {
            let mut asm = Assembler::new(origin);
            for (kind, at) in overrides {
                asm.sections.set_origin_override(kind.clone(), *at);
            }
            asm.assemble(src)?;
            Ok(asm)
        };
        generate_relocatable_hunk_exe(&assemble(0, &[])?, assemble)
    }

    fn long_at(data: &[u8], k: usize) -> u32 {
        u32::from_be_bytes(data[k..k + 4].try_into().unwrap())
    }

    #[test]
    fn absolute_references_are_relocated_across_sections() {
        let src = "
    SECTION code,CODE
start:
    lea     msg,a0
    move.l  #buf,d0
    lea     start(pc),a1
    move.l  #len,d1
    move.l  #msg_end-msg,d2
    rts
    SECTION data,DATA
msg:    dc.b 'hi'
msg_end:
    even
ptr:    dc.l start
    SECTION bss,BSS
buf:    ds.b 16
len     equ 16
";
        let exe = relocatable(src).unwrap();
        let loaded = read_hunk_executable(&exe, 0x2_0000).unwrap();
        let [code, data, bss] = &loaded.sections[..] else {
            panic!("expected three hunks");
        };
        // lea msg,a0 / move.l #buf,d0: addresses of the loaded hunks.
        assert_eq!(long_at(&code.data, 2), data.address);
        assert_eq!(long_at(&code.data, 8), bss.address);
        // PC-relative, EQU and label differences stay as assembled.
        assert_eq!(&code.data[12..16], &[0x43, 0xFA, 0xFF, 0xF2]);
        assert_eq!(long_at(&code.data, 18), 16);
        assert_eq!(long_at(&code.data, 24), 2);
        // A pointer in the data hunk back into code.
        assert_eq!(long_at(&data.data, 2), code.address);
        assert_eq!(code.relocs.len(), 2);
    }

    #[test]
    fn word_sized_address_is_rejected_with_its_line() {
        let err = relocatable(" move.w #msg,d0\n rts\nmsg: dc.b 0\n").unwrap_err();
        assert!(err.message.contains("32-bit field"), "{}", err.message);
        assert_eq!(err.line_no, Some(1));
    }

    #[test]
    fn scaled_address_is_rejected_with_its_line() {
        let err = relocatable(" nop\n move.l #msg*2,d0\n rts\nmsg: dc.b 0\n").unwrap_err();
        assert!(err.message.contains("non-relocatable"), "{}", err.message);
        assert_eq!(err.line_no, Some(2));
    }
}
