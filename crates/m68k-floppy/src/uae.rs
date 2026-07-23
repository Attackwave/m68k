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
    flags: u32,
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
            let flags = u32::from_be_bytes(table_data[base..base + 4].try_into().unwrap());
            let size = u32::from_be_bytes(table_data[base + 4..base + 8].try_into().unwrap());
            entries.push(TrackEntry {
                flags,
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
            if entry.flags != 1 {
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
fn decode_track_sectors(
    track: u32,
    side: u32,
    raw_data: &[u8],
    decoded_sectors: &mut HashMap<(u32, u32, u32), Vec<u8>>,
) {
    let Ok(decoder) = MfmDecoder::new(raw_data) else {
        return;
    };
    let Ok(decoded) = decoder.decode_track(Some(track), Some(side)) else {
        return;
    };
    for sector in decoded {
        decoded_sectors.insert((track, side, sector.sector), sector.data);
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
