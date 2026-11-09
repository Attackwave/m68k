//! Native Rust IPF reader (clean-room implementation).
//!
//! An IPF file is a sequence of records, each starting with a 12-byte
//! header: a four-character name, the record length (header included) and
//! a CRC-32 of the record taken with the CRC field zeroed. `CAPS` comes
//! first, `INFO` describes the disk, one `IMGE` per track names its
//! cylinder, head, block count and data key, and the `DATA` record with
//! the same key is followed by the track's extra data: one 32-byte
//! descriptor per block, then the blocks' element streams, covered by a
//! CRC-32 of their own.
//!
//! An element stream is a list of `(type, size, payload)` elements: sync
//! and raw elements hold MFM cells as recorded, data and gap elements hold
//! decoded bytes that are MFM-encoded here, fuzzy elements carry no payload.
//! Each track is rebuilt as a cell stream, every Amiga sector is located by
//! its `0x4489 0x4489` sync at any bit offset and handed to
//! [`crate::mfm::MfmDecoder`].
//!
//! A proprietary-library-backed `capsimg` backend is intentionally not
//! implemented: it would require FFI bindings to a closed-source
//! `capsimg.{dll,so}` library that may not even be present in most
//! environments.

use std::collections::HashMap;
use std::path::Path;

use crate::floppy_base::{FloppyError, FloppyImageReader, require_file};
use crate::mfm::{MFM_SYNC_WORD, MfmDecoder, SECTOR_RAW_SIZE};

/// Largest IPF file read. Real double-density images are about 1 MB.
const MAX_FILE_SIZE: u64 = 64 << 20;

/// Largest track rebuilt, in MFM cells: eight times a high-density track.
const MAX_TRACK_CELLS: usize = 1 << 21;

/// Most stream elements read for one track. A real track has a few per
/// sector; every block descriptor may point at the same stream, and a
/// stream of empty elements adds no cells, so without a count the work
/// grew with blocks times stream length.
const MAX_TRACK_ELEMENTS: usize = 1 << 16;

const RECORD_HEADER: usize = 12;
const INFO_SIZE: usize = 96;
const IMGE_SIZE: usize = 80;
const DATA_SIZE: usize = 28;
const BLOCK_DESCRIPTOR: usize = 32;

/// Block flag: element sizes count bits, not bytes.
const BLOCK_DATA_IN_BITS: u32 = 1 << 2;

/// CRC-32 (IEEE, reflected) as used by every IPF checksum.
fn crc32(chunks: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for byte in chunks.iter().flat_map(|c| c.iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().expect("4 bytes"))
}

/// One track of the image: its block count and extra data, or why the
/// extra data cannot be used.
#[derive(Debug)]
struct Track {
    blocks: u32,
    data: Result<Vec<u8>, String>,
}

/// Parse IPF files natively and decode their Amiga sectors.
#[derive(Debug)]
pub struct NativeIpfBackend {
    cylinders: u16,
    heads: u16,
    tracks: HashMap<(u32, u32), Track>,
    decoded_sectors: HashMap<(u32, u32, u32), Vec<u8>>,
    /// Tracks already decoded, with the error if decoding failed, so a
    /// track is rebuilt once and not on every sector read.
    decoded_tracks: HashMap<(u32, u32), Result<(), String>>,
}

impl NativeIpfBackend {
    /// Open and validate an IPF image.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if the file doesn't exist, is too large, a
    /// record is truncated or fails its CRC, or the mandatory `CAPS`/`INFO`
    /// records are missing.
    pub fn open(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let path = require_file(filepath.as_ref())?;
        let size = std::fs::metadata(&path)
            .map_err(|e| FloppyError::new(e.to_string()))?
            .len();
        if size > MAX_FILE_SIZE {
            return Err(FloppyError::new(format!(
                "IPF file is {size} bytes, more than the {MAX_FILE_SIZE} supported"
            )));
        }
        let bytes = std::fs::read(&path).map_err(|e| FloppyError::new(e.to_string()))?;
        Self::parse(&bytes).map_err(FloppyError::new)
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut info = None;
        // data key -> (cylinder, head, block count) from IMGE records
        let mut images: HashMap<u32, (u32, u32, u32)> = HashMap::new();
        let mut payloads: HashMap<u32, Result<Vec<u8>, String>> = HashMap::new();
        let mut pos = 0;
        while pos < bytes.len() {
            let header = bytes
                .get(pos..pos + RECORD_HEADER)
                .ok_or_else(|| format!("IPF record header at offset {pos} is truncated"))?;
            let name: [u8; 4] = header[..4].try_into().expect("4 bytes");
            let shown = String::from_utf8_lossy(&name).into_owned();
            let len = be32(header, 4) as usize;
            let record = bytes
                .get(pos..pos.saturating_add(len))
                .filter(|_| len >= RECORD_HEADER)
                .ok_or_else(|| {
                    format!("IPF record {shown} at offset {pos} has an invalid length {len}")
                })?;
            if crc32(&[&record[..8], &[0; 4], &record[12..]]) != be32(header, 8) {
                return Err(format!("IPF record {shown} at offset {pos}: CRC mismatch"));
            }
            if pos == 0 && &name != b"CAPS" {
                return Err("Not a valid IPF file: missing CAPS header".into());
            }
            let need = |size: usize| {
                if len < size {
                    Err(format!(
                        "IPF record {shown} is {len} bytes, expected {size}"
                    ))
                } else {
                    Ok(())
                }
            };
            pos += len;
            match &name {
                b"INFO" => {
                    need(INFO_SIZE)?;
                    let max_cylinder = be32(record, 40);
                    let max_head = be32(record, 48);
                    info = Some((
                        u16::try_from(max_cylinder.saturating_add(1)).unwrap_or(u16::MAX),
                        u16::try_from(max_head.saturating_add(1)).unwrap_or(u16::MAX),
                    ));
                }
                b"IMGE" => {
                    need(IMGE_SIZE)?;
                    let track = (be32(record, 12), be32(record, 16), be32(record, 52));
                    images.insert(be32(record, 64), track);
                }
                b"DATA" => {
                    need(DATA_SIZE)?;
                    let size = be32(record, 12) as usize;
                    let extra = bytes.get(pos..pos.saturating_add(size)).ok_or_else(|| {
                        format!("IPF DATA record at offset {} runs past the end", pos - len)
                    })?;
                    pos += size;
                    let data = if size == 0 || crc32(&[extra]) == be32(record, 20) {
                        Ok(extra.to_vec())
                    } else {
                        Err("track data fails its CRC".to_string())
                    };
                    payloads.insert(be32(record, 24), data);
                }
                _ => {}
            }
        }
        if bytes.is_empty() {
            return Err("Not a valid IPF file: missing CAPS header".into());
        }
        let (cylinders, heads) = info.ok_or("Not a valid IPF file: missing INFO chunk")?;
        let tracks = images
            .into_iter()
            .map(|(key, (cylinder, head, blocks))| {
                let data = payloads
                    .remove(&key)
                    .unwrap_or_else(|| Err("no DATA record for the track".into()));
                ((cylinder, head), Track { blocks, data })
            })
            .collect();
        Ok(Self {
            cylinders,
            heads,
            tracks,
            decoded_sectors: HashMap::new(),
            decoded_tracks: HashMap::new(),
        })
    }

    /// Cylinder count from the INFO record, for diagnostics/tests.
    pub fn cylinders(&self) -> u16 {
        self.cylinders
    }

    /// Head count from the INFO record, for diagnostics/tests.
    pub fn heads(&self) -> u16 {
        self.heads
    }

    /// Decode all sectors of a track and cache them.
    ///
    /// The MFM sector headers carry the raw Amiga track number
    /// (`cylinder*2+head`), which is what `MfmDecoder` compares against.
    fn decode_track_sectors(&mut self, track: u32, side: u32) -> Result<(), String> {
        let entry = self
            .tracks
            .get(&(track, side))
            .ok_or_else(|| format!("No track {track}, side {side} in the IPF image"))?;
        let data = entry.data.as_ref().map_err(Clone::clone)?;
        let framed = frame_sectors(&track_cells(data, entry.blocks)?);
        if framed.is_empty() {
            return Ok(());
        }
        let decoded = MfmDecoder::new(&framed)
            .and_then(|d| d.decode_track(Some(track * 2 + side), Some(side)))
            .map_err(|e| e.0)?;
        // A sector whose MFM checksums fail is not returned: its data (or
        // even its sector number) is corrupt.
        for sector in decoded
            .into_iter()
            .filter(|s| s.header_checksum_ok && s.data_checksum_ok)
        {
            self.decoded_sectors
                .insert((track, side, sector.sector), sector.data);
        }
        Ok(())
    }
}

/// An MFM cell stream, most significant bit first.
#[derive(Default)]
struct Cells {
    bytes: Vec<u8>,
    len: usize,
    /// Last data bit written, which decides the next clock bit.
    previous: bool,
}

impl Cells {
    fn reserve(&self, cells: u64) -> Result<(), String> {
        if (self.len as u64).saturating_add(cells) > MAX_TRACK_CELLS as u64 {
            return Err(format!("track exceeds {MAX_TRACK_CELLS} MFM cells"));
        }
        Ok(())
    }

    fn push(&mut self, cell: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if cell {
            self.bytes[self.len / 8] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    fn bit(&self, index: usize) -> bool {
        self.bytes[index / 8] & (0x80 >> (index % 8)) != 0
    }

    /// Eight cells starting at an arbitrary cell index.
    fn byte_at(&self, index: usize) -> u8 {
        (0..8).fold(0, |acc, i| acc << 1 | u8::from(self.bit(index + i)))
    }

    /// Cells as recorded (sync and raw elements).
    fn raw(&mut self, src: &[u8], bits: usize) {
        for i in 0..bits {
            let cell = src[i / 8] & (0x80 >> (i % 8)) != 0;
            self.push(cell);
            self.previous = cell;
        }
    }

    /// One MFM-encoded data bit: a clock cell, set only between two zeros.
    fn data_bit(&mut self, bit: bool) {
        self.push(!self.previous && !bit);
        self.push(bit);
        self.previous = bit;
    }

    fn data(&mut self, src: &[u8], bits: usize) {
        for i in 0..bits {
            self.data_bit(src[i / 8] & (0x80 >> (i % 8)) != 0);
        }
    }

    /// `cells` cells of encoded zero bytes. Gap content only pads the
    /// layout; sectors are found by their sync, not by position.
    fn gap(&mut self, cells: usize) {
        for _ in 0..cells / 2 {
            self.data_bit(false);
        }
        if cells % 2 == 1 {
            self.push(!self.previous);
        }
    }
}

/// Rebuild a track's MFM cells from its block descriptors and element
/// streams.
fn track_cells(data: &[u8], blocks: u32) -> Result<Cells, String> {
    let truncated = || "IPF track data is truncated".to_string();
    let mut cells = Cells::default();
    let mut elements = 0usize;
    for block in 0..blocks as usize {
        let at = block * BLOCK_DESCRIPTOR;
        let desc = data.get(at..at + BLOCK_DESCRIPTOR).ok_or_else(truncated)?;
        let gap_bits = be32(desc, 4) as usize;
        let in_bits = be32(desc, 20) & BLOCK_DATA_IN_BITS != 0;
        let mut p = be32(desc, 28) as usize;
        loop {
            let head = *data.get(p).ok_or_else(truncated)?;
            p += 1;
            let kind = head & 0x1F;
            if kind == 0 {
                break;
            }
            elements += 1;
            if elements > MAX_TRACK_ELEMENTS {
                return Err(format!(
                    "track has more than {MAX_TRACK_ELEMENTS} data elements"
                ));
            }
            let width = usize::from(head >> 5);
            let size = data
                .get(p..p + width)
                .ok_or_else(truncated)?
                .iter()
                .fold(0u64, |acc, b| acc << 8 | u64::from(*b));
            p += width;
            let bits = if in_bits {
                size
            } else {
                size.saturating_mul(8)
            };
            match kind {
                // sync, raw: cells as recorded
                1 | 4 => {
                    cells.reserve(bits)?;
                    let n = (bits as usize).div_ceil(8);
                    let src = data.get(p..p + n).ok_or_else(truncated)?;
                    p += n;
                    cells.raw(src, bits as usize);
                }
                // data, gap: decoded bytes
                2 | 3 => {
                    cells.reserve(bits.saturating_mul(2))?;
                    let n = (bits as usize).div_ceil(8);
                    let src = data.get(p..p + n).ok_or_else(truncated)?;
                    p += n;
                    cells.data(src, bits as usize);
                }
                // fuzzy: bits that read differently every revolution
                5 => {
                    cells.reserve(bits.saturating_mul(2))?;
                    cells.data(&vec![0; (bits as usize).div_ceil(8)], bits as usize);
                }
                _ => return Err(format!("unknown IPF data element type {kind}")),
            }
        }
        cells.reserve(gap_bits as u64)?;
        cells.gap(gap_bits);
    }
    Ok(cells)
}

/// Copy every Amiga sector found in `cells`, at whatever bit offset its
/// sync lies, into a word-aligned buffer `MfmDecoder` can read: the
/// `0xAAAA 0xAAAA` preamble, the two sync words, then the sector's cells.
fn frame_sectors(cells: &Cells) -> Vec<u8> {
    const SYNC: u32 = (MFM_SYNC_WORD as u32) << 16 | MFM_SYNC_WORD as u32;
    let body = SECTOR_RAW_SIZE - 8;
    let mut framed = Vec::new();
    let mut window = 0u32;
    let (mut i, mut filled) = (0, 0);
    while i < cells.len {
        window = window << 1 | u32::from(cells.bit(i));
        filled += 1;
        let start = i + 1;
        if filled >= 32 && window == SYNC && start + body * 8 <= cells.len {
            framed.extend_from_slice(&[0xAA, 0xAA, 0xAA, 0xAA]);
            framed.extend_from_slice(&SYNC.to_be_bytes());
            framed.extend((0..body).map(|k| cells.byte_at(start + k * 8)));
            // Sectors do not overlap. Searching inside this one found a
            // "sync" every 16 cells in a track of repeated sync words and
            // copied a whole sector for each, about 3000 times the input.
            i = start + body * 8;
            (window, filled) = (0, 0);
            continue;
        }
        i += 1;
    }
    framed
}

impl FloppyImageReader for NativeIpfBackend {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        let key = (track, side, sector);
        if let Some(data) = self.decoded_sectors.get(&key) {
            return Ok(data.clone());
        }
        if !self.decoded_tracks.contains_key(&(track, side)) {
            let result = self.decode_track_sectors(track, side);
            self.decoded_tracks.insert((track, side), result);
        }
        if let Some(Err(e)) = self.decoded_tracks.get(&(track, side)) {
            return Err(FloppyError::new(format!("Track {track}, side {side}: {e}")));
        }
        self.decoded_sectors.get(&key).cloned().ok_or_else(|| {
            FloppyError::new(format!(
                "Sector {} not found on track {}, side {} (missing, or its checksum failed)",
                sector, track, side
            ))
        })
    }
}

/// Builds IPF images the way the SPS encoder lays them out, for tests.
#[cfg(test)]
pub(crate) mod test_image {
    use super::crc32;

    pub fn record(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let len = (12 + payload.len()) as u32;
        let mut rec = name.to_vec();
        rec.extend_from_slice(&len.to_be_bytes());
        let crc = crc32(&[&rec, &[0; 4], payload]);
        rec.extend_from_slice(&crc.to_be_bytes());
        rec.extend_from_slice(payload);
        rec
    }

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// `CAPS` and an `INFO` for an 80-cylinder, 2-head Amiga disk.
    pub fn header() -> Vec<u8> {
        let mut info = vec![1, 2, 1, 0, 1, 0, 0, 79, 0, 1, 0, 0, 1];
        info.resize(21, 0);
        let mut out = record(b"CAPS", &[]);
        out.extend(record(b"INFO", &words(&info)));
        out
    }

    /// The 16 data bits of an odd/even split long, as two decoded bytes.
    fn compress(split: u32) -> [u8; 2] {
        let v = (0..16)
            .rev()
            .fold(0u16, |acc, i| acc << 1 | ((split >> (2 * i)) & 1) as u16);
        v.to_be_bytes()
    }

    /// The decoded bytes following the sync of one Amiga sector.
    fn sector_body(raw_track: u32, sector: u32, data: &[u8; 512]) -> Vec<u8> {
        let odd = |v: u32| (v >> 1) & 0x5555_5555;
        let even = |v: u32| v & 0x5555_5555;
        let info = 0xFF << 24 | raw_track << 16 | sector << 8 | (11 - sector);
        let header = [odd(info), even(info), 0, 0, 0, 0, 0, 0, 0, 0];
        let header_sum = header.iter().fold(0, |a, x| a ^ x) & 0x5555_5555;
        let longs: Vec<u32> = data
            .chunks(4)
            .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
            .collect();
        let body: Vec<u32> = longs
            .iter()
            .map(|&v| odd(v))
            .chain(longs.iter().map(|&v| even(v)))
            .collect();
        let data_sum = body.iter().fold(0, |a, x| a ^ x) & 0x5555_5555;
        let mut split = header.to_vec();
        split.extend([
            odd(header_sum),
            even(header_sum),
            odd(data_sum),
            even(data_sum),
        ]);
        split.extend(body);
        split.into_iter().flat_map(compress).collect()
    }

    fn element(kind: u8, payload: &[u8], size: u16) -> Vec<u8> {
        let mut e = vec![kind | 2 << 5];
        e.extend_from_slice(&size.to_be_bytes());
        e.extend_from_slice(payload);
        e
    }

    /// `IMGE` and `DATA` records for one track of 11 sectors, sector `n`
    /// filled with `fill + n`. `in_bits` stores element sizes in bits.
    pub fn track(cylinder: u32, head: u32, fill: u8, in_bits: bool) -> Vec<u8> {
        let unit = if in_bits { 8 } else { 1 };
        let key = cylinder * 2 + head + 1;
        let streams: Vec<Vec<u8>> = (0..11)
            .map(|n| {
                let body = sector_body(cylinder * 2 + head, n, &[fill.wrapping_add(n as u8); 512]);
                let mut s = element(2, &[0, 0], 2 * unit);
                s.extend(element(1, &[0x44, 0x89, 0x44, 0x89], 4 * unit));
                s.extend(element(2, &body, body.len() as u16 * unit));
                s.push(0);
                s
            })
            .collect();
        let gap = 1600;
        let mut offset = 11 * 32;
        let mut extra = Vec::new();
        for (n, s) in streams.iter().enumerate() {
            let gap_bits = if n == 10 { gap } else { 0 };
            let flag = if in_bits { 4 } else { 0 };
            extra.extend(words(&[1088 * 8, gap_bits, 0, 1, 1, flag, 0, offset]));
            offset += s.len() as u32;
        }
        extra.extend(streams.concat());
        let data_bits = 11 * 1088 * 8;
        let image = [
            cylinder,
            head,
            2,
            1,
            (data_bits + gap).div_ceil(8),
            0,
            0,
            data_bits,
            gap,
            data_bits + gap,
            11,
            0,
            0,
            key,
            0,
            0,
            0,
        ];
        let mut out = record(b"IMGE", &words(&image));
        let crc = crc32(&[&extra]);
        out.extend(record(
            b"DATA",
            &words(&[extra.len() as u32, extra.len() as u32 * 8, crc, key]),
        ));
        out.extend(extra);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_image::{header, record, track};
    use super::*;
    use std::path::PathBuf;

    fn open_bytes(dir: &TempDir, bytes: &[u8]) -> Result<NativeIpfBackend, FloppyError> {
        let path = dir.path().join("test.ipf");
        std::fs::write(&path, bytes).unwrap();
        NativeIpfBackend::open(&path)
    }

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
    }

    #[test]
    fn test_valid_ipf() {
        let dir = tempdir();
        let reader = open_bytes(&dir, &header()).unwrap();
        assert_eq!(reader.cylinders(), 80);
        assert_eq!(reader.heads(), 2);
    }

    #[test]
    fn reads_every_sector_of_both_sides() {
        let dir = tempdir();
        let mut ipf = header();
        ipf.extend(track(0, 0, 0x10, false));
        ipf.extend(track(0, 1, 0x40, false));
        ipf.extend(track(40, 1, 0x80, false));
        let mut reader = open_bytes(&dir, &ipf).unwrap();
        for (cylinder, head, fill) in [(0, 0, 0x10u8), (0, 1, 0x40), (40, 1, 0x80)] {
            for n in 0..11 {
                let sector = reader.read_sector(cylinder, head, n).unwrap();
                assert_eq!(sector, vec![fill + n as u8; 512], "{cylinder}/{head}/{n}");
            }
        }
        assert!(reader.read_sector(1, 0, 0).is_err());
    }

    #[test]
    fn repeated_sync_words_frame_a_bounded_number_of_sectors() {
        let mut cells = Cells::default();
        let syncs = [0x44, 0x89].repeat(50_000);
        cells.raw(&syncs, syncs.len() * 8);
        let frame = SECTOR_RAW_SIZE;
        let framed = frame_sectors(&cells);
        assert!(framed.len() <= cells.len.div_ceil((frame - 8) * 8) * frame);
    }

    #[test]
    fn element_budget_bounds_shared_streams() {
        // Every block descriptor points at one long stream of empty
        // elements, which adds no cells but used to be walked per block.
        let blocks = 300u32;
        let mut data: Vec<u8> = (0..blocks)
            .flat_map(|_| {
                [0u32, 0, 0, 0, 0, 0, 0, blocks * 32]
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>()
            })
            .collect();
        data.extend(std::iter::repeat_n(0x01, 300));
        data.push(0);
        let err = track_cells(&data, blocks).err().unwrap();
        assert!(err.contains("data elements"), "{err}");
    }

    #[test]
    fn element_sizes_in_bits() {
        let dir = tempdir();
        let mut ipf = header();
        ipf.extend(track(3, 0, 0x22, true));
        let mut reader = open_bytes(&dir, &ipf).unwrap();
        assert_eq!(reader.read_sector(3, 0, 10).unwrap(), vec![0x2C; 512]);
    }

    #[test]
    fn record_crc_mismatch_is_rejected() {
        let dir = tempdir();
        let mut ipf = header();
        ipf.extend(track(0, 0, 0, false));
        let imge = ipf.windows(4).position(|w| w == b"IMGE").unwrap();
        ipf[imge + 15] ^= 1; // cylinder field
        let err = open_bytes(&dir, &ipf).unwrap_err();
        assert!(err.0.contains("IMGE") && err.0.contains("CRC"), "{}", err.0);
    }

    #[test]
    fn track_data_crc_mismatch_fails_only_that_track() {
        let dir = tempdir();
        let mut ipf = header();
        ipf.extend(track(0, 0, 0x10, false));
        ipf.extend(track(0, 1, 0x40, false));
        let last = ipf.len() - 100; // inside the second track's element streams
        ipf[last] ^= 0xFF;
        let mut reader = open_bytes(&dir, &ipf).unwrap();
        assert_eq!(reader.read_sector(0, 0, 0).unwrap(), vec![0x10; 512]);
        let err = reader.read_sector(0, 1, 0).unwrap_err();
        assert!(err.0.contains("CRC"), "{}", err.0);
    }

    #[test]
    fn element_size_beyond_the_track_limit_is_an_error() {
        let dir = tempdir();
        // A fuzzy element claiming 2^32 bytes, which carries no payload.
        let mut extra = [8 * 1088u32, 0, 0, 1, 1, 0, 0, 32]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<u8>>();
        extra.extend([5 | 5 << 5, 1, 0, 0, 0, 0, 0]);
        let mut ipf = header();
        let image: Vec<u8> = [0u32, 0, 2, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        ipf.extend(record(b"IMGE", &image));
        let data: Vec<u8> = [extra.len() as u32, 0, crc32(&[&extra]), 1]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        ipf.extend(record(b"DATA", &data));
        ipf.extend(extra);
        let mut reader = open_bytes(&dir, &ipf).unwrap();
        let err = reader.read_sector(0, 0, 0).unwrap_err();
        assert!(err.0.contains("MFM cells"), "{}", err.0);
    }

    /// A record length far beyond the end of the file is rejected before
    /// anything is allocated for it.
    #[test]
    fn test_chunk_size_exceeding_file_length_is_rejected() {
        let dir = tempdir();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"CAPS");
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 1]);
        let err = open_bytes(&dir, &buf).unwrap_err();
        assert!(err.0.contains("invalid length"), "{}", err.0);
    }

    #[test]
    fn test_missing_caps() {
        let dir = tempdir();
        let info = header().split_off(12);
        let err = open_bytes(&dir, &info).unwrap_err();
        assert!(err.0.contains("missing CAPS"), "{}", err.0);
    }

    #[test]
    fn test_missing_info() {
        let dir = tempdir();
        let err = open_bytes(&dir, &record(b"CAPS", &[])).unwrap_err();
        assert!(err.0.contains("missing INFO"), "{}", err.0);
    }

    #[test]
    fn test_file_not_found() {
        let dir = tempdir();
        let path = dir.path().join("nonexistent.ipf");
        let err = NativeIpfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("File not found"));
    }

    fn tempdir() -> TempDir {
        TempDir::new()
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            // Uniqueness comes from an atomic counter, not from the clock.
            // These tests run as threads in one binary, and four modules
            // share this helper, so a timestamp alone is not enough: on
            // platforms whose SystemTime resolution is coarser than a
            // nanosecond (macOS runners, in practice) two concurrent
            // TempDir::new() calls produced the *same* path, and the first
            // Drop then deleted the other test's directory — surfacing as
            // "No such file or directory" in an unrelated test.
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut dir = std::env::temp_dir();
            dir.push(format!(
                "m68k_floppy_test_ipf_{}_{}",
                std::process::id(),
                seq
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
