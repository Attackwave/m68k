//! ADF image creation/writing and bootblock checksum repair.

use std::fs;
use std::path::Path;

use crate::floppy_base::FloppyError;

const SECTOR_SIZE: usize = 512;
const SIDES: usize = 2;

/// Standard double-density ADF: 80 tracks, 2 sides, 11 sectors/track.
pub const DD_TRACKS: usize = 80;
pub const DD_SECTORS_PER_TRACK: usize = 11;
pub const DD_SIZE: usize = DD_TRACKS * SIDES * DD_SECTORS_PER_TRACK * SECTOR_SIZE;

/// High-density ADF: 80 tracks, 2 sides, 22 sectors/track.
pub const HD_SECTORS_PER_TRACK: usize = 22;
pub const HD_SIZE: usize = DD_TRACKS * SIDES * HD_SECTORS_PER_TRACK * SECTOR_SIZE;

/// Build a raw, unformatted ADF image of the given size, filled with
/// `fill_byte` (conventionally 0x00). This produces a disk with no
/// filesystem structure at all — callers that want a mountable AmigaDOS
/// disk should also write a bootblock (see [`write_bootblock`]) and a
/// root block into the returned buffer, or use
/// [`format_empty_ofs_disk`]/[`format_empty_ffs_disk`] for a ready-to-use
/// empty filesystem.
pub fn create_blank_image(size: usize, fill_byte: u8) -> Result<Vec<u8>, FloppyError> {
    if size == 0 || !size.is_multiple_of(SECTOR_SIZE) {
        return Err(FloppyError::new(format!(
            "image size {} is not a multiple of the sector size {}",
            size, SECTOR_SIZE
        )));
    }
    Ok(vec![fill_byte; size])
}

/// Write an in-memory ADF image buffer to disk.
pub fn write_adf_file(path: impl AsRef<Path>, image: &[u8]) -> Result<(), FloppyError> {
    fs::write(path, image).map_err(|e| FloppyError::new(e.to_string()))
}

/// Compute the AmigaDOS bootblock checksum (the one's-complement /
/// end-around-carry variant used only for the 1024-byte bootblock, not
/// the same formula as other block types — see `amigados` module docs).
///
/// Sums every big-endian 32-bit word of `bootblock` (treating the stored
/// checksum field at offset 4 as zero during the sum, per the standard
/// algorithm), folding any carry out of bit 31 back into the sum
/// (end-around carry), then returns the bitwise NOT of the result.
pub fn compute_bootblock_checksum(bootblock: &[u8; 1024]) -> u32 {
    let mut sum: u64 = 0;
    for chunk in bootblock.chunks_exact(4) {
        let offset = chunk.as_ptr() as usize - bootblock.as_ptr() as usize;
        if offset == 4 {
            continue; // the checksum field itself, treated as 0
        }
        let word = u32::from_be_bytes(chunk.try_into().unwrap()) as u64;
        sum += word;
        if sum > 0xFFFF_FFFF {
            sum = (sum & 0xFFFF_FFFF) + 1; // end-around carry
        }
    }
    !(sum as u32)
}

/// Recompute and write the bootblock checksum in place (offset 4..8 of
/// the 1024-byte bootblock at the start of `image`), for use as a
/// `--fix-bootblock` repair: e.g. after the caller has hand-edited
/// bootblock code, or to repair a bootblock a buggy tool wrote with a
/// stale/incorrect checksum.
pub fn fix_bootblock_checksum(image: &mut [u8]) -> Result<(), FloppyError> {
    if image.len() < 1024 {
        return Err(FloppyError::new(
            "image is smaller than one bootblock (1024 bytes)",
        ));
    }
    let mut boot = [0u8; 1024];
    boot.copy_from_slice(&image[..1024]);
    let checksum = compute_bootblock_checksum(&boot);
    image[4..8].copy_from_slice(&checksum.to_be_bytes());
    Ok(())
}

/// Verify the bootblock checksum at the start of `image` is correct.
pub fn verify_bootblock_checksum(image: &[u8]) -> Result<bool, FloppyError> {
    if image.len() < 1024 {
        return Err(FloppyError::new(
            "image is smaller than one bootblock (1024 bytes)",
        ));
    }
    let mut boot = [0u8; 1024];
    boot.copy_from_slice(&image[..1024]);
    let stored = u32::from_be_bytes(image[4..8].try_into().unwrap());
    Ok(compute_bootblock_checksum(&boot) == stored)
}

/// AmigaDOS "sum, then negate" block checksum used by the root block,
/// file header blocks, user directory blocks, and OFS data blocks — the
/// two's-complement variant, distinct from the bootblock's one's
/// complement/end-around-carry variant above. `checksum_offset` is the
/// byte offset of the checksum field within `block` (0x14 for all of the
/// header-style block types this crate writes).
pub fn compute_block_checksum(block: &[u8; 512], checksum_offset: usize) -> u32 {
    let mut sum: u32 = 0;
    for (offset, chunk) in block.chunks_exact(4).enumerate() {
        let offset = offset * 4;
        if offset == checksum_offset {
            continue;
        }
        let word = u32::from_be_bytes(chunk.try_into().unwrap());
        sum = sum.wrapping_add(word);
    }
    0u32.wrapping_sub(sum)
}

fn write_bcpl_string(block: &mut [u8; 512], offset: usize, s: &str, max_len: usize) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(max_len);
    block[offset] = len as u8;
    block[offset + 1..offset + 1 + len].copy_from_slice(&bytes[..len]);
}

fn block_offset(block_num: u32) -> usize {
    block_num as usize * SECTOR_SIZE
}

/// Build a minimal, empty (no files), mountable AmigaDOS root block into
/// `image` at the standard location (`total_blocks / 2`), and write a
/// matching bootblock (OFS or FFS per `ffs`, non-bootable — no boot code,
/// just the filesystem signature) at the start of the image. `disk_name`
/// becomes the volume's BCPL name (silently truncated to 30 bytes if
/// longer, matching the on-disk field's fixed size).
///
/// This produces a disk with a valid, empty hash table (no files/dirs) —
/// callers that want actual file content must write file header + data
/// blocks themselves following the `amigados` module's layout reference;
/// this function only sets up the boot/root structure new files would
/// hash into.
fn format_empty_disk(size: usize, ffs: bool, disk_name: &str) -> Result<Vec<u8>, FloppyError> {
    let mut image = create_blank_image(size, 0)?;
    let total_blocks = (size / SECTOR_SIZE) as u32;
    if total_blocks == 0 || !total_blocks.is_multiple_of(2) {
        return Err(FloppyError::new(format!(
            "image block count {} is not evenly halvable to locate a root block",
            total_blocks
        )));
    }
    let root_block_num = total_blocks / 2;

    // Bootblock: "DOS" + flags byte (bit0 = FFS), zeroed boot code, root
    // block pointer, checksum computed last.
    image[0..3].copy_from_slice(b"DOS");
    image[3] = if ffs { 1 } else { 0 };
    image[8..12].copy_from_slice(&root_block_num.to_be_bytes());
    fix_bootblock_checksum(&mut image)?;

    // Root block: T_HEADER, ht_size=72, empty hash table, valid-but-empty
    // bitmap (bm_flag=-1, no bitmap block pointers — a real filesystem
    // needs actual bitmap blocks marking every free block, which this
    // minimal empty-disk helper doesn't allocate; a full block-allocator
    // is out of scope here, matching the "no data blocks are ever
    // written yet" empty-disk use case), ST_ROOT at the end.
    let mut root = [0u8; 512];
    root[0..4].copy_from_slice(&(2i32).to_be_bytes()); // type = T_HEADER
    root[0x0C..0x10].copy_from_slice(&(72u32).to_be_bytes()); // ht_size
    root[0x138..0x13C].copy_from_slice(&(-1i32).to_be_bytes()); // bm_flag = valid
    write_bcpl_string(&mut root, 0x1B0, disk_name, 30);
    root[0x1FC..0x200].copy_from_slice(&(1i32).to_be_bytes()); // sec_type = ST_ROOT
    let checksum = compute_block_checksum(&root, 0x14);
    root[0x14..0x18].copy_from_slice(&checksum.to_be_bytes());

    let root_offset = block_offset(root_block_num);
    image[root_offset..root_offset + 512].copy_from_slice(&root);

    Ok(image)
}

/// Create a minimal, empty, mountable OFS double-density disk image
/// (880 KiB: 80 tracks x 2 sides x 11 sectors x 512 bytes).
pub fn format_empty_ofs_disk(disk_name: &str) -> Result<Vec<u8>, FloppyError> {
    format_empty_disk(DD_SIZE, false, disk_name)
}

/// Create a minimal, empty, mountable FFS double-density disk image.
pub fn format_empty_ffs_disk(disk_name: &str) -> Result<Vec<u8>, FloppyError> {
    format_empty_disk(DD_SIZE, true, disk_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amigados::AmigaFs;
    use crate::floppy_base::FloppyImageReader;

    struct MemImage {
        data: Vec<u8>,
    }
    impl FloppyImageReader for MemImage {
        fn read_sector(
            &mut self,
            track: u32,
            side: u32,
            sector: u32,
        ) -> Result<Vec<u8>, FloppyError> {
            let block = (track * SIDES as u32 + side) * DD_SECTORS_PER_TRACK as u32 + sector;
            let offset = block as usize * SECTOR_SIZE;
            if offset + SECTOR_SIZE > self.data.len() {
                return Err(FloppyError::new("sector out of range"));
            }
            Ok(self.data[offset..offset + SECTOR_SIZE].to_vec())
        }
    }

    #[test]
    fn test_create_blank_image_rejects_non_sector_multiple() {
        assert!(create_blank_image(100, 0).is_err());
    }

    #[test]
    fn test_create_blank_image_fills_correctly() {
        let img = create_blank_image(SECTOR_SIZE * 2, 0xAA).unwrap();
        assert_eq!(img.len(), SECTOR_SIZE * 2);
        assert!(img.iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn test_bootblock_checksum_round_trips() {
        let mut boot = [0u8; 1024];
        boot[0..3].copy_from_slice(b"DOS");
        boot[3] = 1;
        boot[8..12].copy_from_slice(&880u32.to_be_bytes());
        let checksum = compute_bootblock_checksum(&boot);
        boot[4..8].copy_from_slice(&checksum.to_be_bytes());

        // Verifying: summing all longwords including the (now correct)
        // stored checksum, with end-around carry, must total exactly
        // 0xFFFFFFFF.
        let mut sum: u64 = 0;
        for chunk in boot.chunks_exact(4) {
            sum += u32::from_be_bytes(chunk.try_into().unwrap()) as u64;
            if sum > 0xFFFF_FFFF {
                sum = (sum & 0xFFFF_FFFF) + 1;
            }
        }
        assert_eq!(sum as u32, 0xFFFF_FFFF);
    }

    #[test]
    fn test_fix_bootblock_checksum_repairs_corrupted_field() {
        let mut image = vec![0u8; 1024];
        image[0..3].copy_from_slice(b"DOS");
        image[8..12].copy_from_slice(&880u32.to_be_bytes());
        image[4..8].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes()); // wrong checksum

        assert!(!verify_bootblock_checksum(&image).unwrap());
        fix_bootblock_checksum(&mut image).unwrap();
        assert!(verify_bootblock_checksum(&image).unwrap());
    }

    #[test]
    fn test_format_empty_ofs_disk_is_mountable_and_empty() {
        let image = format_empty_ofs_disk("MyDisk").unwrap();
        assert_eq!(image.len(), DD_SIZE);
        assert!(verify_bootblock_checksum(&image).unwrap());

        let mut reader = MemImage { data: image };
        let mut fs = AmigaFs::mount(&mut reader, DD_TRACKS as u32).unwrap();
        assert_eq!(fs.kind, crate::amigados::FsKind::Ofs);
        let entries = fs.list_dir(fs.root_block_num()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_format_empty_ffs_disk_is_mountable_and_empty() {
        let image = format_empty_ffs_disk("MyFfsDisk").unwrap();
        let mut reader = MemImage { data: image };
        let mut fs = AmigaFs::mount(&mut reader, DD_TRACKS as u32).unwrap();
        assert_eq!(fs.kind, crate::amigados::FsKind::Ffs);
        assert!(fs.list_dir(fs.root_block_num()).unwrap().is_empty());
    }

    /// Manually injects a file (header block + data blocks) into an
    /// otherwise-empty formatted disk, following the same on-disk layout
    /// `amigados::AmigaFs` expects, then verifies `list_dir`/`find_entry`/
    /// `read_file` round-trip the file back out correctly. This is the
    /// end-to-end check that the writer's root-block hash table and the
    /// reader's traversal/data-block-reassembly logic actually agree with
    /// each other, for both the OFS (per-block header + 488-byte payload)
    /// and FFS (no header, full 512-byte payload) data block shapes.
    fn inject_and_read_back_file(ffs: bool) {
        use crate::amigados::amiga_hash_for_test as amiga_hash;

        let mut image = if ffs {
            format_empty_ffs_disk("Disk").unwrap()
        } else {
            format_empty_ofs_disk("Disk").unwrap()
        };
        let total_blocks = (DD_SIZE / SECTOR_SIZE) as u32;
        let root_block_num = total_blocks / 2;

        // Content spans more than one data block (488/512-byte payload)
        // to exercise the data-block-chain traversal, not just a single
        // block.
        let content: Vec<u8> = (0..1200u32).map(|i| (i % 251) as u8).collect();
        let payload_per_block = if ffs { SECTOR_SIZE } else { SECTOR_SIZE - 24 };
        let num_data_blocks = content.len().div_ceil(payload_per_block);

        // Place the file header immediately after the root block, and
        // data blocks right after that — arbitrary but collision-free
        // block numbers for this synthetic test image.
        let file_header_block_num = root_block_num + 1;
        let data_block_nums: Vec<u32> = (0..num_data_blocks as u32)
            .map(|i| root_block_num + 2 + i)
            .collect();

        // Data blocks.
        for (seq, &block_num) in data_block_nums.iter().enumerate() {
            let mut block = [0u8; 512];
            let start = seq * payload_per_block;
            let end = (start + payload_per_block).min(content.len());
            let chunk = &content[start..end];
            if ffs {
                block[..chunk.len()].copy_from_slice(chunk);
            } else {
                block[0..4].copy_from_slice(&(8i32).to_be_bytes()); // type = T_DATA
                block[4..8].copy_from_slice(&file_header_block_num.to_be_bytes());
                block[8..12].copy_from_slice(&((seq + 1) as u32).to_be_bytes());
                block[12..16].copy_from_slice(&(chunk.len() as u32).to_be_bytes());
                let next = data_block_nums.get(seq + 1).copied().unwrap_or(0);
                block[16..20].copy_from_slice(&next.to_be_bytes());
                block[24..24 + chunk.len()].copy_from_slice(chunk);
                let checksum = compute_block_checksum(&block, 0x14);
                block[0x14..0x18].copy_from_slice(&checksum.to_be_bytes());
            }
            let off = block_offset(block_num);
            image[off..off + 512].copy_from_slice(&block);
        }

        // File header block: data block pointers stored highest-index-
        // first (table[high_seq-1] = first data block).
        let mut header = [0u8; 512];
        header[0..4].copy_from_slice(&(2i32).to_be_bytes()); // type = T_HEADER
        header[4..8].copy_from_slice(&file_header_block_num.to_be_bytes());
        header[8..12].copy_from_slice(&(num_data_blocks as u32).to_be_bytes()); // high_seq
        header[0x10..0x14].copy_from_slice(&data_block_nums[0].to_be_bytes()); // first_data
        for (i, &block_num) in data_block_nums.iter().enumerate() {
            let table_idx = num_data_blocks - 1 - i;
            let off = 0x018 + table_idx * 4;
            header[off..off + 4].copy_from_slice(&block_num.to_be_bytes());
        }
        header[0x144..0x148].copy_from_slice(&(content.len() as u32).to_be_bytes()); // byte_size
        write_bcpl_string(&mut header, 0x1B0, "hello.txt", 30);
        header[0x1F4..0x1F8].copy_from_slice(&root_block_num.to_be_bytes()); // parent
        header[0x1FC..0x200].copy_from_slice(&(-3i32).to_be_bytes()); // sec_type = ST_FILE
        let checksum = compute_block_checksum(&header, 0x14);
        header[0x14..0x18].copy_from_slice(&checksum.to_be_bytes());

        let hoff = block_offset(file_header_block_num);
        image[hoff..hoff + 512].copy_from_slice(&header);

        // Wire the file into the root block's hash table + recompute its
        // checksum (root block content changed).
        let root_off = block_offset(root_block_num);
        let idx = amiga_hash(b"hello.txt") as usize;
        image[root_off + 0x018 + idx * 4..root_off + 0x018 + idx * 4 + 4]
            .copy_from_slice(&file_header_block_num.to_be_bytes());
        let mut root = [0u8; 512];
        root.copy_from_slice(&image[root_off..root_off + 512]);
        root[0x14..0x18].copy_from_slice(&0u32.to_be_bytes());
        let checksum = compute_block_checksum(&root, 0x14);
        root[0x14..0x18].copy_from_slice(&checksum.to_be_bytes());
        image[root_off..root_off + 512].copy_from_slice(&root);

        let mut reader = MemImage { data: image };
        let mut fs = AmigaFs::mount(&mut reader, DD_TRACKS as u32).unwrap();
        let entries = fs.list_dir(fs.root_block_num()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "hello.txt");
        assert!(!entries[0].is_dir);
        assert_eq!(entries[0].size, content.len() as u32);

        let found = fs
            .find_entry(fs.root_block_num(), "hello.txt")
            .unwrap()
            .expect("file must be found by hash lookup");
        assert_eq!(found.block, file_header_block_num);

        let read_back = fs.read_file(file_header_block_num).unwrap();
        assert_eq!(read_back, content);
    }

    #[test]
    fn test_ofs_file_round_trip() {
        inject_and_read_back_file(false);
    }

    #[test]
    fn test_ffs_file_round_trip() {
        inject_and_read_back_file(true);
    }
}
