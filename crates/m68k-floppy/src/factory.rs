//! Factory for opening floppy disk images with automatic backend selection.

use std::path::Path;

use crate::adf::AdfBackend;
use crate::floppy_base::{FloppyError, FloppyImageReader};
use crate::ipf::NativeIpfBackend;
use crate::uae::UaeExtendedBackend;

/// A floppy image reader, dynamically dispatched over the backend the
/// [`open_floppy_image`] factory selected.
#[derive(Debug)]
pub enum FloppyImage {
    Adf(AdfBackend),
    Uae(UaeExtendedBackend),
    NativeIpf(NativeIpfBackend),
}

impl FloppyImageReader for FloppyImage {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        match self {
            FloppyImage::Adf(b) => b.read_sector(track, side, sector),
            FloppyImage::Uae(b) => b.read_sector(track, side, sector),
            FloppyImage::NativeIpf(b) => b.read_sector(track, side, sector),
        }
    }
}

/// Which concrete backend a [`FloppyImage`] was opened with, for tests/diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Adf,
    Uae,
    NativeIpf,
}

impl FloppyImage {
    pub fn kind(&self) -> BackendKind {
        match self {
            FloppyImage::Adf(_) => BackendKind::Adf,
            FloppyImage::Uae(_) => BackendKind::Uae,
            FloppyImage::NativeIpf(_) => BackendKind::NativeIpf,
        }
    }
}

/// Backend selector for [`open_floppy_image`]. `Auto` picks based on file
/// extension and content, matching the Python factory's `backend="auto"` default.
///
/// The proprietary `capsimg`-backed reader (Python's `"caps"` backend) is not
/// ported — see `ipf` module docs — so there is no `Caps` variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Adf,
    Native,
    Uae,
}

/// Open a floppy disk image and return an appropriate reader.
///
/// # Errors
/// Returns [`FloppyError`] if the file or backend is invalid.
pub fn open_floppy_image(
    filepath: impl AsRef<Path>,
    backend: Backend,
) -> Result<FloppyImage, FloppyError> {
    let path = filepath.as_ref();

    if backend != Backend::Auto {
        return build_backend(backend, path);
    }

    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("adf") => open_adf(path),
        Some("ipf") => open_ipf(path),
        Some(ext) => Err(FloppyError::new(format!(
            "Unsupported file extension: .{}",
            ext
        ))),
        None => Err(FloppyError::new("Unsupported file extension: ")),
    }
}

/// Try standard ADF first, then UAE extended ADF.
fn open_adf(path: &Path) -> Result<FloppyImage, FloppyError> {
    match AdfBackend::open(path) {
        Ok(b) => Ok(FloppyImage::Adf(b)),
        Err(_) => UaeExtendedBackend::open(path).map(FloppyImage::Uae),
    }
}

/// The native IPF parser is used directly (no capsimg backend is ported —
/// Python's own factory already falls back to it whenever capsimg isn't
/// available, which is the common case in most environments).
fn open_ipf(path: &Path) -> Result<FloppyImage, FloppyError> {
    NativeIpfBackend::open(path).map(FloppyImage::NativeIpf)
}

fn build_backend(name: Backend, path: &Path) -> Result<FloppyImage, FloppyError> {
    match name {
        Backend::Adf => AdfBackend::open(path).map(FloppyImage::Adf),
        Backend::Native => NativeIpfBackend::open(path).map(FloppyImage::NativeIpf),
        Backend::Uae => UaeExtendedBackend::open(path).map(FloppyImage::Uae),
        Backend::Auto => unreachable!("Auto is handled by open_floppy_image directly"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADF_DD_SIZE: usize = 80 * 2 * 11 * 512;

    fn make_adf_file(dir: &Path, fill_byte: u8) -> std::path::PathBuf {
        let path = dir.join("test.adf");
        std::fs::write(&path, vec![fill_byte; ADF_DD_SIZE]).unwrap();
        path
    }

    fn make_uae1adf_file(dir: &Path, num_tracks: u32, track_size: u32) -> std::path::PathBuf {
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
    fn test_auto_adf() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        let reader = open_floppy_image(&path, Backend::Auto).unwrap();
        assert_eq!(reader.kind(), BackendKind::Adf);
    }

    #[test]
    fn test_explicit_adf() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        let reader = open_floppy_image(&path, Backend::Adf).unwrap();
        assert_eq!(reader.kind(), BackendKind::Adf);
    }

    #[test]
    fn test_explicit_native_fails_on_adf() {
        let dir = tempdir();
        let path = make_adf_file(dir.path(), 0xAA);
        // native backend won't work on ADF, but factory should at least try.
        assert!(open_floppy_image(&path, Backend::Native).is_err());
    }

    #[test]
    fn test_unsupported_extension() {
        let dir = tempdir();
        let path = dir.path().join("test.xyz");
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        let err = open_floppy_image(&path, Backend::Auto).unwrap_err();
        assert!(err.0.contains("Unsupported file extension"));
    }

    #[test]
    fn test_file_not_found() {
        let dir = tempdir();
        let path = dir.path().join("nonexistent.adf");
        let err = open_floppy_image(&path, Backend::Auto).unwrap_err();
        assert!(err.0.contains("File not found"));
    }

    #[test]
    fn test_ipf_fallback_to_native() {
        let dir = tempdir();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"CAPS");
        buf.extend_from_slice(&4u32.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 1]);
        buf.extend_from_slice(b"INFO");
        let info_data: Vec<u8> = [80u16, 2, 0, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        buf.extend_from_slice(&(info_data.len() as u32).to_be_bytes());
        buf.extend_from_slice(&info_data);
        let path = dir.path().join("test.ipf");
        std::fs::write(&path, buf).unwrap();

        // capsimg is never used (not ported), so this always resolves natively.
        let reader = open_floppy_image(&path, Backend::Auto).unwrap();
        assert_eq!(reader.kind(), BackendKind::NativeIpf);
    }

    #[test]
    fn test_explicit_uae() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let reader = open_floppy_image(&path, Backend::Uae).unwrap();
        assert_eq!(reader.kind(), BackendKind::Uae);
    }

    #[test]
    fn test_factory_auto_detects_uae() {
        let dir = tempdir();
        let path = make_uae1adf_file(dir.path(), 160, 12812);
        let reader = open_floppy_image(&path, Backend::Auto).unwrap();
        assert_eq!(reader.kind(), BackendKind::Uae);
    }

    fn tempdir() -> TempDir {
        TempDir::new()
    }

    struct TempDir(std::path::PathBuf);
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
