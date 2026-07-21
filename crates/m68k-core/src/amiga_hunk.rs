//! Reader for the classic AmigaOS executable "Hunk" format.
//!
//! Loads a `HUNK_HEADER`-based executable (as produced by `vasm`/`vlink`/the
//! Amiga linker), applies its 32-bit relocations, and produces a flat memory
//! image plus per-hunk section info — so the disassembler can work from a
//! real Amiga program instead of requiring a manually guessed `--org`.
//!
//! Only load-file executables (`HUNK_HEADER` ... `HUNK_END`) are supported,
//! not unlinked object files (`HUNK_UNIT`) or resident libraries (non-zero
//! first hunk). Only `HUNK_CODE`/`HUNK_DATA`/`HUNK_BSS`/`HUNK_RELOC32`/
//! `HUNK_SYMBOL`/`HUNK_DEBUG`/`HUNK_NAME`/`HUNK_END` are handled — the rarer
//! 8/16-bit and PC-relative relocation hunks (`HUNK_RELOC8`, `HUNK_DREL32`,
//! ...) are not, since they don't appear in ordinary linked executables.
//!
//! Ported from IRA's `amiga_hunks.c` (`ReadAmigaHunkExecutable`/
//! `ExamineHunks`), the 680x0 Interactive ReAssembler by Tim Ruehsen /
//! Frank Wille / Nicolas Bastien.

use std::io::{Cursor, Read};

const HUNK_CODE: u32 = 0x03E9;
const HUNK_DATA: u32 = 0x03EA;
const HUNK_BSS: u32 = 0x03EB;
const HUNK_RELOC32: u32 = 0x03EC;
const HUNK_EXT: u32 = 0x03EF;
const HUNK_SYMBOL: u32 = 0x03F0;
const HUNK_DEBUG: u32 = 0x03F1;
const HUNK_END: u32 = 0x03F2;
const HUNK_HEADER: u32 = 0x03F3;
const HUNK_NAME: u32 = 0x03E8;
/// Compact form of `HUNK_RELOC32`, with 16-bit counts/hunk references
/// instead of 32-bit ones. This is what modern linkers (vasm/vlink) emit
/// by default, so it's at least as common as the classic `HUNK_RELOC32`.
///
/// The "correct" V39+ id for this is `0x03FC`, but `dos.library`'s
/// `LoadSeg()` has accepted `0x03F7` (nominally `HUNK_DREL32`) for this
/// purpose since V37 due to a historical bug, and every linker — vasm/vlink
/// included — still emits `0x03F7` for compatibility. Accept both.
const HUNK_RELOC32SHORT: u32 = 0x03F7;
const HUNK_RELOC32SHORT_V39: u32 = 0x03FC;

/// An error reading or interpreting an Amiga Hunk executable.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HunkError(pub String);

impl HunkError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// The kind of hunk a [`Section`] was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Code,
    Data,
    Bss,
}

/// One hunk's content, already relocated to its final load address.
#[derive(Debug, Clone)]
pub struct Section {
    pub kind: SectionKind,
    pub name: Option<String>,
    /// Address this hunk was loaded at (offset from the image's load base).
    pub address: u32,
    /// Hunk content — zero-filled and empty for `Bss`.
    pub data: Vec<u8>,
    /// `(name, address)` pairs from this hunk's `HUNK_SYMBOL` block.
    pub symbols: Vec<(String, u32)>,
}

/// A parsed and relocated Amiga executable.
#[derive(Debug)]
pub struct HunkExecutable {
    pub sections: Vec<Section>,
    /// Flattened image of all sections back-to-back, starting at `load_base`.
    pub image: Vec<u8>,
    /// Address the first hunk was placed at within `image`.
    pub load_base: u32,
}

impl HunkExecutable {
    /// All symbols across all hunks, with addresses already relocated
    /// against `load_base`.
    pub fn all_symbols(&self) -> Vec<(String, u32)> {
        self.sections
            .iter()
            .flat_map(|s| s.symbols.iter().cloned())
            .collect()
    }
}

struct Reader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    fn u32(&mut self) -> Result<u32, HunkError> {
        let mut buf = [0u8; 4];
        self.cursor
            .read_exact(&mut buf)
            .map_err(|_| HunkError::new("unexpected end of file"))?;
        Ok(u32::from_be_bytes(buf))
    }

    fn u16(&mut self) -> Result<u16, HunkError> {
        let mut buf = [0u8; 2];
        self.cursor
            .read_exact(&mut buf)
            .map_err(|_| HunkError::new("unexpected end of file"))?;
        Ok(u16::from_be_bytes(buf))
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, HunkError> {
        let mut buf = vec![0u8; n];
        self.cursor
            .read_exact(&mut buf)
            .map_err(|_| HunkError::new("unexpected end of file"))?;
        Ok(buf)
    }

    fn skip(&mut self, n: u64) -> Result<(), HunkError> {
        self.cursor.set_position(
            self.cursor
                .position()
                .checked_add(n)
                .ok_or_else(|| HunkError::new("hunk length overflow while skipping data"))?,
        );
        Ok(())
    }

    /// Read a length-prefixed name/symbol string: a longword count of
    /// following longwords, then that many bytes of (NUL-padded) name data.
    /// Returns `None` at a terminating zero-length marker.
    fn name(&mut self) -> Result<Option<String>, HunkError> {
        let word_count = self.u32()?;
        if word_count == 0 {
            return Ok(None);
        }
        let raw = self.bytes(word_count as usize * 4)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(Some(String::from_utf8_lossy(&raw[..end]).into_owned()))
    }
}

/// Add each target hunk's load address into the 32-bit longword at each
/// given offset within hunk `i`'s content (both `HUNK_RELOC32` and
/// `HUNK_RELOC32SHORT` boil down to this once their counts/offsets are
/// read, just with different on-disk integer widths).
fn apply_relocations(
    contents: &mut [Vec<u8>],
    offsets: &[u32],
    i: usize,
    target_hunk: usize,
    reloc_offsets: &[u32],
) -> Result<(), HunkError> {
    if target_hunk >= offsets.len() {
        return Err(HunkError::new(format!(
            "relocation references out-of-range hunk {}",
            target_hunk
        )));
    }
    for &offset in reloc_offsets {
        let offset = offset as usize;
        if offset + 4 > contents[i].len() {
            return Err(HunkError::new("relocation offset out of hunk bounds"));
        }
        let addend = u32::from_be_bytes(contents[i][offset..offset + 4].try_into().unwrap());
        let resolved = addend.wrapping_add(offsets[target_hunk]);
        contents[i][offset..offset + 4].copy_from_slice(&resolved.to_be_bytes());
    }
    Ok(())
}

/// Parse and relocate an Amiga Hunk executable (`HUNK_HEADER` load file).
///
/// `load_base` is the address the first hunk is placed at; hunk-relative
/// addresses in relocations and symbols are resolved against it.
pub fn read_hunk_executable(data: &[u8], load_base: u32) -> Result<HunkExecutable, HunkError> {
    let mut r = Reader::new(data);

    let magic = r.u32()?;
    if magic != HUNK_HEADER {
        return Err(HunkError::new(format!(
            "not an Amiga Hunk executable (expected HUNK_HEADER magic 0x{:08x}, got 0x{:08x})",
            HUNK_HEADER, magic
        )));
    }

    // Resident library name list — empty (single terminating zero) in
    // ordinary executables.
    while r.name()?.is_some() {}

    let hunk_count = r.u32()? as usize;
    let first_hunk = r.u32()?;
    let last_hunk = r.u32()?;
    if first_hunk != 0 {
        return Err(HunkError::new(
            "resident libraries (first hunk != 0) are not supported",
        ));
    }
    if hunk_count == 0 || (last_hunk as usize) + 1 != hunk_count {
        return Err(HunkError::new("inconsistent hunk count in HUNK_HEADER"));
    }

    let mut hunk_sizes = Vec::with_capacity(hunk_count);
    for _ in 0..hunk_count {
        let raw = r.u32()?;
        // Bits 30-31 select memory type (public/chip/fast/AllocMem-flags);
        // an AllocMem-flags marker consumes one extra longword we don't need.
        if (raw >> 30) == 3 {
            r.u32()?;
        }
        hunk_sizes.push((raw & 0x3FFF_FFFF) * 4);
    }

    let mut offsets = Vec::with_capacity(hunk_count);
    let mut offs = load_base;
    for &size in &hunk_sizes {
        offsets.push(offs);
        offs = offs
            .checked_add(size)
            .ok_or_else(|| HunkError::new("hunk layout overflows 32-bit address space"))?;
    }

    let mut contents: Vec<Vec<u8>> = hunk_sizes.iter().map(|&s| vec![0u8; s as usize]).collect();
    let mut kinds: Vec<Option<SectionKind>> = vec![None; hunk_count];
    let mut names: Vec<Option<String>> = vec![None; hunk_count];
    let mut symbols: Vec<Vec<(String, u32)>> = vec![Vec::new(); hunk_count];

    let mut i = 0usize;
    let mut pending_name: Option<String> = None;
    while i < hunk_count {
        let raw = r.u32()?;
        let hunk = raw & 0x0000_FFFF;

        match hunk {
            HUNK_CODE | HUNK_DATA | HUNK_BSS => {
                if (raw & 0xC000_0000) != 0 && (raw >> 30) == 3 {
                    r.u32()?; // AllocMem flags, unused
                }
                kinds[i] = Some(match hunk {
                    HUNK_CODE => SectionKind::Code,
                    HUNK_DATA => SectionKind::Data,
                    _ => SectionKind::Bss,
                });
                names[i] = pending_name.take();

                let word_count = r.u32()?;
                if hunk != HUNK_BSS {
                    let body = r.bytes(word_count as usize * 4)?;
                    contents[i][..body.len()].copy_from_slice(&body);
                }
            }
            HUNK_RELOC32 => loop {
                let count = r.u32()?;
                if count == 0 {
                    break;
                }
                let target_hunk = r.u32()? as usize;
                let offsets_list = (0..count).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
                apply_relocations(&mut contents, &offsets, i, target_hunk, &offsets_list)?;
            },
            HUNK_RELOC32SHORT | HUNK_RELOC32SHORT_V39 => {
                let mut total = 0usize;
                loop {
                    let count = r.u16()?;
                    if count == 0 {
                        // Word-aligned: the terminator word itself counts
                        // towards the running total, so a *even* total of
                        // relocations-plus-terminator (i.e. `total` itself
                        // even) means one more padding word follows.
                        if total.is_multiple_of(2) {
                            r.u16()?;
                        }
                        break;
                    }
                    total += count as usize;
                    let target_hunk = r.u16()? as usize;
                    let offsets_list = (0..count)
                        .map(|_| r.u16().map(|v| v as u32))
                        .collect::<Result<Vec<_>, _>>()?;
                    apply_relocations(&mut contents, &offsets, i, target_hunk, &offsets_list)?;
                }
            }
            HUNK_SYMBOL => {
                while let Some(sym_name) = r.name()? {
                    let value = r.u32()?;
                    symbols[i].push((sym_name, offsets[i] + value));
                }
            }
            HUNK_DEBUG => {
                let word_count = r.u32()?;
                r.skip(word_count as u64 * 4)?;
            }
            HUNK_NAME => {
                // name for the *next* code/data/bss hunk.
                pending_name = r.name()?;
            }
            HUNK_EXT => {
                return Err(HunkError::new(
                    "HUNK_EXT (external symbol references) is not supported — \
                     the executable is not fully linked",
                ));
            }
            HUNK_END => {
                i += 1;
                pending_name = None;
            }
            other => {
                return Err(HunkError::new(format!(
                    "unsupported or unknown hunk type 0x{:04x}",
                    other
                )));
            }
        }
    }

    let sections = (0..hunk_count)
        .map(|idx| Section {
            kind: kinds[idx].unwrap_or(SectionKind::Code),
            name: names[idx].take(),
            address: offsets[idx],
            data: std::mem::take(&mut contents[idx]),
            symbols: std::mem::take(&mut symbols[idx]),
        })
        .collect::<Vec<_>>();

    let total_size: u32 = hunk_sizes.iter().sum();
    let mut image = vec![0u8; total_size as usize];
    for section in &sections {
        let start = (section.address - load_base) as usize;
        image[start..start + section.data.len()].copy_from_slice(&section.data);
    }

    Ok(HunkExecutable {
        sections,
        image,
        load_base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // vasm -Fhunkexe output for a single CODE hunk with no relocations:
    //   moveq #1,d0 ; move.l #$12345678,d1 ; lea msg(pc),a0 ; jsr func ; rts
    //   func: movem.l d0-d7/a0-a6,-(sp) ; movem.l (sp)+,d0-d7/a0-a6 ; rts
    //   msg: dc.b "Hello",0
    const SINGLE_HUNK_EXE: &[u8] = &[
        0x00, 0x00, 0x03, 0xf3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x03, 0xe9, 0x00, 0x00,
        0x00, 0x08, 0x70, 0x01, 0x22, 0x3c, 0x12, 0x34, 0x56, 0x78, 0x41, 0xfa, 0x00, 0x10, 0x61,
        0x02, 0x4e, 0x75, 0x48, 0xe7, 0xff, 0xfe, 0x4c, 0xdf, 0x7f, 0xff, 0x4e, 0x75, 0x48, 0x65,
        0x6c, 0x6c, 0x6f, 0x00, 0x00, 0x00, 0x03, 0xf0, 0x00, 0x00, 0x00, 0x02, 0x73, 0x74, 0x61,
        0x72, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x6d, 0x73,
        0x67, 0x00, 0x00, 0x00, 0x00, 0x1a, 0x00, 0x00, 0x00, 0x01, 0x66, 0x75, 0x6e, 0x63, 0x00,
        0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xf2,
    ];

    // vasm -Fhunkexe output for CODE+DATA hunks, with a HUNK_RELOC32SHORT
    // patching an absolute reference to `value` (in the DATA hunk) into
    // the CODE hunk:
    //   move.l #value,a1 ; move.l (a1),d0 ; rts
    //   [data hunk] value: dc.l $deadbeef
    const TWO_HUNK_EXE_WITH_RELOC: &[u8] = &[
        0x00, 0x00, 0x03, 0xf3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x03, 0xe9, 0x00, 0x00, 0x00, 0x03, 0x43, 0xf9, 0x00, 0x00, 0x00, 0x00, 0x20, 0x11, 0x4e,
        0x75, 0x4e, 0x71, 0x00, 0x00, 0x03, 0xf7, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00,
        0x00, 0x00, 0x03, 0xf0, 0x00, 0x00, 0x00, 0x02, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xf2, 0x00, 0x00,
        0x03, 0xea, 0x00, 0x00, 0x00, 0x01, 0xde, 0xad, 0xbe, 0xef, 0x00, 0x00, 0x03, 0xf0, 0x00,
        0x00, 0x00, 0x02, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xf2,
    ];

    #[test]
    fn rejects_non_hunk_data() {
        let err = read_hunk_executable(&[0, 0, 0, 0], 0x1000).unwrap_err();
        assert!(err.0.contains("HUNK_HEADER"));
    }

    #[test]
    fn single_hunk_loads_at_requested_base() {
        let exe = read_hunk_executable(SINGLE_HUNK_EXE, 0x1000).unwrap();
        assert_eq!(exe.sections.len(), 1);
        assert_eq!(exe.sections[0].kind as u8, SectionKind::Code as u8);
        assert_eq!(exe.sections[0].address, 0x1000);
        assert_eq!(exe.sections[0].data.len(), 32);
        // moveq #1,d0
        assert_eq!(&exe.sections[0].data[0..2], &[0x70, 0x01]);
        assert_eq!(exe.load_base, 0x1000);
        assert_eq!(exe.image.len(), 32);
    }

    #[test]
    fn single_hunk_symbols_are_relocated_against_load_base() {
        let exe = read_hunk_executable(SINGLE_HUNK_EXE, 0x2000).unwrap();
        let symbols = exe.all_symbols();
        assert_eq!(symbols.len(), 3);
        assert!(symbols.contains(&("start".to_string(), 0x2000)));
        assert!(symbols.contains(&("msg".to_string(), 0x2000 + 0x1a)));
        assert!(symbols.contains(&("func".to_string(), 0x2000 + 0x10)));
    }

    #[test]
    fn two_hunk_reloc32short_patches_absolute_reference() {
        let exe = read_hunk_executable(TWO_HUNK_EXE_WITH_RELOC, 0x4000).unwrap();
        assert_eq!(exe.sections.len(), 2);

        let code = &exe.sections[0];
        let data = &exe.sections[1];
        assert_eq!(code.address, 0x4000);
        // code hunk is 3 longwords -> 12 bytes; data hunk follows immediately.
        assert_eq!(data.address, 0x4000 + 12);

        // `move.l #value,a1` is `43F9 <abs32>` at offset 0; the relocation
        // patches the address field (offset 2) to point at the data hunk.
        let patched = u32::from_be_bytes(code.data[2..6].try_into().unwrap());
        assert_eq!(patched, data.address);
    }

    #[test]
    fn rejects_resident_library_first_hunk() {
        // HUNK_HEADER layout: [0..4)=magic [4..8)=lib-name-terminator
        // [8..12)=hunk_count [12..16)=first_hunk [16..20)=last_hunk.
        // Set first_hunk=1 — resident libraries aren't supported.
        let mut data = SINGLE_HUNK_EXE.to_vec();
        data[15] = 1;
        let err = read_hunk_executable(&data, 0x1000).unwrap_err();
        assert!(err.0.contains("resident librar"));
    }
}
