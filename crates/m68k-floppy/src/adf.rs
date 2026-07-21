//! ADF (Amiga Disk Format) backend for floppy image reading.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::floppy_base::{FloppyError, FloppyImageReader, require_file};

const SECTOR_SIZE: u64 = 512;
const SIDES: u64 = 2;

/// Read unmodified ADF images (standard and extended).
#[derive(Debug)]
pub struct AdfBackend {
    filepath: PathBuf,
    tracks: u32,
    sectors_per_track: u32,
}

impl AdfBackend {
    /// Open and validate an ADF image.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if the file doesn't exist or its size isn't a
    /// multiple of the 512-byte sector size.
    pub fn open(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let filepath = require_file(filepath.as_ref())?;
        let file_size = std::fs::metadata(&filepath)
            .map_err(|e| FloppyError::new(e.to_string()))?
            .len();

        if file_size % SECTOR_SIZE != 0 {
            return Err(FloppyError::new(format!(
                "Invalid ADF size: {} bytes (not a multiple of {})",
                file_size, SECTOR_SIZE
            )));
        }
        if file_size == 0 {
            return Err(FloppyError::new("Empty ADF file"));
        }

        // Known sizes: DD (80 tracks, 11 sectors/track), HD (80 tracks, 22 sectors/track).
        let (tracks, spt) = if file_size == 80 * SIDES * 11 * SECTOR_SIZE {
            (80, 11)
        } else if file_size == 80 * SIDES * 22 * SECTOR_SIZE {
            (80, 22)
        } else {
            let tracks = 80u64;
            (tracks, file_size / (tracks * SIDES * SECTOR_SIZE))
        };

        if spt == 0 {
            return Err(FloppyError::new(format!(
                "ADF file too small to contain a full track: {} bytes",
                file_size
            )));
        }

        Ok(Self {
            filepath,
            tracks: tracks as u32,
            sectors_per_track: spt as u32,
        })
    }
}

impl FloppyImageReader for AdfBackend {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        if track >= self.tracks {
            return Err(FloppyError::new(format!(
                "Track {} out of range (0-{})",
                track,
                self.tracks - 1
            )));
        }
        if side > 1 {
            return Err(FloppyError::new(format!(
                "Side {} out of range (0-1)",
                side
            )));
        }
        if sector >= self.sectors_per_track {
            return Err(FloppyError::new(format!(
                "Sector {} out of range (0-{})",
                sector,
                self.sectors_per_track.saturating_sub(1)
            )));
        }

        let offset = (track as u64 * SIDES * self.sectors_per_track as u64
            + side as u64 * self.sectors_per_track as u64
            + sector as u64)
            * SECTOR_SIZE;

        let mut file = File::open(&self.filepath).map_err(|e| FloppyError::new(e.to_string()))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| FloppyError::new(e.to_string()))?;
        let mut data = vec![0u8; SECTOR_SIZE as usize];
        file.read_exact(&mut data).map_err(|_| {
            FloppyError::new(format!("Could not read full sector at offset {}", offset))
        })?;

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADF_DD_SIZE: usize = 80 * 2 * 11 * 512;

    fn make_adf_file(dir: &Path, fill_byte: u8) -> PathBuf {
        let path = dir.join("test.adf");
        std::fs::write(&path, vec![fill_byte; ADF_DD_SIZE]).unwrap();
        path
    }

    #[test]
    fn test_valid_image() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        assert!(AdfBackend::open(&path).is_ok());
    }

    #[test]
    fn test_invalid_size() {
        let dir = tempdir();
        let path = dir.path().join("bad.adf");
        std::fs::write(&path, vec![0u8; 1000]).unwrap();
        let err = AdfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("Invalid ADF size"));
    }

    #[test]
    fn test_read_sector() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xBB);
        let mut reader = AdfBackend::open(&path).unwrap();
        let data = reader.read_sector(0, 0, 0).unwrap();
        assert_eq!(data, vec![0xBBu8; 512]);
    }

    #[test]
    fn test_read_sector_different_location() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xCC);
        let mut reader = AdfBackend::open(&path).unwrap();
        let data = reader.read_sector(40, 1, 5).unwrap();
        assert_eq!(data, vec![0xCCu8; 512]);
    }

    #[test]
    fn test_sector_out_of_range_track() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        let mut reader = AdfBackend::open(&path).unwrap();
        let err = reader.read_sector(80, 0, 0).unwrap_err();
        assert!(err.0.contains("Track"));
    }

    #[test]
    fn test_sector_out_of_range_side() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        let mut reader = AdfBackend::open(&path).unwrap();
        let err = reader.read_sector(0, 2, 0).unwrap_err();
        assert!(err.0.contains("Side"));
    }

    #[test]
    fn test_sector_out_of_range_sector() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        let mut reader = AdfBackend::open(&path).unwrap();
        let err = reader.read_sector(0, 0, 11).unwrap_err();
        assert!(err.0.contains("Sector"));
    }

    #[test]
    fn test_get_bootblock() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xDD);
        let mut reader = AdfBackend::open(&path).unwrap();
        let bootblock = reader.get_bootblock().unwrap();
        assert_eq!(bootblock.len(), 1024);
        assert_eq!(bootblock, vec![0xDDu8; 1024]);
    }

    #[test]
    fn test_read_sectors() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xEE);
        let mut reader = AdfBackend::open(&path).unwrap();
        let data = reader.read_sectors(0, 0, 0, 3).unwrap();
        assert_eq!(data.len(), 1536);
        assert_eq!(data, vec![0xEEu8; 1536]);
    }

    #[test]
    fn test_file_not_found() {
        let dir = tempdir();
        let path = dir.path().join("nonexistent.adf");
        let err = AdfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("File not found"));
    }

    #[test]
    fn test_tiny_valid_multiple_of_sector_size_rejected() {
        // 512 bytes is a valid multiple of SECTOR_SIZE but far too small to
        // hold a full 80-track/2-side image, so `spt` would truncate to 0.
        // This must be rejected in `open()` rather than panicking later in
        // `read_sector()` on the `sectors_per_track - 1` underflow.
        let dir = tempdir();
        let path = dir.path().join("tiny.adf");
        std::fs::write(&path, vec![0u8; 512]).unwrap();
        let err = AdfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("too small"));
    }

    #[test]
    fn test_read_sector_on_zero_spt_does_not_panic() {
        // Defense in depth: even if a backend were somehow constructed with
        // sectors_per_track == 0, read_sector must return an Err, not panic.
        let mut reader = AdfBackend {
            filepath: PathBuf::from("/nonexistent"),
            tracks: 80,
            sectors_per_track: 0,
        };
        let err = reader.read_sector(0, 0, 0).unwrap_err();
        assert!(err.0.contains("Sector"));
    }

    // Minimal tempdir helper to avoid pulling in a dev-dependency for this crate.
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
