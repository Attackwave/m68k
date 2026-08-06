//! Reader for the classic AmigaOS executable "Hunk" format.
//!
//! Loads a `HUNK_HEADER`-based executable (as produced by reference encodings/`vlink`/the
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
/// instead of 32-bit ones. This is what modern linkers emit
/// by default, so it's at least as common as the classic `HUNK_RELOC32`.
///
/// The "correct" V39+ id for this is `0x03FC`, but `dos.library`'s
/// `LoadSeg()` has accepted `0x03F7` (nominally `HUNK_DREL32`) for this
/// purpose since V37 due to a historical bug, and every linker
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
    /// Absolute addresses (already offset by this hunk's load address) of
    /// every 32-bit longword patched by a `HUNK_RELOC32`/`HUNK_RELOC32SHORT`
    /// entry targeting any hunk.
    ///
    /// Each entry marks four bytes that hold a relocated pointer, which is
    /// data by definition — the linker said so. A disassembler can use this
    /// to avoid decoding pointer tables as instructions, and to treat the
    /// pointed-to addresses as code entry points when the target is a
    /// `Code` hunk. Sorted ascending and deduplicated.
    pub relocs: Vec<u32>,
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

    /// Absolute addresses of every relocated 32-bit pointer across all
    /// hunks, ascending. Each marks four bytes of pointer data rather than
    /// code — see [`Section::relocs`].
    pub fn all_relocs(&self) -> Vec<u32> {
        let mut all: Vec<u32> = self
            .sections
            .iter()
            .flat_map(|s| s.relocs.clone())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
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
        // Every caller derives `n` from an attacker-controlled length field
        // (e.g. `name()`'s word_count*4, or a hunk's word_count*4 body
        // size) with no upper bound of its own. Validating against the
        // reader's own remaining length before allocating (rather than
        // allocating `n` bytes and letting `read_exact` fail after the
        // fact) avoids a multi-gigabyte allocation from a few-byte input —
        // the same DoS pattern already fixed for the hunk-count/hunk-size
        // tables above (see `read_hunk_executable`'s comments).
        let remaining = self.cursor.get_ref().len() as u64 - self.cursor.position();
        if n as u64 > remaining {
            return Err(HunkError::new("unexpected end of file"));
        }
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
///
/// Each patched location is also recorded in `reloc_sites[i]` as an
/// absolute address, so callers can tell which longwords hold pointers
/// rather than code.
fn apply_relocations(
    contents: &mut [Vec<u8>],
    offsets: &[u32],
    reloc_sites: &mut [Vec<u32>],
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
        // `offset` is bounded by the hunk's own length, which the layout
        // loop already proved fits above `offsets[i]` without overflowing.
        reloc_sites[i].push(offsets[i].wrapping_add(offset as u32));
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
    // hunk_count comes straight from the file with no upper bound of its
    // own; the last_hunk+1==hunk_count check above doesn't help since
    // last_hunk is attacker-controlled too. Each hunk needs at least one
    // size longword (4 bytes) in the table that follows, so hunk_count
    // can never legitimately exceed the file's remaining length — a
    // crafted `hunk_count = 0xFFFFFFFF` previously drove
    // `Vec::with_capacity(hunk_count)` (a 16 GB `Vec<u32>` allocation)
    // before the first per-entry `u32()` read would have failed anyway.
    if hunk_count > data.len() {
        return Err(HunkError::new(format!(
            "hunk count {} exceeds file size {}",
            hunk_count,
            data.len()
        )));
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
    // Likewise, each hunk's own size (in longwords, so already x4 above)
    // is attacker-controlled — the loop below allocates one Vec<u8> per
    // hunk of exactly that size, so the sum needs bounding before doing
    // so (a single oversized hunk, or many moderate ones summing past the
    // file, would otherwise still allocate up to 4 GB per hunk).
    //
    // The bound cannot be the file size itself: a BSS hunk declares its
    // size in the header and contributes *no* bytes to the file, which is
    // its whole purpose. Real executables are routinely larger in memory
    // than on disk — vbcc's test binary declares 4340 bytes across three
    // hunks in a 3804-byte file — and rejecting those was wrong. Allow a
    // generous multiple of the file size instead, which still refuses the
    // crafted headers this check exists for.
    const MAX_MEMORY_TO_FILE_RATIO: u64 = 64;
    let total_hunk_bytes: u64 = hunk_sizes.iter().map(|&s| s as u64).sum();
    let budget = (data.len() as u64)
        .saturating_mul(MAX_MEMORY_TO_FILE_RATIO)
        .max(1 << 20);
    if total_hunk_bytes > budget {
        return Err(HunkError::new(format!(
            "total hunk size {} implausible for a {}-byte file",
            total_hunk_bytes,
            data.len()
        )));
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
    let mut reloc_sites: Vec<Vec<u32>> = vec![Vec::new(); hunk_count];

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
                    // `word_count` here is independent of (and, for a
                    // malformed file, need not agree with) the hunk's own
                    // size from the HUNK_HEADER size table that
                    // `contents[i]` was allocated with — a body longer
                    // than the hunk it belongs to must be a load error,
                    // not an out-of-bounds slice panic.
                    if body.len() > contents[i].len() {
                        return Err(HunkError::new(format!(
                            "hunk {} body ({} bytes) exceeds its declared size ({} bytes)",
                            i,
                            body.len(),
                            contents[i].len()
                        )));
                    }
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
                apply_relocations(
                    &mut contents,
                    &offsets,
                    &mut reloc_sites,
                    i,
                    target_hunk,
                    &offsets_list,
                )?;
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
                    apply_relocations(
                        &mut contents,
                        &offsets,
                        &mut reloc_sites,
                        i,
                        target_hunk,
                        &offsets_list,
                    )?;
                }
            }
            HUNK_SYMBOL => {
                while let Some(sym_name) = r.name()? {
                    let value = r.u32()?;
                    // `value` is an attacker-controlled 32-bit offset read
                    // straight from the file with no bound of its own;
                    // wrapping (matching real 32-bit address-space
                    // wraparound) rather than panicking on overflow is
                    // consistent with `Reader::skip`'s `checked_add` used
                    // for length accounting elsewhere in this module,
                    // just applied to an address computation instead of a
                    // cursor position.
                    symbols[i].push((sym_name, offsets[i].wrapping_add(value)));
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
            relocs: {
                // A malformed file may list the same offset twice, and
                // separate HUNK_RELOC32 blocks (one per target hunk) each
                // contribute to the same hunk in file order, not address
                // order — so sort and dedup rather than trusting the file.
                let mut sites = std::mem::take(&mut reloc_sites[idx]);
                sites.sort_unstable();
                sites.dedup();
                sites
            },
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

    // Reference hunk-executable output for a single CODE hunk with no relocations:
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

    // Reference hunk-executable output for CODE+DATA hunks, with a HUNK_RELOC32SHORT
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

    /// Regression: `hunk_count` came straight from the file with no
    /// upper bound, previously driving `Vec::with_capacity(hunk_count)`
    /// (a 16 GB `Vec<u32>` for `hunk_count = 0xFFFFFFFF`) before the
    /// first per-entry read would have failed anyway.
    #[test]
    fn rejects_absurd_hunk_count() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER magic
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // empty resident-library list
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // hunk_count: absurd
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // first_hunk
        buf.extend_from_slice(&0xFFFF_FFFEu32.to_be_bytes()); // last_hunk (consistent with hunk_count)

        let err = read_hunk_executable(&buf, 0x1000).unwrap_err();
        assert!(err.0.contains("hunk count"), "unexpected error: {}", err.0);
    }

    /// Regression: each hunk's own size (attacker-controlled, read from
    /// the file) previously drove `vec![0u8; s as usize]` per hunk with
    /// no bound against the actual file size.
    #[test]
    fn rejects_hunk_size_exceeding_file_length() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER magic
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // empty resident-library list
        buf.extend_from_slice(&1u32.to_be_bytes()); // hunk_count = 1
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // first_hunk
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // last_hunk
        buf.extend_from_slice(&0x0FFF_FFFFu32.to_be_bytes()); // hunk size: ~4GB in longwords

        let err = read_hunk_executable(&buf, 0x1000).unwrap_err();
        assert!(err.0.contains("hunk size"), "unexpected error: {}", err.0);
    }

    /// A BSS hunk declares its size in the header and contributes no bytes
    /// to the file, so a real executable is routinely larger in memory
    /// than on disk. Bounding declared size by the file size rejected
    /// those: vbcc's test binary declares 4340 bytes across three hunks in
    /// a 3804-byte file and could not be read at all.
    #[test]
    fn accepts_bss_hunk_larger_than_the_file() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // no resident libs
        buf.extend_from_slice(&2u32.to_be_bytes()); // hunk_count
        buf.extend_from_slice(&0u32.to_be_bytes()); // first_hunk
        buf.extend_from_slice(&1u32.to_be_bytes()); // last_hunk
        buf.extend_from_slice(&1u32.to_be_bytes()); // hunk 0: 1 longword of code
        buf.extend_from_slice(&64u32.to_be_bytes()); // hunk 1: 256 bytes of BSS
        // HUNK_CODE with one longword: rts + padding
        buf.extend_from_slice(&HUNK_CODE.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&[0x4E, 0x75, 0x4E, 0x71]);
        buf.extend_from_slice(&HUNK_END.to_be_bytes());
        // HUNK_BSS declares its length but carries no data.
        buf.extend_from_slice(&HUNK_BSS.to_be_bytes());
        buf.extend_from_slice(&64u32.to_be_bytes());
        buf.extend_from_slice(&HUNK_END.to_be_bytes());

        let exe = read_hunk_executable(&buf, 0x1000).unwrap();
        assert_eq!(exe.sections.len(), 2);
        assert_eq!(exe.sections[1].kind, SectionKind::Bss);
        // The BSS hunk occupies address space beyond the file's own length.
        assert_eq!(exe.sections[1].data.len(), 256);
        assert!(exe.image.len() > buf.len());
    }

    /// Fuzzing regression (cargo-fuzz `amiga_hunk_parse` target): `Reader::
    /// bytes(n)` used to allocate `vec![0u8; n]` before checking whether
    /// the reader actually had `n` bytes left, so a resident-library-name
    /// `word_count` field claiming ~4 billion bytes (from an 8-byte input:
    /// just the magic plus one word_count longword) drove a multi-gigabyte
    /// allocation immediately, well before the inevitable `read_exact`
    /// failure — an OOM from a handful of attacker bytes.
    #[test]
    fn rejects_absurd_name_length_without_oom() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER magic
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // resident-lib name word_count: absurd

        let err = read_hunk_executable(&buf, 0x1000).unwrap_err();
        assert!(
            err.0.contains("unexpected end of file"),
            "unexpected error: {}",
            err.0
        );
    }

    /// Fuzzing regression (cargo-fuzz `amiga_hunk_parse` target): a
    /// `HUNK_SYMBOL` value is a raw attacker-controlled `u32` added
    /// directly to the hunk's load offset with no bound of its own;
    /// `offsets[i] + value` panicked ("attempt to add with overflow")
    /// whenever the sum exceeded `u32::MAX`, e.g. a high `load_base` (or
    /// a large earlier hunk) combined with a large symbol value.
    #[test]
    fn hunk_symbol_value_overflow_does_not_panic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER magic
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // empty resident-library list
        buf.extend_from_slice(&1u32.to_be_bytes()); // hunk_count = 1
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // first_hunk
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // last_hunk
        buf.extend_from_slice(&0u32.to_be_bytes()); // hunk size: 0 longwords
        buf.extend_from_slice(&HUNK_CODE.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // word_count = 0 (empty body)
        buf.extend_from_slice(&HUNK_SYMBOL.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes()); // name word_count = 1 (4 bytes)
        buf.extend_from_slice(b"sym\0");
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // symbol value: max
        buf.extend_from_slice(&0u32.to_be_bytes()); // name list terminator
        buf.extend_from_slice(&HUNK_END.to_be_bytes());

        // load_base near u32::MAX makes offsets[0] + value overflow.
        let exe = read_hunk_executable(&buf, 0xFFFF_FFF0).unwrap();
        assert_eq!(exe.sections[0].symbols.len(), 1);
    }

    /// Fuzzing regression (cargo-fuzz `amiga_hunk_parse` target): a
    /// `HUNK_CODE`/`HUNK_DATA` block's own `word_count` field is
    /// independent of the hunk's size from the `HUNK_HEADER` size table
    /// that `contents[i]` was allocated with — both are attacker-controlled
    /// and a malformed file can disagree between them. A body longer than
    /// its declared hunk size panicked ("range end index out of range")
    /// on the fixed-size slice write instead of erroring cleanly.
    #[test]
    fn rejects_hunk_body_larger_than_declared_hunk_size() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x03, 0xf3]); // HUNK_HEADER magic
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // empty resident-library list
        buf.extend_from_slice(&1u32.to_be_bytes()); // hunk_count = 1
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // first_hunk
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // last_hunk
        buf.extend_from_slice(&0u32.to_be_bytes()); // declared hunk size: 0 longwords
        buf.extend_from_slice(&HUNK_CODE.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes()); // body word_count = 1 (4 bytes) -- disagrees
        buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        buf.extend_from_slice(&HUNK_END.to_be_bytes());

        let err = read_hunk_executable(&buf, 0x1000).unwrap_err();
        assert!(
            err.0.contains("exceeds its declared size"),
            "unexpected error: {}",
            err.0
        );
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

    /// The reloc offsets were previously consumed by `apply_relocations`
    /// and dropped, so nothing downstream could tell which longwords hold
    /// relocated pointers rather than code. They are now retained as
    /// absolute addresses.
    #[test]
    fn reloc_sites_are_retained_as_absolute_addresses() {
        let exe = read_hunk_executable(TWO_HUNK_EXE_WITH_RELOC, 0x4000).unwrap();

        // The single relocation patches offset 2 of the code hunk, which
        // loads at 0x4000 — so the pointer longword sits at 0x4002.
        assert_eq!(exe.sections[0].relocs, vec![0x4002]);
        // The data hunk holds the pointed-to value, not a pointer itself.
        assert!(exe.sections[1].relocs.is_empty());
        assert_eq!(exe.all_relocs(), vec![0x4002]);
    }

    /// Reloc addresses must follow the load base, like symbols do.
    #[test]
    fn reloc_sites_move_with_load_base() {
        let exe = read_hunk_executable(TWO_HUNK_EXE_WITH_RELOC, 0x10000).unwrap();
        assert_eq!(exe.all_relocs(), vec![0x10002]);
    }

    /// An executable with no relocation hunks at all must report none,
    /// rather than e.g. inheriting a stale list from another hunk.
    #[test]
    fn executable_without_relocs_reports_none() {
        let exe = read_hunk_executable(SINGLE_HUNK_EXE, 0x1000).unwrap();
        assert!(exe.all_relocs().is_empty());
        assert!(exe.sections[0].relocs.is_empty());
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
