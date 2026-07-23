//! UAE Extended ADF backend (UAE-0ADF / UAE-1ADF format).
//!
//! Parses UAE's extended ADF format which stores raw MFM bitstreams per
//! track. Used for copy-protected disks and non-standard formats.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::floppy_base::{FloppyError, FloppyImageReader, require_file};
use crate::mfm::MfmDecoder;

const SIDES: u32 = 2;

struct TrackEntry {
    track_type: u8,
    size: u32,
    file_offset: u64,
}

/// Read UAE extended ADF files (UAE-0ADF / UAE-1ADF).
///
/// These files store raw MFM bitstreams per track rather than decoded
/// sectors. Sector extraction is performed by MFM decoding the raw
/// bitstreams.
#[derive(Debug)]
pub struct UaeExtendedBackend {
    signature: [u8; 8],
    raw_tracks: HashMap<(u32, u32), Vec<u8>>,
    decoded_sectors: HashMap<(u32, u32, u32), Vec<u8>>,
}

impl UaeExtendedBackend {
    /// Open and validate a UAE extended ADF image, decoding all track sectors up front.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if the file doesn't exist, isn't UAE-ADF
    /// signed, or the track table is truncated/invalid.
    pub fn open(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let filepath = require_file(filepath.as_ref())?;
        let mut file = File::open(&filepath).map_err(|e| FloppyError::new(e.to_string()))?;

        let mut header = [0u8; 12];
        file.read_exact(&mut header)
            .map_err(|_| FloppyError::new("File too small for UAE-ADF header"))?;

        let mut signature = [0u8; 8];
        signature.copy_from_slice(&header[0..8]);
        if !signature.starts_with(b"UAE-") || !signature.ends_with(b"ADF") {
            return Err(FloppyError::new(format!(
                "Not a UAE extended ADF: {}",
                String::from_utf8_lossy(&signature)
            )));
        }

        let num_entries = u32::from_be_bytes(header[8..12].try_into().unwrap());
        if num_entries == 0 || num_entries > 200 {
            return Err(FloppyError::new(format!(
                "Invalid number of track entries: {}",
                num_entries
            )));
        }

        let track_data_base = 12 + num_entries as u64 * 12;

        let mut table_data = vec![0u8; num_entries as usize * 12];
        file.seek(SeekFrom::Start(12))
            .map_err(|e| FloppyError::new(e.to_string()))?;
        file.read_exact(&mut table_data)
            .map_err(|_| FloppyError::new("Truncated track table"))?;

        let mut entries = Vec::with_capacity(num_entries as usize);
        let mut file_offset = track_data_base;
        for i in 0..num_entries as usize {
            let base = i * 12;
            // 12-byte entry: reserved(2) | revolutions-1(1) | type(1) | len(4 BE) | bitlen(4 BE).
            // The type byte (0=normal AmigaDOS, 1=raw MFM) sits at offset 3, not combined
            // with the reserved/revolutions bytes into a single 4-byte value - a track with
            // revolutions>1 (common for copy-protected multi-revolution captures, the main
            // use case for this format) or a nonzero reserved byte would otherwise be
            // misread as a non-MFM track and silently dropped.
            let track_type = table_data[base + 3];
            let size = u32::from_be_bytes(table_data[base + 4..base + 8].try_into().unwrap());
            entries.push(TrackEntry {
                track_type,
                size,
                file_offset,
            });
            file_offset += size as u64;
        }

        let mut raw_tracks = HashMap::new();
        let mut decoded_sectors = HashMap::new();

        let mut track_num = 0u32;
        let mut side = 0u32;
        for entry in &entries {
            if entry.track_type != 1 {
                track_num += 1;
                side = 0;
                continue;
            }
            file.seek(SeekFrom::Start(entry.file_offset))
                .map_err(|e| FloppyError::new(e.to_string()))?;
            let mut raw_data = vec![0u8; entry.size as usize];
            file.read_exact(&mut raw_data).map_err(|_| {
                FloppyError::new(format!(
                    "Could not read full track data at offset {}",
                    entry.file_offset
                ))
            })?;

            decode_track_sectors(track_num, side, &raw_data, &mut decoded_sectors);
            raw_tracks.insert((track_num, side), raw_data);

            side = 1 - side;
            if side == 0 {
                track_num += 1;
            }
        }

        Ok(Self {
            signature,
            raw_tracks,
            decoded_sectors,
        })
    }

    /// Number of tracks with raw data present (both sides counted as one track).
    pub fn track_count(&self) -> usize {
        self.raw_tracks.len() / SIDES as usize
    }

    /// UAE-ADF signature bytes (e.g. `b"UAE-1ADF"`), for tests/diagnostics.
    pub fn signature(&self) -> &[u8; 8] {
        &self.signature
    }

    /// Return the raw MFM bitstream for a track.
    pub fn get_raw_track(&self, track: u32, side: u32) -> Result<&[u8], FloppyError> {
        self.raw_tracks
            .get(&(track, side))
            .map(|v| v.as_slice())
            .ok_or_else(|| {
                FloppyError::new(format!("No track data for track {}, side {}", track, side))
            })
    }
}

/// Decode all sectors from a track's raw MFM data and cache them.
///
/// `cylinder`/`side` are the logical (0-79, 0-1) coordinates used as the
/// public `read_sector` key. The MFM sector headers embed the raw Amiga
/// track number instead (`cylinder*2+side`, 0-159) - that's what must be
/// passed to `decode_track` as `expected_track`, since `MfmDecoder`
/// compares against the header's raw value directly (see
/// `mfm.rs::decode_sector_from_raw`, and its own `test_decode_track_high_number`).
fn decode_track_sectors(
    cylinder: u32,
    side: u32,
    raw_data: &[u8],
    decoded_sectors: &mut HashMap<(u32, u32, u32), Vec<u8>>,
) {
    let Ok(decoder) = MfmDecoder::new(raw_data) else {
        return;
    };
    let raw_track = cylinder * SIDES + side;
    let Ok(decoded) = decoder.decode_track(Some(raw_track), Some(side)) else {
        return;
    };
    for sector in decoded {
        decoded_sectors.insert((cylinder, side, sector.sector), sector.data);
    }
}

impl FloppyImageReader for UaeExtendedBackend {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        let key = (track, side, sector);
        if let Some(data) = self.decoded_sectors.get(&key) {
            return Ok(data.clone());
        }

        // Re-attempt decoding, in case the up-front decode at open() time
        // failed for this track but the raw data is present.
        if let Some(raw_data) = self.raw_tracks.get(&(track, side)).cloned() {
            decode_track_sectors(track, side, &raw_data, &mut self.decoded_sectors);
        }

        self.decoded_sectors.get(&key).cloned().ok_or_else(|| {
            FloppyError::new(format!(
                "Sector {} not found on track {}, side {}",
                sector, track, side
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_uae1adf_file(dir: &Path, num_tracks: u32, track_size: u32) -> PathBuf {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"UAE-1ADF");
        buf.extend_from_slice(&num_tracks.to_be_bytes());

        let mut offset = 12 + num_tracks * 12;
        for _ in 0..num_tracks {
            buf.extend_from_slice(&1u32.to_be_bytes());
            buf.extend_from_slice(&track_size.to_be_bytes());
            buf.extend_from_slice(&offset.to_be_bytes());
            offset += track_size;
        }
        for _ in 0..num_tracks {
            buf.extend(vec![0u8; track_size as usize]);
        }

        let path = dir.join("test.adf");
        std::fs::write(&path, buf).unwrap();
        path
    }

    #[test]
    fn test_valid_uae1adf() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let reader = UaeExtendedBackend::open(&path).unwrap();
        assert_eq!(reader.signature(), b"UAE-1ADF");
        assert_eq!(reader.track_count(), 80);
    }

    #[test]
    fn test_raw_track_access() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let reader = UaeExtendedBackend::open(&path).unwrap();
        let raw = reader.get_raw_track(0, 0).unwrap();
        assert_eq!(raw.len(), 12812);
    }

    #[test]
    fn test_read_sector_raises_floppy_error_for_invalid_data() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let mut reader = UaeExtendedBackend::open(&path).unwrap();
        assert!(reader.read_sector(0, 0, 0).is_err());
    }

    #[test]
    fn test_get_bootblock_raises_floppy_error_for_invalid_data() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let mut reader = UaeExtendedBackend::open(&path).unwrap();
        assert!(reader.get_bootblock().is_err());
    }

    #[test]
    fn test_file_not_found() {
        let dir = tempdir();
        let path = dir.path().join("nonexistent.adf");
        let err = UaeExtendedBackend::open(&path).unwrap_err();
        assert!(err.0.contains("File not found"));
    }

    #[test]
    fn test_invalid_signature() {
        let dir = tempdir();
        let path = dir.path().join("bad.adf");
        let mut data = b"NOTUAE".to_vec();
        data.extend(vec![0u8; 100]);
        std::fs::write(&path, data).unwrap();
        let err = UaeExtendedBackend::open(&path).unwrap_err();
        assert!(err.0.contains("Not a UAE extended ADF"));
    }

    // Regression tests for the track-number/flags-byte bugs: build tracks with real
    // MFM-encoded sectors (not the all-zero placeholder tracks the tests above use,
    // which never exercise MfmDecoder's track-number comparison) and verify both
    // cylinder/side combinations decode, and that a multi-revolution track entry
    // (revolutions-1 != 0) is still recognized as a raw-MFM track.

    fn encode_mfm_long(value: u32) -> (u32, u32) {
        let odd = (value & 0xAAAA_AAAA) >> 1;
        let even = value & 0x5555_5555;
        (odd, even)
    }

    fn build_valid_sector(
        raw_track: u32,
        sector: u32,
        sectors_to_end: u32,
        data: &[u8],
    ) -> Vec<u8> {
        const SECTOR_RAW_SIZE: usize = 1088;
        let mut sector_raw = vec![0u8; SECTOR_RAW_SIZE];

        sector_raw[0..2].copy_from_slice(&0xAAAAu16.to_be_bytes());
        sector_raw[2..4].copy_from_slice(&0xAAAAu16.to_be_bytes());
        sector_raw[4..6].copy_from_slice(&0x4489u16.to_be_bytes());
        sector_raw[6..8].copy_from_slice(&0x4489u16.to_be_bytes());

        let info_value = (0xFFu32 << 24) | (raw_track << 16) | (sector << 8) | sectors_to_end;
        let (info_odd, info_even) = encode_mfm_long(info_value);
        sector_raw[0x08..0x0C].copy_from_slice(&info_odd.to_be_bytes());
        sector_raw[0x0C..0x10].copy_from_slice(&info_even.to_be_bytes());

        // label longs are zero, so the header checksum is just info_odd ^ info_even.
        let checksum = (info_odd ^ info_even) & 0x5555_5555;
        let (cksum_odd, cksum_even) = encode_mfm_long(checksum);
        sector_raw[0x30..0x34].copy_from_slice(&cksum_odd.to_be_bytes());
        sector_raw[0x34..0x38].copy_from_slice(&cksum_even.to_be_bytes());

        let mut data_odd = vec![0u8; 512];
        let mut data_even = vec![0u8; 512];
        for (i, &byte) in data.iter().enumerate() {
            data_odd[i] = (byte & 0xAA) >> 1;
            data_even[i] = byte & 0x55;
        }
        let mut data_checksum = 0u32;
        for i in 0..512 / 4 {
            data_checksum ^= u32::from_be_bytes(data_odd[i * 4..i * 4 + 4].try_into().unwrap());
            data_checksum ^= u32::from_be_bytes(data_even[i * 4..i * 4 + 4].try_into().unwrap());
        }
        data_checksum &= 0x5555_5555;
        let (data_cksum_odd, data_cksum_even) = encode_mfm_long(data_checksum);
        sector_raw[0x38..0x3C].copy_from_slice(&data_cksum_odd.to_be_bytes());
        sector_raw[0x3C..0x40].copy_from_slice(&data_cksum_even.to_be_bytes());

        sector_raw[0x40..0x40 + 512].copy_from_slice(&data_odd);
        sector_raw[0x240..0x240 + 512].copy_from_slice(&data_even);
        sector_raw
    }

    /// Build a raw MFM track containing one sector, keyed by the raw Amiga track
    /// number (`cylinder*2+side`), as it would appear in a real capture.
    fn build_raw_track(raw_track: u32, data_byte: u8) -> Vec<u8> {
        let mut track = Vec::new();
        for _ in 0..20 {
            track.extend_from_slice(&0xAAAAu16.to_be_bytes());
        }
        track.extend(build_valid_sector(raw_track, 0, 1, &[data_byte; 512]));
        track
    }

    fn make_uae1adf_file_with_tracks(dir: &Path, tracks: &[(Vec<u8>, u8)]) -> PathBuf {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"UAE-1ADF");
        buf.extend_from_slice(&(tracks.len() as u32).to_be_bytes());

        let mut offset = 12 + tracks.len() as u32 * 12;
        for (data, revolutions_minus_1) in tracks {
            buf.push(0); // reserved high byte
            buf.push(0); // reserved low byte
            buf.push(*revolutions_minus_1);
            buf.push(1); // type = 1 (raw MFM)
            buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
            buf.extend_from_slice(&offset.to_be_bytes());
            offset += data.len() as u32;
        }
        for (data, _) in tracks {
            buf.extend_from_slice(data);
        }

        let path = dir.join("test.adf");
        std::fs::write(&path, buf).unwrap();
        path
    }

    #[test]
    fn test_read_sector_both_sides_with_real_mfm_headers() {
        let dir = tempdir();
        // cylinder 0, side 0 -> raw_track 0; cylinder 0, side 1 -> raw_track 1.
        let tracks = vec![(build_raw_track(0, 0x11), 0), (build_raw_track(1, 0x22), 0)];
        let path = make_uae1adf_file_with_tracks(dir.path(), &tracks);
        let mut reader = UaeExtendedBackend::open(&path).unwrap();

        let side0 = reader.read_sector(0, 0, 0).unwrap();
        assert_eq!(side0, vec![0x11u8; 512]);

        let side1 = reader.read_sector(0, 1, 0).unwrap();
        assert_eq!(side1, vec![0x22u8; 512]);
    }

    #[test]
    fn test_multi_revolution_track_still_decoded() {
        let dir = tempdir();
        // revolutions-1 = 1 (two revolutions captured) must not be mistaken for a
        // non-MFM track type.
        let tracks = vec![(build_raw_track(0, 0x33), 1)];
        let path = make_uae1adf_file_with_tracks(dir.path(), &tracks);
        let mut reader = UaeExtendedBackend::open(&path).unwrap();

        assert!(reader.get_raw_track(0, 0).is_ok());
        assert_eq!(reader.read_sector(0, 0, 0).unwrap(), vec![0x33u8; 512]);
    }

    fn tempdir() -> TempDir {
        TempDir::new()
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let mut dir = std::env::temp_dir();
            dir.push(format!(
                "m68k_floppy_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
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
