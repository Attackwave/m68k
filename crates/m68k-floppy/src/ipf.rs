//! Native Rust IPF reader (clean-room implementation).
//!
//! Parses IPF chunk headers (CAPS, INFO, IMGE, TRCK) without relying on the
//! proprietary capsimg library. MFM bitstream decoding inside TRCK chunks is
//! handled by [`crate::mfm::MfmDecoder`].
//!
//! The proprietary-library-backed `capsimg` backend (Python's
//! `caps_backend.py`, which loads `capsimg.{dll,so}` via ctypes) is
//! intentionally not ported: it requires FFI bindings to a closed-source
//! library that may not even be present in most environments (Python's own
//! factory already falls back to this native parser when it isn't). See
//! `plan.md`/B5 notes.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::floppy_base::{FloppyError, FloppyImageReader, require_file};
use crate::mfm::MfmDecoder;

#[derive(Debug, Default)]
struct ImageInfo {
    cylinders: u16,
    heads: u16,
}

/// Parse IPF files natively. Supports header parsing and MFM decoding of
/// TRCK chunks.
#[derive(Debug)]
pub struct NativeIpfBackend {
    info: ImageInfo,
    track_chunks: HashMap<(u32, u32), Vec<u8>>,
    decoded_sectors: HashMap<(u32, u32, u32), Vec<u8>>,
}

fn read_u32_be(f: &mut impl Read) -> Result<u32, FloppyError> {
    let mut buf = [0u8; 4];
    f.read_exact(&mut buf)
        .map_err(|_| FloppyError::new("Unexpected end of file while reading uint32"))?;
    Ok(u32::from_be_bytes(buf))
}

fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

impl NativeIpfBackend {
    /// Open and validate a native (non-capsimg) IPF image.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if the file doesn't exist or is missing the
    /// mandatory `CAPS`/`INFO` chunks.
    pub fn open(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let filepath = require_file(filepath.as_ref())?;
        let (chunks_present, info, track_chunks) = Self::parse_ipf(&filepath)?;

        if !chunks_present.0 {
            return Err(FloppyError::new(
                "Not a valid IPF file: missing CAPS header",
            ));
        }
        if !chunks_present.1 {
            return Err(FloppyError::new("Not a valid IPF file: missing INFO chunk"));
        }

        Ok(Self {
            info,
            track_chunks,
            decoded_sectors: HashMap::new(),
        })
    }

    #[allow(clippy::type_complexity)]
    fn parse_ipf(
        filepath: &PathBuf,
    ) -> Result<((bool, bool), ImageInfo, HashMap<(u32, u32), Vec<u8>>), FloppyError> {
        let mut file = File::open(filepath).map_err(|e| FloppyError::new(e.to_string()))?;
        let mut has_caps = false;
        let mut has_info = false;
        let mut info = ImageInfo::default();
        let mut track_chunks: HashMap<(u32, u32), Vec<u8>> = HashMap::new();

        loop {
            let mut header = [0u8; 4];
            match file.read(&mut header) {
                Ok(4) => {}
                _ => break,
            };

            match &header {
                b"CAPS" => {
                    let size = read_u32_be(&mut file)?;
                    let mut data = vec![0u8; size as usize];
                    file.read_exact(&mut data)
                        .map_err(|e| FloppyError::new(e.to_string()))?;
                    has_caps = true;
                }
                b"INFO" => {
                    let size = read_u32_be(&mut file)?;
                    let mut data = vec![0u8; size as usize];
                    file.read_exact(&mut data)
                        .map_err(|e| FloppyError::new(e.to_string()))?;
                    if data.len() < 8 {
                        return Err(FloppyError::new("INFO chunk too small"));
                    }
                    info.cylinders = read_u16_be(&data, 0);
                    info.heads = read_u16_be(&data, 2);
                    has_info = true;
                }
                b"IMGE" => {
                    let size = read_u32_be(&mut file)?;
                    let mut data = vec![0u8; size as usize];
                    file.read_exact(&mut data)
                        .map_err(|e| FloppyError::new(e.to_string()))?;
                }
                b"TRCK" => {
                    let size = read_u32_be(&mut file)?;
                    let mut data = vec![0u8; size as usize];
                    file.read_exact(&mut data)
                        .map_err(|e| FloppyError::new(e.to_string()))?;
                    if data.len() >= 4 {
                        let track = read_u16_be(&data, 0) as u32;
                        let side = read_u16_be(&data, 2) as u32;
                        track_chunks
                            .entry((track, side))
                            .or_default()
                            .extend_from_slice(&data);
                    }
                }
                _ => {
                    let size = read_u32_be(&mut file)?;
                    let mut skip = vec![0u8; size as usize];
                    let _ = file.read_exact(&mut skip);
                }
            }
        }

        Ok(((has_caps, has_info), info, track_chunks))
    }

    /// Cylinder count from the INFO chunk, for diagnostics/tests.
    pub fn cylinders(&self) -> u16 {
        self.info.cylinders
    }

    /// Head count from the INFO chunk, for diagnostics/tests.
    pub fn heads(&self) -> u16 {
        self.info.heads
    }

    /// Decode all sectors from a track's TRCK chunk and cache them.
    fn decode_track_sectors(&mut self, track: u32, side: u32) {
        let Some(raw_data) = self.track_chunks.get(&(track, side)) else {
            return;
        };
        if raw_data.len() < 4 {
            return;
        }
        let Ok(decoder) = MfmDecoder::new(raw_data) else {
            return;
        };
        let Ok(decoded) = decoder.decode_track(Some(track), Some(side)) else {
            return;
        };
        for sector in decoded {
            self.decoded_sectors
                .insert((track, side, sector.sector), sector.data);
        }
    }
}

impl FloppyImageReader for NativeIpfBackend {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        let key = (track, side, sector);
        if let Some(data) = self.decoded_sectors.get(&key) {
            return Ok(data.clone());
        }

        if !self.track_chunks.contains_key(&(track, side)) {
            return Err(FloppyError::new(format!(
                "No TRCK chunk for track {}, side {}",
                track, side
            )));
        }

        self.decode_track_sectors(track, side);

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

    fn write_chunk(buf: &mut Vec<u8>, fourcc: &[u8; 4], data: &[u8]) {
        buf.extend_from_slice(fourcc);
        buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        buf.extend_from_slice(data);
    }

    fn make_minimal_ipf(dir: &Path) -> PathBuf {
        let mut buf = Vec::new();
        write_chunk(&mut buf, b"CAPS", &[0, 0, 0, 1]);
        let info_data: Vec<u8> = [80u16, 2, 0, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        write_chunk(&mut buf, b"INFO", &info_data);
        let path = dir.join("test.ipf");
        std::fs::write(&path, buf).unwrap();
        path
    }

    #[test]
    fn test_valid_ipf() {
        let dir = tempdir();
        let path = make_minimal_ipf(dir.path());
        let reader = NativeIpfBackend::open(&path).unwrap();
        assert_eq!(reader.cylinders(), 80);
        assert_eq!(reader.heads(), 2);
    }

    #[test]
    fn test_missing_caps() {
        let dir = tempdir();
        let mut buf = Vec::new();
        let info_data: Vec<u8> = [80u16, 2, 0, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        write_chunk(&mut buf, b"INFO", &info_data);
        let path = dir.path().join("bad.ipf");
        std::fs::write(&path, buf).unwrap();
        let err = NativeIpfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("missing CAPS"));
    }

    #[test]
    fn test_missing_info() {
        let dir = tempdir();
        let mut buf = Vec::new();
        write_chunk(&mut buf, b"CAPS", &[0, 0, 0, 1]);
        let path = dir.path().join("bad.ipf");
        std::fs::write(&path, buf).unwrap();
        let err = NativeIpfBackend::open(&path).unwrap_err();
        assert!(err.0.contains("missing INFO"));
    }

    #[test]
    fn test_read_sector_raises_floppy_error_for_invalid_data() {
        let dir = tempdir();
        let mut buf = Vec::new();
        write_chunk(&mut buf, b"CAPS", &[0, 0, 0, 1]);
        let info_data: Vec<u8> = [80u16, 2, 0, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        write_chunk(&mut buf, b"INFO", &info_data);
        let mut trck_data = vec![0u8, 0, 0, 0];
        trck_data.extend(vec![0u8; 100]);
        write_chunk(&mut buf, b"TRCK", &trck_data);
        let path = dir.path().join("test.ipf");
        std::fs::write(&path, buf).unwrap();

        let mut reader = NativeIpfBackend::open(&path).unwrap();
        assert!(reader.read_sector(0, 0, 0).is_err());
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
