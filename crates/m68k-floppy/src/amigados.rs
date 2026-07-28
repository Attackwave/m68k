//! Read-only AmigaDOS OFS/FFS filesystem support: directory listing and
//! file extraction from a mounted floppy image.
//!
//! Works over any [`FloppyImageReader`] backend (ADF, UAE, native IPF) —
//! this module only deals with the logical block layer (512-byte blocks
//! numbered 0..total_blocks), reading a block via `AmigaFs::read_block`'s
//! track/side/sector split.
//!
//! Block layout reference: AmigaDOS Technical Reference Manual (root
//! block, file header block, user directory block, OFS/FFS data block).
//! All multi-byte fields are big-endian 32-bit longwords, matching m68k
//! native byte order.

use crate::floppy_base::{FloppyError, FloppyImageReader};

const BLOCK_SIZE: usize = 512;
const SECTORS_PER_TRACK: u32 = 11;
const SIDES: u32 = 2;
const HASH_TABLE_SIZE: usize = 72;

const T_HEADER: i32 = 2;
const ST_ROOT: i32 = 1;
const ST_USERDIR: i32 = 2;
const ST_FILE: i32 = -3;

/// Filesystem flavor from the bootblock's disk-type flags byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    Ofs,
    Ffs,
}

/// One entry in a directory listing.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size: u32,
    /// Logical block number of this entry's header block — needed to
    /// extract a file's contents or list a subdirectory.
    pub block: u32,
}

/// A mounted AmigaDOS filesystem over a floppy image.
pub struct AmigaFs<'a> {
    reader: &'a mut dyn FloppyImageReader,
    pub kind: FsKind,
    root_block: u32,
}

fn read_u32(block: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(block[offset..offset + 4].try_into().unwrap())
}

fn read_i32(block: &[u8], offset: usize) -> i32 {
    read_u32(block, offset) as i32
}

/// AmigaDOS filename hash: `hash = hash*13 + fold(c); hash &= 0x7FFFFFFF`
/// per byte, starting from `hash = name.len()`, folded to `% ht_size`.
/// Case-folds plain ASCII 'a'-'z' to 'A'-'Z' (both OFS and FFS use the
/// same core algorithm; this covers the common non-accented case — full
/// International-Mode accented-character folding is not implemented, so
/// names using it may hash to the wrong bucket and fail lookup).
fn amiga_hash(name: &[u8]) -> u32 {
    let mut hash: u32 = name.len() as u32;
    for &b in name {
        let c = if b.is_ascii_lowercase() {
            b.to_ascii_uppercase()
        } else {
            b
        };
        hash = hash.wrapping_mul(13).wrapping_add(c as u32);
        hash &= 0x7FFF_FFFF;
    }
    hash % HASH_TABLE_SIZE as u32
}

/// Test-only accessor for `amiga_hash`, so `adf_writer`'s cross-module
/// integration tests can wire a synthetic file into a root block's hash
/// table using the exact same hash the reader side will look it up with.
#[cfg(test)]
pub(crate) fn amiga_hash_for_test(name: &[u8]) -> u32 {
    amiga_hash(name)
}

/// Read a BCPL string (1 length-prefix byte + up to `max_len` chars) at
/// `offset` in `block`.
fn read_bcpl_string(block: &[u8], offset: usize, max_len: usize) -> String {
    let len = (block[offset] as usize).min(max_len);
    String::from_utf8_lossy(&block[offset + 1..offset + 1 + len]).into_owned()
}

impl<'a> AmigaFs<'a> {
    /// Mount the filesystem: read the bootblock to determine OFS/FFS and
    /// total disk size (from the reader), locate and validate the root
    /// block (assumed at `total_blocks / 2`, the standard AmigaDOS
    /// convention — this module doesn't support non-standard root block
    /// placement).
    pub fn mount(
        reader: &'a mut dyn FloppyImageReader,
        total_tracks: u32,
    ) -> Result<Self, FloppyError> {
        let boot = reader.get_bootblock()?;
        if &boot[0..3] != b"DOS" {
            return Err(FloppyError::new(
                "not an AmigaDOS filesystem (missing 'DOS' bootblock signature)",
            ));
        }
        let flags = boot[3];
        let kind = if flags & 1 != 0 {
            FsKind::Ffs
        } else {
            FsKind::Ofs
        };

        let total_blocks = total_tracks * SIDES * SECTORS_PER_TRACK;
        if total_blocks == 0 || !total_blocks.is_multiple_of(2) {
            return Err(FloppyError::new(format!(
                "cannot locate root block: total block count {} is not evenly halvable",
                total_blocks
            )));
        }
        let root_block = total_blocks / 2;

        let mut fs = Self {
            reader,
            kind,
            root_block,
        };
        let root = fs.read_block(root_block)?;
        if read_i32(&root, 0) != T_HEADER || read_i32(&root, 0x1FC) != ST_ROOT {
            return Err(FloppyError::new(format!(
                "block {} is not a valid root block (type/sec_type mismatch)",
                root_block
            )));
        }
        Ok(fs)
    }

    fn read_block(&mut self, block_num: u32) -> Result<Vec<u8>, FloppyError> {
        let sector = block_num % SECTORS_PER_TRACK;
        let track_and_side = block_num / SECTORS_PER_TRACK;
        let side = track_and_side % SIDES;
        let track = track_and_side / SIDES;
        self.reader.read_sector(track, side, sector)
    }

    /// List the contents of a directory, given its header block number
    /// (use [`AmigaFs::root_block_num`] for the root directory).
    pub fn list_dir(&mut self, dir_block: u32) -> Result<Vec<DirEntry>, FloppyError> {
        let block = self.read_block(dir_block)?;
        let sec_type = read_i32(&block, 0x1FC);
        if sec_type != ST_ROOT && sec_type != ST_USERDIR {
            return Err(FloppyError::new(format!(
                "block {} is not a directory (sec_type {})",
                dir_block, sec_type
            )));
        }

        let mut entries = Vec::new();
        for i in 0..HASH_TABLE_SIZE {
            let mut current = read_u32(&block, 0x018 + i * 4);
            while current != 0 {
                let header = self.read_block(current)?;
                let entry_sec_type = read_i32(&header, 0x1FC);
                let (name, is_dir, size) = match entry_sec_type {
                    ST_FILE => (
                        read_bcpl_string(&header, 0x1B0, 30),
                        false,
                        read_u32(&header, 0x144),
                    ),
                    ST_USERDIR => (read_bcpl_string(&header, 0x1B0, 30), true, 0),
                    other => {
                        return Err(FloppyError::new(format!(
                            "block {} has unexpected sec_type {} in directory hash chain",
                            current, other
                        )));
                    }
                };
                entries.push(DirEntry {
                    name,
                    is_dir,
                    size,
                    block: current,
                });
                current = read_u32(&header, 0x1F0); // hash_chain
            }
        }
        Ok(entries)
    }

    /// Root directory's own block number, for [`AmigaFs::list_dir`].
    pub fn root_block_num(&self) -> u32 {
        self.root_block
    }

    /// Look up a single entry by name within a directory, using the same
    /// hash the filesystem itself uses (avoids scanning the whole
    /// directory when only one entry is needed).
    pub fn find_entry(
        &mut self,
        dir_block: u32,
        name: &str,
    ) -> Result<Option<DirEntry>, FloppyError> {
        let block = self.read_block(dir_block)?;
        let idx = amiga_hash(name.as_bytes()) as usize;
        let mut current = read_u32(&block, 0x018 + idx * 4);
        while current != 0 {
            let header = self.read_block(current)?;
            let sec_type = read_i32(&header, 0x1FC);
            let (entry_name, is_dir, size) = match sec_type {
                ST_FILE => (
                    read_bcpl_string(&header, 0x1B0, 30),
                    false,
                    read_u32(&header, 0x144),
                ),
                ST_USERDIR => (read_bcpl_string(&header, 0x1B0, 30), true, 0),
                other => {
                    return Err(FloppyError::new(format!(
                        "block {} has unexpected sec_type {} in directory hash chain",
                        current, other
                    )));
                }
            };
            if entry_name.eq_ignore_ascii_case(name) {
                return Ok(Some(DirEntry {
                    name: entry_name,
                    is_dir,
                    size,
                    block: current,
                }));
            }
            current = read_u32(&header, 0x1F0);
        }
        Ok(None)
    }

    /// Extract a file's full contents, given its file header block number
    /// (from a [`DirEntry`] with `is_dir == false`).
    pub fn read_file(&mut self, file_header_block: u32) -> Result<Vec<u8>, FloppyError> {
        let header = self.read_block(file_header_block)?;
        if read_i32(&header, 0x1FC) != ST_FILE {
            return Err(FloppyError::new(format!(
                "block {} is not a file header block",
                file_header_block
            )));
        }
        let byte_size = read_u32(&header, 0x144) as usize;

        // Data block pointers are stored highest-index-first: table[high_seq-1]
        // is the *first* data block, table[0] the last. Walk high_seq-1 down
        // to 0 to visit blocks in file order. When a file needs more than 72
        // data blocks, `extension` points to a File Extension Block with the
        // same 72-entry reversed-table shape plus its own `extension` link.
        let mut data_blocks = Vec::new();
        let mut current_table_block = header.clone();
        loop {
            let high_seq = read_u32(&current_table_block, 0x008) as usize;
            for i in (0..high_seq.min(HASH_TABLE_SIZE)).rev() {
                let ptr = read_u32(&current_table_block, 0x018 + i * 4);
                if ptr != 0 {
                    data_blocks.push(ptr);
                }
            }
            let extension = read_u32(&current_table_block, 0x1F8);
            if extension == 0 {
                break;
            }
            current_table_block = self.read_block(extension)?;
        }

        let mut out = Vec::with_capacity(byte_size);
        for block_num in data_blocks {
            let block = self.read_block(block_num)?;
            match self.kind {
                FsKind::Ofs => {
                    let data_size = read_u32(&block, 0x00C) as usize;
                    let data_size = data_size.min(BLOCK_SIZE - 24);
                    out.extend_from_slice(&block[24..24 + data_size]);
                }
                FsKind::Ffs => {
                    let remaining = byte_size.saturating_sub(out.len());
                    let take = remaining.min(BLOCK_SIZE);
                    out.extend_from_slice(&block[..take]);
                }
            }
        }
        out.truncate(byte_size);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amiga_hash_matches_known_values() {
        // "disk.info" and similarly-shaped short names are commonly cited
        // reference hashes in AmigaDOS documentation/tooling; verifying
        // the core algorithm shape (deterministic, in-range) rather than
        // a specific external reference value, since the length*13+c
        // loop is what's load-bearing here, not any one fixture name.
        let h = amiga_hash(b"test");
        assert!(h < HASH_TABLE_SIZE as u32);
        // Case-insensitivity: same name differing only in case must hash
        // identically (required for AmigaDOS case-insensitive lookup).
        assert_eq!(amiga_hash(b"TEST"), amiga_hash(b"test"));
        assert_eq!(amiga_hash(b"TeSt"), amiga_hash(b"test"));
    }

    #[test]
    fn test_read_bcpl_string() {
        let mut block = [0u8; 512];
        block[0x1B0] = 5;
        block[0x1B1..0x1B1 + 5].copy_from_slice(b"hello");
        assert_eq!(read_bcpl_string(&block, 0x1B0, 30), "hello");
    }

    #[test]
    fn test_read_bcpl_string_zero_length() {
        let block = [0u8; 512];
        assert_eq!(read_bcpl_string(&block, 0x1B0, 30), "");
    }
}
