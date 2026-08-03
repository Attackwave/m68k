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
/// Slots in a file header's / extension block's data-block pointer table.
/// Numerically the same as [`HASH_TABLE_SIZE`] (both are
/// `BLOCK_SIZE/4 - 56`) but a distinct concept: this table is filled from
/// the end backwards, so index `BLOCK_TABLE_ENTRIES - 1` is a file's
/// *first* data block.
const BLOCK_TABLE_ENTRIES: usize = 72;

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

/// AmigaDOS filename hash, used to pick a directory's hash-table bucket:
///
/// ```text
/// hash = len(name)
/// for each byte c: hash = (hash * 13 + toupper(c)) & 0x7FF
/// bucket = hash % ht_size
/// ```
///
/// The intermediate mask is **11 bits** (`0x7FF`). An earlier version used
/// `0x7FFFFFFF`, which agrees for very short names but diverges as soon as
/// the running value exceeds 2047 — it placed most real-world names in the
/// wrong bucket (roughly a 30% hit rate against Workbench disks, i.e. no
/// better than chance).
///
/// Case folding is plain ASCII only. That is correct for standard OFS/FFS
/// volumes (`DOS\0`..`DOS\3`); International-Mode volumes (`DOS\2`/`DOS\3`)
/// additionally fold accented Latin-1 characters, which this does not
/// implement. Verified against all eight Workbench 1.3/3.1 disks walked
/// recursively: 1116/1116 entries, accented names included, land in the
/// bucket this function computes.
fn amiga_hash(name: &[u8]) -> u32 {
    let mut hash: u32 = name.len() as u32;
    for &b in name {
        let c = b.to_ascii_uppercase();
        hash = (hash.wrapping_mul(13).wrapping_add(c as u32)) & 0x7FF;
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
        // The hash chain is a pointer read out of the image, so a corrupt
        // one can point back at itself and loop forever. A chain cannot
        // legitimately be longer than the disk has blocks — every entry
        // in it occupies a distinct block.
        let max_chain = self.total_blocks() as usize;
        for i in 0..HASH_TABLE_SIZE {
            let mut current = read_u32(&block, 0x018 + i * 4);
            let mut hops = 0usize;
            while current != 0 {
                let header = self.read_block(current)?;
                if let Some(entry) = Self::dir_entry_from_header(&header, current)? {
                    entries.push(entry);
                }
                hops += 1;
                if hops > max_chain {
                    return Err(FloppyError::new(format!(
                        "hash chain in bucket {} is cyclic or longer than the disk",
                        i
                    )));
                }
                current = read_u32(&header, 0x1F0); // hash_chain
            }
        }
        Ok(entries)
    }

    /// Root directory's own block number, for [`AmigaFs::list_dir`].
    pub fn root_block_num(&self) -> u32 {
        self.root_block
    }

    /// Total number of blocks on the mounted disk.
    ///
    /// Derived from the root block, which `mount` places at
    /// `total_blocks / 2` — the standard AmigaDOS convention. Used to
    /// bound sizes and chain lengths read out of the image, none of
    /// which can legitimately exceed the disk itself.
    fn total_blocks(&self) -> u32 {
        self.root_block.saturating_mul(2)
    }

    /// Build a [`DirEntry`] from a file/directory header block.
    ///
    /// Returns `Ok(None)` for a block whose `sec_type` is neither
    /// `ST_FILE` nor `ST_USERDIR` — hard links and soft links appear in
    /// directory chains on real disks, and skipping them is preferable to
    /// aborting the whole listing.
    fn dir_entry_from_header(header: &[u8], block: u32) -> Result<Option<DirEntry>, FloppyError> {
        let sec_type = read_i32(header, 0x1FC);
        let entry = match sec_type {
            ST_FILE => DirEntry {
                name: read_bcpl_string(header, 0x1B0, 30),
                is_dir: false,
                size: read_u32(header, 0x144),
                block,
            },
            ST_USERDIR => DirEntry {
                name: read_bcpl_string(header, 0x1B0, 30),
                is_dir: true,
                size: 0,
                block,
            },
            _ => return Ok(None),
        };
        Ok(Some(entry))
    }

    /// Look up a single entry by name within a directory.
    ///
    /// Follows the hash chain for `amiga_hash(name)` first, which is what
    /// AmigaDOS itself does and costs only the blocks in that one chain.
    /// If the name isn't there, falls back to scanning every bucket.
    ///
    /// The fallback is not redundant: International-Mode volumes fold
    /// accented characters differently from the plain-ASCII rule in
    /// [`amiga_hash`], so such a name can legitimately live in a bucket we
    /// don't compute. Without the fallback those files are invisible to
    /// `--extract` while `--list` (which walks every bucket) still shows
    /// them — the exact mismatch that made extraction fail across all
    /// eight Workbench disks before the hash mask was corrected.
    pub fn find_entry(
        &mut self,
        dir_block: u32,
        name: &str,
    ) -> Result<Option<DirEntry>, FloppyError> {
        let block = self.read_block(dir_block)?;
        let idx = amiga_hash(name.as_bytes()) as usize;
        let mut current = read_u32(&block, 0x018 + idx * 4);
        // Bounded for the same reason as in `list_dir`: a corrupt image
        // can make this chain cyclic.
        let max_chain = self.total_blocks() as usize;
        let mut hops = 0usize;
        while current != 0 {
            let header = self.read_block(current)?;
            if let Some(entry) = Self::dir_entry_from_header(&header, current)?
                && entry.name.eq_ignore_ascii_case(name)
            {
                return Ok(Some(entry));
            }
            hops += 1;
            if hops > max_chain {
                break;
            }
            current = read_u32(&header, 0x1F0);
        }

        Ok(self
            .list_dir(dir_block)?
            .into_iter()
            .find(|e| e.name.eq_ignore_ascii_case(name)))
    }

    /// Resolve a slash-separated path such as `Libs/diskfont.library`
    /// against the root directory.
    ///
    /// Leading, trailing and repeated separators are ignored, so `/C/`,
    /// `C//List` and `C/List` all work. An empty path resolves to the root
    /// directory itself. Matching is case-insensitive, like AmigaDOS.
    ///
    /// Returns `Ok(None)` if any path component is missing, or if a
    /// component that needs to be traversed turns out to be a file rather
    /// than a directory.
    pub fn resolve_path(&mut self, path: &str) -> Result<Option<DirEntry>, FloppyError> {
        let mut current = DirEntry {
            name: String::new(),
            is_dir: true,
            size: 0,
            block: self.root_block,
        };

        for component in path.split('/').filter(|c| !c.is_empty()) {
            if !current.is_dir {
                return Ok(None);
            }
            match self.find_entry(current.block, component)? {
                Some(entry) => current = entry,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// Read a file by path, e.g. `read_file_at_path("C/List")`.
    ///
    /// Fails with a descriptive error if the path names a directory or
    /// does not exist.
    pub fn read_file_at_path(&mut self, path: &str) -> Result<Vec<u8>, FloppyError> {
        match self.resolve_path(path)? {
            Some(entry) if entry.is_dir => Err(FloppyError::new(format!(
                "'{}' is a directory, not a file",
                path
            ))),
            Some(entry) => self.read_file(entry.block),
            None => Err(FloppyError::new(format!("'{}' not found", path))),
        }
    }

    /// List a directory by path; an empty path lists the root directory.
    pub fn list_dir_at_path(&mut self, path: &str) -> Result<Vec<DirEntry>, FloppyError> {
        match self.resolve_path(path)? {
            Some(entry) if entry.is_dir => self.list_dir(entry.block),
            Some(_) => Err(FloppyError::new(format!(
                "'{}' is a file, not a directory",
                path
            ))),
            None => Err(FloppyError::new(format!("'{}' not found", path))),
        }
    }

    /// Recursively walk the filesystem from `path`, returning every file
    /// and directory found, each with its full slash-separated path.
    ///
    /// Directory entries are included alongside files. Cycles caused by a
    /// corrupt image are bounded by `max_depth`.
    pub fn walk(
        &mut self,
        path: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, DirEntry)>, FloppyError> {
        let start = match self.resolve_path(path)? {
            Some(entry) if entry.is_dir => entry,
            Some(_) => return Err(FloppyError::new(format!("'{}' is not a directory", path))),
            None => return Err(FloppyError::new(format!("'{}' not found", path))),
        };

        let base = path.trim_matches('/').to_string();
        let mut out = Vec::new();
        let mut stack = vec![(base, start.block, 0usize)];
        while let Some((prefix, block, depth)) = stack.pop() {
            for entry in self.list_dir(block)? {
                let full = if prefix.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{}/{}", prefix, entry.name)
                };
                if entry.is_dir && depth < max_depth {
                    stack.push((full.clone(), entry.block, depth + 1));
                }
                out.push((full, entry));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
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

        // The data block table is filled from the *end* of the block
        // backwards: the last slot (index BLOCK_TABLE_ENTRIES-1) holds the
        // file's first data block, and it grows downwards from there. So
        // for `high_seq` blocks the used slots are the final `high_seq`
        // entries, walked from the last index down to
        // `BLOCK_TABLE_ENTRIES - high_seq`.
        //
        // The previous code read indices `high_seq-1 ..= 0`, i.e. the
        // *start* of the table, which is all zeroes on a real disk — every
        // extraction silently produced an empty file. Verified against the
        // Workbench 3.1 fonts disk: `courier.font` has high_seq=3 with its
        // pointers at indices 69/70/71, not 0/1/2.
        //
        // When a file needs more blocks than one table holds, `extension`
        // points to a File Extension Block with the same shape plus its
        // own `extension` link.
        // A file cannot be larger than the disk it sits on. `byte_size`
        // comes straight out of the header, so on a corrupt image it can
        // claim anything — a fuzzer found a header declaring 0xFF000000,
        // which made the `Vec::with_capacity` below ask for 4 GB from a
        // 901 KB file.
        let disk_capacity = self.total_blocks() as usize * BLOCK_SIZE;
        if byte_size > disk_capacity {
            return Err(FloppyError::new(format!(
                "file header claims {} bytes, larger than the {}-byte disk",
                byte_size, disk_capacity
            )));
        }

        let mut data_blocks = Vec::new();
        let mut current_table_block = header.clone();
        // Bound the extension-block walk: the chain is a pointer read out
        // of the image, so a corrupt (or malicious) one can point back at
        // itself and loop forever. No real file needs more extension
        // blocks than the disk has blocks.
        let mut extensions_followed = 0usize;
        let max_extensions = self.total_blocks() as usize;
        loop {
            let high_seq = read_u32(&current_table_block, 0x008) as usize;
            let used = high_seq.min(BLOCK_TABLE_ENTRIES);
            for i in (BLOCK_TABLE_ENTRIES - used..BLOCK_TABLE_ENTRIES).rev() {
                let ptr = read_u32(&current_table_block, 0x018 + i * 4);
                if ptr != 0 {
                    data_blocks.push(ptr);
                }
            }
            let extension = read_u32(&current_table_block, 0x1F8);
            if extension == 0 {
                break;
            }
            extensions_followed += 1;
            if extensions_followed > max_extensions {
                return Err(FloppyError::new(
                    "file extension block chain is cyclic or longer than the disk",
                ));
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
        // Bucket indices read straight out of the root blocks of real
        // Workbench 1.3/3.1 disks. The previous version of this test only
        // asserted "deterministic and in range", which an 11-bit-vs-31-bit
        // mask error passes happily — these fixtures pin the actual value.
        //
        // Short names agree under either mask; the longer ones are the
        // discriminating cases (e.g. "Utilities" is bucket 69 with the
        // correct 0x7FF mask, 5 with the old one).
        for (name, bucket) in [
            (&b"C"[..], 8u32),
            (&b"L"[..], 17),
            (&b"S"[..], 24),
            (&b"Devs"[..], 22),
            (&b"Libs"[..], 46),
            (&b"Prefs"[..], 9),
            (&b"System"[..], 15),
            (&b"Classes"[..], 65),
            (&b"Utilities"[..], 69),
            (&b"Disk.info"[..], 54),
            (&b"Expansion.info"[..], 1),
            (&b"WBStartup.info"[..], 4),
        ] {
            assert_eq!(
                amiga_hash(name),
                bucket,
                "hash mismatch for {:?}",
                String::from_utf8_lossy(name)
            );
        }

        // Case-insensitivity: same name differing only in case must hash
        // identically (required for AmigaDOS case-insensitive lookup).
        assert_eq!(amiga_hash(b"TEST"), amiga_hash(b"test"));
        assert_eq!(amiga_hash(b"TeSt"), amiga_hash(b"test"));
        assert!(amiga_hash(b"test") < HASH_TABLE_SIZE as u32);
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
