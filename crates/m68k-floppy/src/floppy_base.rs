//! Abstract interface and error type for floppy disk image readers.

use std::path::{Path, PathBuf};

/// An error reading or interpreting a floppy disk image.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FloppyError(pub String);

impl FloppyError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Common interface for reading floppy disk images by (track, side, sector).
pub trait FloppyImageReader {
    /// Read a single 512-byte sector from the image.
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError>;

    /// Read the Amiga bootblock (track 0, side 0, sectors 0-1): 1024 bytes.
    fn get_bootblock(&mut self) -> Result<Vec<u8>, FloppyError> {
        let mut data = self.read_sector(0, 0, 0)?;
        data.extend(self.read_sector(0, 0, 1)?);
        Ok(data)
    }

    /// Read `count` consecutive sectors starting at `sector`.
    fn read_sectors(
        &mut self,
        track: u32,
        side: u32,
        sector: u32,
        count: u32,
    ) -> Result<Vec<u8>, FloppyError> {
        let mut result = Vec::new();
        for i in 0..count {
            result.extend(self.read_sector(track, side, sector + i)?);
        }
        Ok(result)
    }
}

/// Verify a path exists and is a regular file.
pub(crate) fn require_file(path: &Path) -> Result<PathBuf, FloppyError> {
    if !path.is_file() {
        return Err(FloppyError::new(format!(
            "File not found: {}",
            path.display()
        )));
    }
    Ok(path.to_path_buf())
}
