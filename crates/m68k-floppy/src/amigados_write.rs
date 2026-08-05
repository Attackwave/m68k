//! Write support for AmigaDOS OFS/FFS volumes: block allocation, file
//! creation and deletion, directories, renaming and metadata.
//!
//! This is the write-side counterpart to the read-only [`crate::amigados`]
//! module. Where that one works over a [`FloppyImageReader`] trait object,
//! this one operates directly on a mutable raw image buffer (`&mut [u8]`,
//! block N at offset N*512). That difference is deliberate: the reader
//! trait abstracts over ADF/UAE/IPF, but IPF is a flux-level format that
//! cannot meaningfully be written back, and UAE-extended images have
//! variable-length tracks. Plain sector-addressable images are the only
//! ones a filesystem writer makes sense for, and those are exactly what a
//! raw buffer represents.
//!
//! # Layout
//!
//! Every structural detail here was read off a real AmigaDOS disk rather
//! than derived from documentation, then cross-checked against this
//! crate's own reader:
//!
//! - The bitmap covers blocks from 2 upward; **a set bit means free**.
//! - A bitmap block's checksum makes the sum of all its longwords zero.
//! - A file header's data-block table is filled **from the end backwards**,
//!   so the last table slot holds the file's first data block.
//! - `high_seq` counts the entries used in that table.
//! - OFS data blocks carry a 24-byte header, leaving 488 payload bytes;
//!   FFS data blocks are raw 512-byte payload.

use crate::floppy_base::FloppyError;

pub(crate) const BLOCK_SIZE: usize = 512;

/// First block the bitmap accounts for. Blocks 0 and 1 are the bootblock
/// and are never allocatable.
pub(crate) const FIRST_ALLOCATABLE_BLOCK: u32 = 2;

/// Longwords of bitmap payload in one bitmap block (512 bytes minus the
/// 4-byte checksum at offset 0), i.e. 127 * 32 = 4064 blocks covered.
const BITMAP_LONGS_PER_BLOCK: u32 = 127;
const BITS_PER_BITMAP_BLOCK: u32 = BITMAP_LONGS_PER_BLOCK * 32;

/// Slots in a root/directory hash table and in a file header's data-block
/// table. Both are `BLOCK_SIZE/4 - 56`.
pub(crate) const HASH_TABLE_SIZE: usize = 72;
pub(crate) const BLOCK_TABLE_ENTRIES: usize = 72;

/// Payload bytes in an OFS data block: 512 minus the 24-byte block header.
pub(crate) const OFS_DATA_PER_BLOCK: usize = 488;

pub(crate) const T_HEADER: i32 = 2;
pub(crate) const T_DATA: i32 = 8;
pub(crate) const T_LIST: i32 = 16;
pub(crate) const ST_ROOT: i32 = 1;
pub(crate) const ST_USERDIR: i32 = 2;
pub(crate) const ST_FILE: i32 = -3;

// Field offsets shared by header-type blocks (root, file header, userdir).
pub(crate) const OFF_TYPE: usize = 0x000;
pub(crate) const OFF_HEADER_KEY: usize = 0x004;
pub(crate) const OFF_HIGH_SEQ: usize = 0x008;
pub(crate) const OFF_DATA_SIZE: usize = 0x00C;
pub(crate) const OFF_FIRST_DATA: usize = 0x010;
pub(crate) const OFF_CHECKSUM: usize = 0x014;
pub(crate) const OFF_TABLE: usize = 0x018;
pub(crate) const OFF_PROTECT: usize = 0x140;
pub(crate) const OFF_BYTE_SIZE: usize = 0x144;
pub(crate) const OFF_COMMENT: usize = 0x148;
pub(crate) const OFF_DAYS: usize = 0x1A4;
pub(crate) const OFF_MINS: usize = 0x1A8;
pub(crate) const OFF_TICKS: usize = 0x1AC;
pub(crate) const OFF_NAME: usize = 0x1B0;
pub(crate) const OFF_HASH_CHAIN: usize = 0x1F0;
pub(crate) const OFF_PARENT: usize = 0x1F4;
pub(crate) const OFF_EXTENSION: usize = 0x1F8;
pub(crate) const OFF_SEC_TYPE: usize = 0x1FC;

// Root-block-specific fields. `bm_flag` at 0x138 is not touched here: the
// bitmap is updated in the same operation as the blocks it describes, so it
// is never transiently invalid and the flag stays at -1 (valid) throughout —
// which is what a cleanly unmounted volume shows.
pub(crate) const OFF_ROOT_BM_PAGES: usize = 0x13C;

// OFS data block fields.
pub(crate) const OFF_DATA_SEQ: usize = 0x008;
pub(crate) const OFF_DATA_NEXT: usize = 0x010;

pub(crate) fn read_u32(image: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(image[offset..offset + 4].try_into().unwrap())
}

pub(crate) fn write_u32(image: &mut [u8], offset: usize, value: u32) {
    image[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn block_offset(block: u32) -> usize {
    block as usize * BLOCK_SIZE
}

/// AmigaDOS filename hash. Kept in sync with the reader's implementation —
/// see [`crate::amigados`] for the derivation and the 11-bit mask caveat.
pub(crate) fn amiga_hash(name: &[u8]) -> u32 {
    let mut hash: u32 = name.len() as u32;
    for &b in name {
        let c = b.to_ascii_uppercase();
        hash = (hash.wrapping_mul(13).wrapping_add(c as u32)) & 0x7FF;
    }
    hash % HASH_TABLE_SIZE as u32
}

/// Write a BCPL string (1 length byte + chars) at `offset`, truncating to
/// `max_len` bytes. The remaining field bytes are cleared, so rewriting a
/// shorter name over a longer one leaves no tail behind.
pub(crate) fn write_bcpl_string(block: &mut [u8], offset: usize, s: &str, max_len: usize) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(max_len);
    block[offset] = len as u8;
    block[offset + 1..offset + 1 + max_len].fill(0);
    block[offset + 1..offset + 1 + len].copy_from_slice(&bytes[..len]);
}

pub(crate) fn read_bcpl_string(block: &[u8], offset: usize, max_len: usize) -> String {
    let len = (block[offset] as usize).min(max_len);
    String::from_utf8_lossy(&block[offset + 1..offset + 1 + len]).into_owned()
}

/// Standard AmigaDOS block checksum: the value that makes the sum of all
/// longwords in the block come out zero, with the checksum field itself
/// counted as zero while summing.
pub(crate) fn block_checksum(block: &[u8], checksum_offset: usize) -> u32 {
    let mut sum = 0u32;
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        if i * 4 == checksum_offset {
            continue;
        }
        sum = sum.wrapping_add(u32::from_be_bytes(chunk.try_into().unwrap()));
    }
    0u32.wrapping_sub(sum)
}

/// Recompute and store a block's checksum in place.
pub(crate) fn fix_block_checksum(image: &mut [u8], block: u32, checksum_offset: usize) {
    let start = block_offset(block);
    let sum = block_checksum(&image[start..start + BLOCK_SIZE], checksum_offset);
    write_u32(image, start + checksum_offset, sum);
}

/// Clear the bitmap bits that lie past the end of the volume.
///
/// A bitmap block always covers 4064 blocks; a DD floppy has 1760, so the
/// tail bits describe blocks that do not exist. They are cleared (= "in
/// use") so nothing can hand them out. [`Bitmap::mark_used`] deliberately
/// refuses out-of-volume block numbers, so this writes the words directly.
pub(crate) fn clear_bits_past_end(image: &mut [u8], first_bitmap_block: u32, total_blocks: u32) {
    let pages = Bitmap::pages_needed(total_blocks);
    for page_idx in 0..pages {
        let page_base = block_offset(first_bitmap_block + page_idx) + 4;
        for long_idx in 0..BITMAP_LONGS_PER_BLOCK {
            let first_block_in_long =
                FIRST_ALLOCATABLE_BLOCK + page_idx * BITS_PER_BITMAP_BLOCK + long_idx * 32;
            if first_block_in_long >= total_blocks {
                write_u32(image, page_base + long_idx as usize * 4, 0);
                continue;
            }
            // A longword straddling the end: keep the low bits, clear the
            // rest.
            let valid = total_blocks - first_block_in_long;
            if valid < 32 {
                let off = page_base + long_idx as usize * 4;
                let mask = (1u32 << valid) - 1;
                let word = read_u32(image, off) & mask;
                write_u32(image, off, word);
            }
        }
    }
}

/// Block allocator over a volume's bitmap blocks.
///
/// The bitmap is stored in one or more dedicated blocks pointed at by the
/// root block's `bm_pages` table. Bit `n` of the payload corresponds to
/// block `n + 2`, and a **set** bit means the block is free — the inverse
/// of what one might assume, and the kind of detail worth stating twice.
#[derive(Debug, Clone)]
pub struct Bitmap {
    /// Block numbers of the bitmap blocks themselves, in order.
    pages: Vec<u32>,
    total_blocks: u32,
}

impl Bitmap {
    /// Number of bitmap blocks needed to cover `total_blocks`.
    pub fn pages_needed(total_blocks: u32) -> u32 {
        let covered = total_blocks.saturating_sub(FIRST_ALLOCATABLE_BLOCK);
        covered.div_ceil(BITS_PER_BITMAP_BLOCK).max(1)
    }

    /// Read the bitmap page list out of a volume's root block.
    pub fn from_root(
        image: &[u8],
        root_block: u32,
        total_blocks: u32,
    ) -> Result<Self, FloppyError> {
        let root = block_offset(root_block);
        if root + BLOCK_SIZE > image.len() {
            return Err(FloppyError::new("root block is outside the image"));
        }
        let mut pages = Vec::new();
        // 25 in-root bm_pages slots; volumes larger than those cover need a
        // bm_ext chain, which a floppy-sized image never does (one page
        // covers 4064 blocks, a HD floppy has 3520).
        for i in 0..25 {
            let page = read_u32(image, root + OFF_ROOT_BM_PAGES + i * 4);
            if page == 0 {
                break;
            }
            if page >= total_blocks {
                return Err(FloppyError::new(format!(
                    "bitmap page pointer {} is outside the volume ({} blocks)",
                    page, total_blocks
                )));
            }
            pages.push(page);
        }
        if pages.is_empty() {
            return Err(FloppyError::new(
                "volume has no bitmap blocks — it was created without a block allocator \
                 and cannot have files written to it",
            ));
        }
        Ok(Self {
            pages,
            total_blocks,
        })
    }

    /// Locate the (bitmap block, byte offset within it, bit index) for a
    /// data block number.
    fn locate(&self, block: u32) -> Option<(u32, usize, u32)> {
        if block < FIRST_ALLOCATABLE_BLOCK || block >= self.total_blocks {
            return None;
        }
        let index = block - FIRST_ALLOCATABLE_BLOCK;
        let page_idx = (index / BITS_PER_BITMAP_BLOCK) as usize;
        let page = *self.pages.get(page_idx)?;
        let within = index % BITS_PER_BITMAP_BLOCK;
        // +4 skips the bitmap block's own checksum longword.
        let byte = block_offset(page) + 4 + (within / 32) as usize * 4;
        Some((page, byte, within % 32))
    }

    pub fn is_free(&self, image: &[u8], block: u32) -> bool {
        match self.locate(block) {
            Some((_, byte, bit)) => (read_u32(image, byte) >> bit) & 1 == 1,
            None => false,
        }
    }

    fn set_state(&self, image: &mut [u8], block: u32, free: bool) -> Result<(), FloppyError> {
        let (page, byte, bit) = self.locate(block).ok_or_else(|| {
            FloppyError::new(format!("block {} is outside the allocatable range", block))
        })?;
        let mut word = read_u32(image, byte);
        if free {
            word |= 1 << bit;
        } else {
            word &= !(1 << bit);
        }
        write_u32(image, byte, word);
        fix_block_checksum(image, page, 0);
        Ok(())
    }

    /// Mark a block as in use.
    pub fn mark_used(&self, image: &mut [u8], block: u32) -> Result<(), FloppyError> {
        self.set_state(image, block, false)
    }

    /// Mark a block as available again.
    pub fn mark_free(&self, image: &mut [u8], block: u32) -> Result<(), FloppyError> {
        self.set_state(image, block, true)
    }

    /// Allocate one free block, marking it used and zeroing its contents.
    ///
    /// Search starts just past the root block and wraps, which is what
    /// AmigaDOS itself does: files written to a fresh disk land next to the
    /// root rather than at block 2, keeping seeks short on real hardware.
    pub fn allocate(&self, image: &mut [u8], root_block: u32) -> Result<u32, FloppyError> {
        let start = root_block + 1;
        for offset in 0..self.total_blocks {
            let candidate = FIRST_ALLOCATABLE_BLOCK
                + (start + offset - FIRST_ALLOCATABLE_BLOCK)
                    % (self.total_blocks - FIRST_ALLOCATABLE_BLOCK);
            if self.is_free(image, candidate) {
                self.mark_used(image, candidate)?;
                let off = block_offset(candidate);
                image[off..off + BLOCK_SIZE].fill(0);
                return Ok(candidate);
            }
        }
        Err(FloppyError::new("no free blocks left on the volume"))
    }

    /// Allocate `count` blocks, rolling back if the volume runs out partway
    /// through — a half-written file with its blocks marked used would leak
    /// space that nothing references.
    pub fn allocate_many(
        &self,
        image: &mut [u8],
        root_block: u32,
        count: usize,
    ) -> Result<Vec<u32>, FloppyError> {
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            match self.allocate(image, root_block) {
                Ok(b) => blocks.push(b),
                Err(e) => {
                    for b in blocks {
                        let _ = self.mark_free(image, b);
                    }
                    return Err(e);
                }
            }
        }
        Ok(blocks)
    }

    /// Total blocks this bitmap accounts for.
    pub fn total_blocks(&self) -> u32 {
        self.total_blocks
    }

    /// Count of currently free blocks, for `df`-style reporting and tests.
    pub fn free_count(&self, image: &[u8]) -> u32 {
        (FIRST_ALLOCATABLE_BLOCK..self.total_blocks)
            .filter(|&b| self.is_free(image, b))
            .count() as u32
    }
}

/// A mounted AmigaDOS volume opened for writing, over a raw image buffer.
pub struct AmigaFsWriter<'a> {
    image: &'a mut Vec<u8>,
    root_block: u32,
    total_blocks: u32,
    bitmap: Bitmap,
    ffs: bool,
}

impl<'a> AmigaFsWriter<'a> {
    /// Open a raw sector-addressable image (an ADF buffer) for writing.
    ///
    /// Validates the bootblock signature, locates the root block at the
    /// standard `total_blocks / 2`, and loads the bitmap. A volume whose
    /// root block has no bitmap pages is rejected: it cannot be allocated
    /// from, and writing to it anyway would corrupt it.
    pub fn mount(image: &'a mut Vec<u8>) -> Result<Self, FloppyError> {
        if image.len() < 3 * BLOCK_SIZE || !image.len().is_multiple_of(BLOCK_SIZE) {
            return Err(FloppyError::new(format!(
                "image size {} is not a usable multiple of the block size",
                image.len()
            )));
        }
        if &image[0..3] != b"DOS" {
            return Err(FloppyError::new(
                "not an AmigaDOS volume (no DOS signature)",
            ));
        }
        let ffs = image[3] & 1 == 1;
        let total_blocks = (image.len() / BLOCK_SIZE) as u32;
        let root_block = total_blocks / 2;

        let root = block_offset(root_block);
        if read_u32(image, root + OFF_TYPE) as i32 != T_HEADER
            || read_u32(image, root + OFF_SEC_TYPE) as i32 != ST_ROOT
        {
            return Err(FloppyError::new(format!(
                "block {} is not a valid root block",
                root_block
            )));
        }
        let bitmap = Bitmap::from_root(image, root_block, total_blocks)?;
        Ok(Self {
            image,
            root_block,
            total_blocks,
            bitmap,
            ffs,
        })
    }

    pub fn root_block(&self) -> u32 {
        self.root_block
    }

    pub fn is_ffs(&self) -> bool {
        self.ffs
    }

    /// Free blocks remaining on the volume.
    pub fn free_blocks(&self) -> u32 {
        self.bitmap.free_count(self.image)
    }

    /// Consume the writer, returning the modified image.
    pub fn into_image(self) -> &'a mut Vec<u8> {
        self.image
    }

    fn check_block(&self, block: u32) -> Result<usize, FloppyError> {
        if block >= self.total_blocks {
            return Err(FloppyError::new(format!(
                "block {} is outside the volume ({} blocks)",
                block, self.total_blocks
            )));
        }
        Ok(block_offset(block))
    }

    fn sec_type_of(&self, block: u32) -> Result<i32, FloppyError> {
        let off = self.check_block(block)?;
        Ok(read_u32(self.image, off + OFF_SEC_TYPE) as i32)
    }

    /// Directory hash table slot count. The root block stores it; user
    /// directories inherit the same size.
    fn hash_table_size(&self) -> usize {
        HASH_TABLE_SIZE
    }

    /// Walk a directory's hash chain for `name`, returning the entry block
    /// and the block whose `hash_chain` field points at it (or `None` for
    /// the first entry in a bucket, which the hash table points at).
    fn find_in_dir(
        &self,
        dir_block: u32,
        name: &str,
    ) -> Result<Option<(u32, Option<u32>)>, FloppyError> {
        let dir_off = self.check_block(dir_block)?;
        let bucket = amiga_hash(name.as_bytes()) as usize;
        let mut current = read_u32(self.image, dir_off + OFF_TABLE + bucket * 4);
        let mut previous: Option<u32> = None;
        // Bounded by the volume size: a corrupted chain that loops back on
        // itself must not spin forever.
        let mut steps = 0u32;
        while current != 0 {
            steps += 1;
            if steps > self.total_blocks {
                return Err(FloppyError::new(format!(
                    "hash chain in directory block {} does not terminate",
                    dir_block
                )));
            }
            let off = self.check_block(current)?;
            let entry_name = read_bcpl_string(self.image, off + OFF_NAME, 30);
            if entry_name.eq_ignore_ascii_case(name) {
                return Ok(Some((current, previous)));
            }
            previous = Some(current);
            current = read_u32(self.image, off + OFF_HASH_CHAIN);
        }
        Ok(None)
    }

    /// Insert an already-written header block into a directory's hash
    /// chain. New entries go to the front of their bucket, which is what
    /// AmigaDOS does and keeps this O(1).
    fn link_into_dir(&mut self, dir_block: u32, entry_block: u32) -> Result<(), FloppyError> {
        let entry_off = self.check_block(entry_block)?;
        let name = read_bcpl_string(self.image, entry_off + OFF_NAME, 30);
        let bucket = amiga_hash(name.as_bytes()) as usize;
        let dir_off = self.check_block(dir_block)?;

        let head = read_u32(self.image, dir_off + OFF_TABLE + bucket * 4);
        write_u32(self.image, entry_off + OFF_HASH_CHAIN, head);
        write_u32(self.image, entry_off + OFF_PARENT, dir_block);
        write_u32(self.image, dir_off + OFF_TABLE + bucket * 4, entry_block);

        fix_block_checksum(self.image, entry_block, OFF_CHECKSUM);
        fix_block_checksum(self.image, dir_block, OFF_CHECKSUM);
        self.touch_dir(dir_block)?;
        Ok(())
    }

    /// Remove an entry from its parent directory's hash chain, leaving the
    /// entry block itself untouched.
    fn unlink_from_dir(&mut self, dir_block: u32, entry_block: u32) -> Result<(), FloppyError> {
        let entry_off = self.check_block(entry_block)?;
        let name = read_bcpl_string(self.image, entry_off + OFF_NAME, 30);
        let next = read_u32(self.image, entry_off + OFF_HASH_CHAIN);

        let found = self.find_in_dir(dir_block, &name)?;
        let Some((found_block, previous)) = found else {
            return Err(FloppyError::new(format!(
                "entry {:?} is not in directory block {}",
                name, dir_block
            )));
        };
        if found_block != entry_block {
            return Err(FloppyError::new(format!(
                "directory block {} holds a different entry named {:?}",
                dir_block, name
            )));
        }

        match previous {
            Some(prev) => {
                let prev_off = self.check_block(prev)?;
                write_u32(self.image, prev_off + OFF_HASH_CHAIN, next);
                fix_block_checksum(self.image, prev, OFF_CHECKSUM);
            }
            None => {
                let bucket = amiga_hash(name.as_bytes()) as usize;
                let dir_off = self.check_block(dir_block)?;
                write_u32(self.image, dir_off + OFF_TABLE + bucket * 4, next);
                fix_block_checksum(self.image, dir_block, OFF_CHECKSUM);
            }
        }
        self.touch_dir(dir_block)?;
        Ok(())
    }

    /// Update a directory's modification timestamp and re-checksum it.
    fn touch_dir(&mut self, dir_block: u32) -> Result<(), FloppyError> {
        let off = self.check_block(dir_block)?;
        let (days, mins, ticks) = amiga_now();
        write_u32(self.image, off + OFF_DAYS, days);
        write_u32(self.image, off + OFF_MINS, mins);
        write_u32(self.image, off + OFF_TICKS, ticks);
        fix_block_checksum(self.image, dir_block, OFF_CHECKSUM);
        Ok(())
    }

    /// Resolve a slash-separated path to its header block. An empty path
    /// (or "/") is the root directory.
    pub fn resolve(&self, path: &str) -> Result<Option<u32>, FloppyError> {
        let mut current = self.root_block;
        for component in path.split('/').filter(|c| !c.is_empty()) {
            match self.find_in_dir(current, component)? {
                Some((block, _)) => current = block,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// Split "a/b/c.txt" into the parent directory's block and "c.txt".
    fn split_parent<'p>(&self, path: &'p str) -> Result<(u32, &'p str), FloppyError> {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Err(FloppyError::new("empty path"));
        }
        let (dir_path, name) = match trimmed.rsplit_once('/') {
            Some((d, n)) => (d, n),
            None => ("", trimmed),
        };
        if name.len() > 30 {
            return Err(FloppyError::new(format!(
                "name {:?} exceeds the 30-character AmigaDOS limit",
                name
            )));
        }
        let dir = self
            .resolve(dir_path)?
            .ok_or_else(|| FloppyError::new(format!("directory {:?} does not exist", dir_path)))?;
        let sec_type = self.sec_type_of(dir)?;
        if sec_type != ST_ROOT && sec_type != ST_USERDIR {
            return Err(FloppyError::new(format!(
                "{:?} is not a directory",
                dir_path
            )));
        }
        Ok((dir, name))
    }

    /// Write a file at `path`, replacing any existing file of that name.
    ///
    /// Lays out a file header block, the data blocks (OFS blocks carry a
    /// 24-byte header and 488 payload bytes; FFS blocks are raw 512-byte
    /// payload), and extension blocks once the header's 72-slot table is
    /// full.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<u32, FloppyError> {
        let (dir_block, name) = self.split_parent(path)?;
        // Replacing means the old blocks come back to the pool first, so a
        // rewrite of similar size cannot spuriously fail on a full disk.
        if let Some((existing, _)) = self.find_in_dir(dir_block, name)? {
            if self.sec_type_of(existing)? != ST_FILE {
                return Err(FloppyError::new(format!(
                    "{:?} exists and is not a file",
                    path
                )));
            }
            self.delete_file_at(dir_block, existing)?;
        }

        let per_block = if self.ffs {
            BLOCK_SIZE
        } else {
            OFS_DATA_PER_BLOCK
        };
        let data_block_count = data.len().div_ceil(per_block.max(1));
        let ext_count = data_block_count.saturating_sub(1) / BLOCK_TABLE_ENTRIES;

        // Header + data + extension blocks, allocated as one unit so a
        // volume-full condition rolls the whole file back.
        let total_needed = 1 + data_block_count + ext_count;
        let mut blocks = self
            .bitmap
            .allocate_many(self.image, self.root_block, total_needed)?;
        let header = blocks.remove(0);
        let data_blocks: Vec<u32> = blocks.drain(..data_block_count).collect();
        let ext_blocks = blocks;

        if let Err(e) = self.lay_out_file(header, &data_blocks, &ext_blocks, name, data) {
            for b in std::iter::once(header)
                .chain(data_blocks.iter().copied())
                .chain(ext_blocks.iter().copied())
            {
                let _ = self.bitmap.mark_free(self.image, b);
            }
            return Err(e);
        }

        self.link_into_dir(dir_block, header)?;
        Ok(header)
    }

    fn lay_out_file(
        &mut self,
        header: u32,
        data_blocks: &[u32],
        ext_blocks: &[u32],
        name: &str,
        data: &[u8],
    ) -> Result<(), FloppyError> {
        let per_block = if self.ffs {
            BLOCK_SIZE
        } else {
            OFS_DATA_PER_BLOCK
        };
        let (days, mins, ticks) = amiga_now();

        // Data blocks first: OFS blocks reference the header and the next
        // block in sequence, so they need the whole list up front.
        for (i, &block) in data_blocks.iter().enumerate() {
            let off = self.check_block(block)?;
            let start = i * per_block;
            let end = (start + per_block).min(data.len());
            let chunk = &data[start..end];
            if self.ffs {
                self.image[off..off + chunk.len()].copy_from_slice(chunk);
            } else {
                write_u32(self.image, off + OFF_TYPE, T_DATA as u32);
                write_u32(self.image, off + OFF_HEADER_KEY, header);
                write_u32(self.image, off + OFF_DATA_SEQ, i as u32 + 1);
                write_u32(self.image, off + OFF_DATA_SIZE, chunk.len() as u32);
                let next = data_blocks.get(i + 1).copied().unwrap_or(0);
                write_u32(self.image, off + OFF_DATA_NEXT, next);
                self.image[off + 24..off + 24 + chunk.len()].copy_from_slice(chunk);
                fix_block_checksum(self.image, block, OFF_CHECKSUM);
            }
        }

        // Header block, then extension blocks for anything past the first
        // 72 data blocks. The block table is filled from the end backwards,
        // so slot 71 holds the first data block — verified against a real
        // disk, where a file's first_data equalled its last table slot.
        let mut chunks = data_blocks.chunks(BLOCK_TABLE_ENTRIES);
        let first_chunk = chunks.next().unwrap_or(&[]);

        let hoff = self.check_block(header)?;
        write_u32(self.image, hoff + OFF_TYPE, T_HEADER as u32);
        write_u32(self.image, hoff + OFF_HEADER_KEY, header);
        write_u32(self.image, hoff + OFF_HIGH_SEQ, first_chunk.len() as u32);
        write_u32(self.image, hoff + OFF_DATA_SIZE, 0);
        write_u32(
            self.image,
            hoff + OFF_FIRST_DATA,
            first_chunk.first().copied().unwrap_or(0),
        );
        for (i, &block) in first_chunk.iter().enumerate() {
            let slot = BLOCK_TABLE_ENTRIES - 1 - i;
            write_u32(self.image, hoff + OFF_TABLE + slot * 4, block);
        }
        write_u32(self.image, hoff + OFF_BYTE_SIZE, data.len() as u32);
        write_u32(self.image, hoff + OFF_DAYS, days);
        write_u32(self.image, hoff + OFF_MINS, mins);
        write_u32(self.image, hoff + OFF_TICKS, ticks);
        write_bcpl_string(self.image, hoff + OFF_NAME, name, 30);
        write_u32(self.image, hoff + OFF_SEC_TYPE, ST_FILE as u32);
        write_u32(
            self.image,
            hoff + OFF_EXTENSION,
            ext_blocks.first().copied().unwrap_or(0),
        );
        fix_block_checksum(self.image, header, OFF_CHECKSUM);

        for (i, chunk) in chunks.enumerate() {
            let ext = ext_blocks[i];
            let eoff = self.check_block(ext)?;
            write_u32(self.image, eoff + OFF_TYPE, T_LIST as u32);
            write_u32(self.image, eoff + OFF_HEADER_KEY, ext);
            write_u32(self.image, eoff + OFF_HIGH_SEQ, chunk.len() as u32);
            write_u32(
                self.image,
                eoff + OFF_FIRST_DATA,
                chunk.first().copied().unwrap_or(0),
            );
            for (k, &block) in chunk.iter().enumerate() {
                let slot = BLOCK_TABLE_ENTRIES - 1 - k;
                write_u32(self.image, eoff + OFF_TABLE + slot * 4, block);
            }
            write_u32(self.image, eoff + OFF_PARENT, header);
            write_u32(
                self.image,
                eoff + OFF_EXTENSION,
                ext_blocks.get(i + 1).copied().unwrap_or(0),
            );
            write_u32(self.image, eoff + OFF_SEC_TYPE, ST_FILE as u32);
            fix_block_checksum(self.image, ext, OFF_CHECKSUM);
        }
        Ok(())
    }

    /// Collect every block a file occupies: header, extension blocks and
    /// all data blocks.
    fn file_blocks(&self, header: u32) -> Result<Vec<u32>, FloppyError> {
        let mut blocks = vec![header];
        let mut current = header;
        let mut guard = 0u32;
        loop {
            guard += 1;
            if guard > self.total_blocks {
                return Err(FloppyError::new(format!(
                    "extension chain of block {} does not terminate",
                    header
                )));
            }
            let off = self.check_block(current)?;
            let used = read_u32(self.image, off + OFF_HIGH_SEQ) as usize;
            if used > BLOCK_TABLE_ENTRIES {
                return Err(FloppyError::new(format!(
                    "block {} claims {} table entries, more than the {} a block holds",
                    current, used, BLOCK_TABLE_ENTRIES
                )));
            }
            for i in 0..used {
                let slot = BLOCK_TABLE_ENTRIES - 1 - i;
                let b = read_u32(self.image, off + OFF_TABLE + slot * 4);
                if b != 0 {
                    blocks.push(b);
                }
            }
            let next = read_u32(self.image, off + OFF_EXTENSION);
            if next == 0 {
                break;
            }
            blocks.push(next);
            current = next;
        }
        Ok(blocks)
    }

    fn delete_file_at(&mut self, dir_block: u32, header: u32) -> Result<(), FloppyError> {
        let blocks = self.file_blocks(header)?;
        self.unlink_from_dir(dir_block, header)?;
        for b in blocks {
            self.bitmap.mark_free(self.image, b)?;
        }
        Ok(())
    }

    /// Delete a file, returning its blocks to the free pool.
    pub fn delete_file(&mut self, path: &str) -> Result<(), FloppyError> {
        let (dir_block, name) = self.split_parent(path)?;
        let (header, _) = self
            .find_in_dir(dir_block, name)?
            .ok_or_else(|| FloppyError::new(format!("{:?} does not exist", path)))?;
        if self.sec_type_of(header)? != ST_FILE {
            return Err(FloppyError::new(format!(
                "{:?} is a directory — use delete_dir",
                path
            )));
        }
        self.delete_file_at(dir_block, header)
    }

    /// Create a directory. Fails if the name is already taken.
    pub fn create_dir(&mut self, path: &str) -> Result<u32, FloppyError> {
        let (parent, name) = self.split_parent(path)?;
        if self.find_in_dir(parent, name)?.is_some() {
            return Err(FloppyError::new(format!("{:?} already exists", path)));
        }
        let block = self.bitmap.allocate(self.image, self.root_block)?;
        let off = self.check_block(block)?;
        let (days, mins, ticks) = amiga_now();

        write_u32(self.image, off + OFF_TYPE, T_HEADER as u32);
        write_u32(self.image, off + OFF_HEADER_KEY, block);
        write_u32(self.image, off + OFF_DAYS, days);
        write_u32(self.image, off + OFF_MINS, mins);
        write_u32(self.image, off + OFF_TICKS, ticks);
        write_bcpl_string(self.image, off + OFF_NAME, name, 30);
        write_u32(self.image, off + OFF_SEC_TYPE, ST_USERDIR as u32);
        // The hash table is left zeroed: an empty directory.
        fix_block_checksum(self.image, block, OFF_CHECKSUM);

        if let Err(e) = self.link_into_dir(parent, block) {
            let _ = self.bitmap.mark_free(self.image, block);
            return Err(e);
        }
        Ok(block)
    }

    /// Whether a directory has any entries.
    fn dir_is_empty(&self, dir_block: u32) -> Result<bool, FloppyError> {
        let off = self.check_block(dir_block)?;
        for i in 0..self.hash_table_size() {
            if read_u32(self.image, off + OFF_TABLE + i * 4) != 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Delete an empty directory. Refuses a non-empty one rather than
    /// orphaning its contents — the blocks would stay marked used with
    /// nothing pointing at them.
    pub fn delete_dir(&mut self, path: &str) -> Result<(), FloppyError> {
        let (parent, name) = self.split_parent(path)?;
        let (block, _) = self
            .find_in_dir(parent, name)?
            .ok_or_else(|| FloppyError::new(format!("{:?} does not exist", path)))?;
        if self.sec_type_of(block)? != ST_USERDIR {
            return Err(FloppyError::new(format!("{:?} is not a directory", path)));
        }
        if !self.dir_is_empty(block)? {
            return Err(FloppyError::new(format!(
                "directory {:?} is not empty",
                path
            )));
        }
        self.unlink_from_dir(parent, block)?;
        self.bitmap.mark_free(self.image, block)?;
        Ok(())
    }

    /// Rename or move an entry (file or directory).
    ///
    /// Moving is the same operation as renaming: unlink from the old
    /// bucket, rewrite the name, link into the new parent. Both are done
    /// here so a move across directories cannot leave the entry in two
    /// chains or none.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), FloppyError> {
        let (from_dir, from_name) = self.split_parent(from)?;
        let (to_dir, to_name) = self.split_parent(to)?;
        let (block, _) = self
            .find_in_dir(from_dir, from_name)?
            .ok_or_else(|| FloppyError::new(format!("{:?} does not exist", from)))?;

        if let Some((existing, _)) = self.find_in_dir(to_dir, to_name)?
            && existing != block
        {
            return Err(FloppyError::new(format!("{:?} already exists", to)));
        }

        // Moving a directory into its own subtree would detach that subtree
        // from the volume entirely: the directory would still point at its
        // children, but nothing would point at it.
        if self.sec_type_of(block)? == ST_USERDIR && self.is_ancestor_of(block, to_dir)? {
            return Err(FloppyError::new(format!(
                "cannot move {:?} into its own subdirectory",
                from
            )));
        }

        self.unlink_from_dir(from_dir, block)?;
        let off = self.check_block(block)?;
        write_bcpl_string(self.image, off + OFF_NAME, to_name, 30);
        fix_block_checksum(self.image, block, OFF_CHECKSUM);
        self.link_into_dir(to_dir, block)
    }

    /// Whether `ancestor` lies on the parent chain of `block` (inclusive).
    fn is_ancestor_of(&self, ancestor: u32, mut block: u32) -> Result<bool, FloppyError> {
        let mut guard = 0u32;
        loop {
            if block == ancestor {
                return Ok(true);
            }
            if block == self.root_block {
                return Ok(false);
            }
            guard += 1;
            if guard > self.total_blocks {
                return Err(FloppyError::new("parent chain does not terminate"));
            }
            let off = self.check_block(block)?;
            let parent = read_u32(self.image, off + OFF_PARENT);
            if parent == 0 {
                return Ok(false);
            }
            block = parent;
        }
    }

    /// Set an entry's comment (AmigaDOS `FileNote`), max 79 characters.
    pub fn set_comment(&mut self, path: &str, comment: &str) -> Result<(), FloppyError> {
        if comment.len() > 79 {
            return Err(FloppyError::new(format!(
                "comment is {} bytes, the on-disk field holds 79",
                comment.len()
            )));
        }
        let block = self.resolve_existing(path)?;
        let off = self.check_block(block)?;
        write_bcpl_string(self.image, off + OFF_COMMENT, comment, 79);
        fix_block_checksum(self.image, block, OFF_CHECKSUM);
        Ok(())
    }

    /// Read back an entry's comment.
    pub fn comment(&self, path: &str) -> Result<String, FloppyError> {
        let block = self.resolve_existing(path)?;
        let off = self.check_block(block)?;
        Ok(read_bcpl_string(self.image, off + OFF_COMMENT, 79))
    }

    /// Set an entry's protection bits (the AmigaDOS RWED/HSPA mask).
    pub fn set_protection(&mut self, path: &str, bits: u32) -> Result<(), FloppyError> {
        let block = self.resolve_existing(path)?;
        let off = self.check_block(block)?;
        write_u32(self.image, off + OFF_PROTECT, bits);
        fix_block_checksum(self.image, block, OFF_CHECKSUM);
        Ok(())
    }

    /// Read back an entry's protection bits.
    pub fn protection(&self, path: &str) -> Result<u32, FloppyError> {
        let block = self.resolve_existing(path)?;
        let off = self.check_block(block)?;
        Ok(read_u32(self.image, off + OFF_PROTECT))
    }

    fn resolve_existing(&self, path: &str) -> Result<u32, FloppyError> {
        self.resolve(path)?
            .ok_or_else(|| FloppyError::new(format!("{:?} does not exist", path)))
    }

    /// Set the volume name in the root block.
    pub fn set_volume_name(&mut self, name: &str) -> Result<(), FloppyError> {
        if name.len() > 30 {
            return Err(FloppyError::new(format!(
                "volume name is {} bytes, the on-disk field holds 30",
                name.len()
            )));
        }
        let root = self.root_block;
        let off = self.check_block(root)?;
        write_bcpl_string(self.image, off + OFF_NAME, name, 30);
        fix_block_checksum(self.image, root, OFF_CHECKSUM);
        Ok(())
    }
}

/// Current time in AmigaDOS form: days since 1978-01-01, minutes past
/// midnight, and ticks (1/50 s) within the minute.
fn amiga_now() -> (u32, u32, u32) {
    const SECONDS_1970_TO_1978: u64 = 252_460_800;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(SECONDS_1970_TO_1978);
    let amiga_secs = now.saturating_sub(SECONDS_1970_TO_1978);
    let days = (amiga_secs / 86_400) as u32;
    let rem = amiga_secs % 86_400;
    ((days), (rem / 60) as u32, ((rem % 60) * 50) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adf_writer::format_empty_ofs_disk;

    /// Standard DD floppy: 1760 blocks, root at 880.
    fn fresh() -> (Vec<u8>, u32, u32) {
        let image = format_empty_ofs_disk("TestDisk").unwrap();
        let total = (image.len() / BLOCK_SIZE) as u32;
        (image, total / 2, total)
    }

    #[test]
    fn format_creates_a_usable_bitmap() {
        // Before this, format_empty_*_disk set bm_flag=-1 but no bitmap
        // pages at all — a disk AmigaDOS mounts but cannot write to.
        let (image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).expect("bitmap pages must be present");
        // Everything free except the root block and the one bitmap block.
        assert_eq!(bm.free_count(&image), total - FIRST_ALLOCATABLE_BLOCK - 2);
        assert!(!bm.is_free(&image, root), "root block must be in use");
        assert!(!bm.is_free(&image, root + 1), "bitmap block must be in use");
        assert!(bm.is_free(&image, FIRST_ALLOCATABLE_BLOCK));
        assert!(bm.is_free(&image, total - 1));
    }

    #[test]
    fn bitmap_block_checksum_is_valid() {
        // A real volume's bitmap block sums to zero over all longwords;
        // verified against Cybernetix.adf before writing this.
        let (image, root, _) = fresh();
        let start = block_offset(root + 1);
        let sum = image[start..start + BLOCK_SIZE]
            .chunks_exact(4)
            .fold(0u32, |a, c| {
                a.wrapping_add(u32::from_be_bytes(c.try_into().unwrap()))
            });
        assert_eq!(sum, 0, "bitmap block checksum does not balance");
    }

    #[test]
    fn allocate_marks_used_and_zeroes() {
        let (mut image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).unwrap();
        let before = bm.free_count(&image);

        // Dirty the block first so the zeroing is actually observable.
        let probe = bm.allocate(&mut image, root).unwrap();
        bm.mark_free(&mut image, probe).unwrap();
        image[block_offset(probe)..block_offset(probe) + BLOCK_SIZE].fill(0xAB);

        let block = bm.allocate(&mut image, root).unwrap();
        assert_eq!(block, probe, "allocator should reuse the freed block");
        assert!(!bm.is_free(&image, block));
        assert_eq!(bm.free_count(&image), before - 1);
        assert!(
            image[block_offset(block)..block_offset(block) + BLOCK_SIZE]
                .iter()
                .all(|&b| b == 0),
            "allocated block must be zeroed, or stale data leaks into the new file"
        );
    }

    #[test]
    fn free_returns_the_block() {
        let (mut image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).unwrap();
        let before = bm.free_count(&image);
        let block = bm.allocate(&mut image, root).unwrap();
        bm.mark_free(&mut image, block).unwrap();
        assert_eq!(bm.free_count(&image), before);
        assert!(bm.is_free(&image, block));
    }

    #[test]
    fn allocate_many_rolls_back_when_the_volume_is_full() {
        // A partial allocation that kept its blocks marked used would leak
        // space nothing references — the disk would shrink with every
        // failed write.
        let (mut image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).unwrap();
        let free = bm.free_count(&image);
        let err = bm.allocate_many(&mut image, root, free as usize + 1);
        assert!(err.is_err(), "over-allocation must fail");
        assert_eq!(
            bm.free_count(&image),
            free,
            "failed allocation must not consume blocks"
        );
    }

    #[test]
    fn allocation_is_exhaustive_and_then_fails() {
        let (mut image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).unwrap();
        let free = bm.free_count(&image) as usize;
        let blocks = bm.allocate_many(&mut image, root, free).unwrap();
        assert_eq!(blocks.len(), free);
        assert_eq!(bm.free_count(&image), 0);
        assert!(bm.allocate(&mut image, root).is_err());
        // No block handed out twice, and none outside the volume.
        let mut seen = blocks.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), blocks.len(), "allocator returned a duplicate");
        assert!(
            blocks
                .iter()
                .all(|&b| b >= FIRST_ALLOCATABLE_BLOCK && b < total)
        );
    }

    #[test]
    fn bits_past_the_end_of_the_volume_are_not_free() {
        // One bitmap block covers 4064 blocks; a DD floppy has 1760, so the
        // tail describes blocks that do not exist.
        let (image, root, total) = fresh();
        let bm = Bitmap::from_root(&image, root, total).unwrap();
        assert!(
            !bm.is_free(&image, total),
            "block past the end reads as free"
        );
        assert!(!bm.is_free(&image, total + 500));
    }

    #[test]
    fn volume_without_bitmap_is_rejected() {
        // Images produced by older versions of format_empty_*_disk have
        // bm_flag=-1 but no pages; writing to them must fail loudly rather
        // than corrupt the volume.
        let (mut image, root, total) = fresh();
        let off = block_offset(root) + OFF_ROOT_BM_PAGES;
        write_u32(&mut image, off, 0);
        let err = Bitmap::from_root(&image, root, total).unwrap_err();
        assert!(
            err.0.contains("no bitmap blocks"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn pages_needed_covers_common_geometries() {
        assert_eq!(Bitmap::pages_needed(1760), 1); // DD floppy
        assert_eq!(Bitmap::pages_needed(3520), 1); // HD floppy, still one
        assert_eq!(Bitmap::pages_needed(4066), 1); // exactly 4064 covered
        assert_eq!(Bitmap::pages_needed(4067), 2);
    }

    /// Read a volume back through the real reader, so the tests verify the
    /// on-disk result rather than the writer's own view of it.
    fn read_back(image: &[u8]) -> Vec<(String, bool, u32)> {
        use crate::amigados::AmigaFs;
        use crate::floppy_base::{FloppyError as FE, FloppyImageReader};

        struct Mem<'a>(&'a [u8]);
        impl FloppyImageReader for Mem<'_> {
            fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FE> {
                let block = ((track * 2 + side) * 11 + sector) as usize;
                let off = block * BLOCK_SIZE;
                self.0
                    .get(off..off + BLOCK_SIZE)
                    .map(|s| s.to_vec())
                    .ok_or_else(|| FE::new("out of range"))
            }
        }
        let mut mem = Mem(image);
        let mut fs = AmigaFs::mount(&mut mem, 80).expect("volume must mount");
        let root = fs.root_block_num();
        let mut out: Vec<(String, bool, u32)> = fs
            .list_dir(root)
            .expect("root must list")
            .into_iter()
            .map(|e| (e.name, e.is_dir, e.size))
            .collect();
        out.sort();
        out
    }

    fn read_file(image: &[u8], path: &str) -> Vec<u8> {
        use crate::amigados::AmigaFs;
        use crate::floppy_base::{FloppyError as FE, FloppyImageReader};
        struct Mem<'a>(&'a [u8]);
        impl FloppyImageReader for Mem<'_> {
            fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FE> {
                let block = ((track * 2 + side) * 11 + sector) as usize;
                let off = block * BLOCK_SIZE;
                self.0
                    .get(off..off + BLOCK_SIZE)
                    .map(|s| s.to_vec())
                    .ok_or_else(|| FE::new("out of range"))
            }
        }
        let mut mem = Mem(image);
        let mut fs = AmigaFs::mount(&mut mem, 80).unwrap();
        fs.read_file_at_path(path).expect("file must read back")
    }

    #[test]
    fn written_file_reads_back_through_the_reader() {
        // The writer and reader are separate implementations; this is the
        // check that they agree on the layout.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.write_file("HELLO.TXT", b"Hello Amiga").unwrap();
            w.write_file("BIG.DAT", &payload).unwrap();
        }
        assert_eq!(
            read_back(&image),
            vec![
                ("BIG.DAT".to_string(), false, 5000),
                ("HELLO.TXT".to_string(), false, 11),
            ]
        );
        assert_eq!(read_file(&image, "HELLO.TXT"), b"Hello Amiga");
        assert_eq!(read_file(&image, "BIG.DAT"), payload);
    }

    #[test]
    fn ffs_volume_roundtrips_too() {
        // FFS data blocks are raw 512-byte payload with no block header —
        // a different code path from OFS, and the one where an off-by-24
        // would silently corrupt every file.
        let mut image = crate::adf_writer::format_empty_ffs_disk("TestDisk").unwrap();
        let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            assert!(w.is_ffs());
            w.write_file("BIG.DAT", &payload).unwrap();
        }
        assert_eq!(read_file(&image, "BIG.DAT"), payload);
    }

    #[test]
    fn file_spanning_extension_blocks_roundtrips() {
        // A header's block table holds 72 entries; anything larger needs an
        // extension chain. At 488 payload bytes per OFS block that is
        // 35_136 bytes, so this file needs three.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 253) as u8).collect();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.write_file("HUGE.BIN", &payload).unwrap();
        }
        assert_eq!(read_file(&image, "HUGE.BIN"), payload);
        assert_eq!(read_back(&image)[0].2, 100_000);
    }

    #[test]
    fn empty_file_is_valid() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.write_file("EMPTY", b"").unwrap();
        }
        assert_eq!(read_back(&image), vec![("EMPTY".to_string(), false, 0)]);
        assert_eq!(read_file(&image, "EMPTY"), Vec::<u8>::new());
    }

    #[test]
    fn deleting_a_file_frees_exactly_its_blocks() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let payload: Vec<u8> = (0..20_000u32).map(|i| i as u8).collect();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        let before = w.free_blocks();
        w.write_file("TMP.DAT", &payload).unwrap();
        assert!(w.free_blocks() < before);
        w.delete_file("TMP.DAT").unwrap();
        assert_eq!(
            w.free_blocks(),
            before,
            "delete must return every block the file used"
        );
        drop(w);
        assert!(read_back(&image).is_empty());
    }

    #[test]
    fn overwriting_reuses_space_rather_than_leaking_it() {
        // Rewriting the same file repeatedly must not consume the volume.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.write_file("A.DAT", &vec![1u8; 10_000]).unwrap();
        let after_first = w.free_blocks();
        for _ in 0..5 {
            w.write_file("A.DAT", &vec![2u8; 10_000]).unwrap();
        }
        assert_eq!(w.free_blocks(), after_first, "rewrite leaked blocks");
        drop(w);
        assert_eq!(read_file(&image, "A.DAT"), vec![2u8; 10_000]);
    }

    #[test]
    fn directories_nest_and_hold_files() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.create_dir("C").unwrap();
            w.create_dir("C/SUB").unwrap();
            w.write_file("C/SUB/DEEP.TXT", b"deep").unwrap();
        }
        assert_eq!(read_file(&image, "C/SUB/DEEP.TXT"), b"deep");
        let entries = read_back(&image);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].1, "C must be reported as a directory");
    }

    #[test]
    fn non_empty_directory_cannot_be_deleted() {
        // Removing it anyway would orphan the contents: blocks still marked
        // used, nothing pointing at them.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.create_dir("C").unwrap();
        w.write_file("C/X", b"x").unwrap();
        let err = w.delete_dir("C").unwrap_err();
        assert!(err.0.contains("not empty"), "unexpected error: {}", err);
        w.delete_file("C/X").unwrap();
        w.delete_dir("C").unwrap();
    }

    #[test]
    fn many_files_in_one_directory_all_resolve() {
        // Exercises hash collisions: 72 buckets, so 200 names guarantee
        // chains several entries deep. A broken chain insert would lose
        // files silently.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let names: Vec<String> = (0..200).map(|i| format!("FILE{:03}", i)).collect();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            for (i, n) in names.iter().enumerate() {
                w.write_file(n, format!("body-{}", i).as_bytes()).unwrap();
            }
        }
        let listed = read_back(&image);
        assert_eq!(listed.len(), names.len(), "some entries went missing");
        for (i, n) in names.iter().enumerate() {
            assert_eq!(read_file(&image, n), format!("body-{}", i).as_bytes());
        }
    }

    #[test]
    fn deleting_from_the_middle_of_a_hash_chain_keeps_the_rest() {
        // Names chosen so all three share a bucket; removing the middle one
        // must relink, not truncate the chain.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut names: Vec<String> = Vec::new();
        let target_bucket = amiga_hash(b"AAA");
        for i in 0..500 {
            let n = format!("N{}", i);
            if amiga_hash(n.as_bytes()) == target_bucket {
                names.push(n);
            }
            if names.len() == 3 {
                break;
            }
        }
        assert_eq!(names.len(), 3, "test needs three colliding names");

        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            for n in &names {
                w.write_file(n, n.as_bytes()).unwrap();
            }
            w.delete_file(&names[1]).unwrap();
        }
        let listed: Vec<String> = read_back(&image).into_iter().map(|e| e.0).collect();
        assert!(listed.contains(&names[0]), "head of chain lost");
        assert!(listed.contains(&names[2]), "tail of chain lost");
        assert!(!listed.contains(&names[1]));
        assert_eq!(read_file(&image, &names[2]), names[2].as_bytes());
    }

    #[test]
    fn rename_within_a_directory() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.write_file("OLD.TXT", b"content").unwrap();
            w.rename("OLD.TXT", "NEW.TXT").unwrap();
        }
        assert_eq!(read_back(&image), vec![("NEW.TXT".to_string(), false, 7)]);
        assert_eq!(read_file(&image, "NEW.TXT"), b"content");
    }

    #[test]
    fn rename_moves_between_directories() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.create_dir("SRC").unwrap();
            w.create_dir("DST").unwrap();
            w.write_file("SRC/F.TXT", b"moved").unwrap();
            w.rename("SRC/F.TXT", "DST/F.TXT").unwrap();
        }
        assert_eq!(read_file(&image, "DST/F.TXT"), b"moved");
        let mut image2 = image.clone();
        let w = AmigaFsWriter::mount(&mut image2).unwrap();
        assert!(w.resolve("SRC/F.TXT").unwrap().is_none(), "still in source");
    }

    #[test]
    fn rename_rejects_moving_a_directory_into_itself() {
        // Would detach the whole subtree: it keeps pointing at its
        // children, but nothing points at it.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.create_dir("A").unwrap();
        w.create_dir("A/B").unwrap();
        let err = w.rename("A", "A/B/A").unwrap_err();
        assert!(err.0.contains("own subdirectory"), "unexpected: {}", err);
    }

    #[test]
    fn rename_onto_an_existing_name_is_refused() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.write_file("A", b"a").unwrap();
        w.write_file("B", b"b").unwrap();
        assert!(w.rename("A", "B").is_err());
        // Both must survive the refusal intact.
        drop(w);
        assert_eq!(read_file(&image, "A"), b"a");
        assert_eq!(read_file(&image, "B"), b"b");
    }

    #[test]
    fn comment_and_protection_persist() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.write_file("F.TXT", b"x").unwrap();
            w.set_comment("F.TXT", "written by the toolkit").unwrap();
            w.set_protection("F.TXT", 0x0F).unwrap();
        }
        let mut image2 = image.clone();
        let w = AmigaFsWriter::mount(&mut image2).unwrap();
        assert_eq!(w.comment("F.TXT").unwrap(), "written by the toolkit");
        assert_eq!(w.protection("F.TXT").unwrap(), 0x0F);
        // The file itself must still be readable after the metadata edits.
        drop(w);
        assert_eq!(read_file(&image, "F.TXT"), b"x");
    }

    #[test]
    fn shorter_comment_leaves_no_tail_behind() {
        // The BCPL field is fixed-size; a naive overwrite would leave the
        // old suffix in place past the new length byte.
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.write_file("F", b"x").unwrap();
        w.set_comment("F", "a very long comment indeed").unwrap();
        w.set_comment("F", "short").unwrap();
        assert_eq!(w.comment("F").unwrap(), "short");
        let off = block_offset(w.resolve("F").unwrap().unwrap()) + OFF_COMMENT;
        assert!(
            w.image[off + 1 + 5..off + 1 + 26].iter().all(|&b| b == 0),
            "stale comment bytes left in the block"
        );
    }

    #[test]
    fn oversized_names_and_comments_are_refused() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        assert!(w.write_file(&"N".repeat(31), b"x").is_err());
        w.write_file("OK", b"x").unwrap();
        assert!(w.set_comment("OK", &"c".repeat(80)).is_err());
        assert!(w.set_volume_name(&"V".repeat(31)).is_err());
    }

    #[test]
    fn volume_name_is_settable_and_read_by_the_reader() {
        let mut image = format_empty_ofs_disk("Original").unwrap();
        {
            let mut w = AmigaFsWriter::mount(&mut image).unwrap();
            w.set_volume_name("Renamed").unwrap();
        }
        let root = (image.len() / BLOCK_SIZE / 2) as u32;
        let off = block_offset(root) + OFF_NAME;
        assert_eq!(read_bcpl_string(&image, off, 30), "Renamed");
    }

    #[test]
    fn writing_more_than_fits_fails_without_corrupting_the_volume() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        w.write_file("KEEP.TXT", b"survivor").unwrap();
        let free_before = w.free_blocks();
        let too_big = vec![0u8; (free_before as usize + 10) * OFS_DATA_PER_BLOCK];
        assert!(w.write_file("TOOBIG.DAT", &too_big).is_err());
        assert_eq!(
            w.free_blocks(),
            free_before,
            "failed write must not consume blocks"
        );
        drop(w);
        // The pre-existing file must be untouched.
        assert_eq!(read_file(&image, "KEEP.TXT"), b"survivor");
        let listed: Vec<String> = read_back(&image).into_iter().map(|e| e.0).collect();
        assert_eq!(listed, vec!["KEEP.TXT".to_string()]);
    }

    #[test]
    fn mount_rejects_a_non_amigados_image() {
        let mut junk = vec![0u8; 1760 * BLOCK_SIZE];
        assert!(AmigaFsWriter::mount(&mut junk).is_err());
        junk[0..3].copy_from_slice(b"DOS");
        // Signature alone is not enough — the root block must be valid.
        assert!(AmigaFsWriter::mount(&mut junk).is_err());
    }

    #[test]
    fn writing_into_a_missing_directory_fails() {
        let mut image = format_empty_ofs_disk("TestDisk").unwrap();
        let mut w = AmigaFsWriter::mount(&mut image).unwrap();
        assert!(w.write_file("NOPE/F.TXT", b"x").is_err());
        // And a path whose parent is a file, not a directory.
        w.write_file("AFILE", b"x").unwrap();
        assert!(w.write_file("AFILE/CHILD", b"x").is_err());
    }

    #[test]
    fn hash_matches_the_reader() {
        // The two modules carry separate copies; a divergence would put
        // files in buckets the reader never looks in.
        for name in ["cybernetix", "S", "IMPORTANT.DOC", "a", "Zzz9", "add44k"] {
            assert_eq!(
                amiga_hash(name.as_bytes()),
                crate::amigados::amiga_hash_for_test(name.as_bytes()),
                "hash mismatch for {:?}",
                name
            );
        }
    }
}
