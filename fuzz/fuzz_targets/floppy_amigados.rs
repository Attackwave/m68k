//! Fuzz the AmigaDOS (OFS/FFS) filesystem reader and the ADF writer.
//!
//! These two modules had no fuzz coverage at all, which is notable given
//! that both of the July 2026 filesystem bugs lived there: the file
//! header's data-block table is anchored at the end of the block and
//! grows downwards (the reader walked it from index 0, so every
//! extraction returned an empty file), and `amiga_hash` masked to 31 bits
//! instead of 11 (so `--extract` reported "not found" for most files).
//!
//! What makes this module worth fuzzing is that it chases *pointers read
//! out of the image*: hash-table buckets, hash chains, file-header block
//! numbers, the reversed data-block table, and extension-block chains.
//! A corrupt image can point those at anything — including back at
//! themselves, which is an infinite loop rather than a panic, so the walk
//! depth and block-count bounds matter as much as the indexing does.
//!
//! A purely random byte array would be rejected by `mount`'s `DOS`
//! signature check and never reach any of that. So the target builds a
//! structurally valid disk — correct size, `DOS` bootblock, a root block
//! of the right type at the conventional location — and lets the fuzzer
//! corrupt everything *inside* it. That keeps iterations in the code
//! under test instead of bouncing off the front door.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::adf::AdfBackend;
use m68k_floppy::adf_writer;
use m68k_floppy::amigados::AmigaFs;

/// Standard double-density Amiga floppy: 80 tracks x 2 sides x 11 sectors.
const DD_TRACKS: u32 = 80;
const BLOCK_SIZE: usize = 512;
const TOTAL_BLOCKS: usize = (DD_TRACKS as usize) * 2 * 11;
const IMAGE_SIZE: usize = TOTAL_BLOCKS * BLOCK_SIZE;

/// Depth bound for `walk`, matching what the CLI uses.
const MAX_WALK_DEPTH: usize = 64;

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }

    // Start from a real, mountable image so the fuzzer's bytes land in
    // the filesystem structures rather than being rejected outright.
    let ffs = data[0] & 1 != 0;
    let mut image = match if ffs {
        adf_writer::format_empty_ffs_disk("FUZZ")
    } else {
        adf_writer::format_empty_ofs_disk("FUZZ")
    } {
        Ok(img) => img,
        Err(_) => return,
    };
    if image.len() != IMAGE_SIZE {
        return;
    }

    // Splice the fuzz bytes over the block area, leaving the bootblock
    // (blocks 0-1) intact so `mount` still recognises the filesystem.
    // Everything the reader actually chases — root block, hash table,
    // file headers, data-block tables, extension chains — is fair game.
    let body = &data[1..];
    let start = 2 * BLOCK_SIZE;
    for (i, b) in body.iter().enumerate() {
        let pos = start + (i % (IMAGE_SIZE - start));
        image[pos] ^= *b;
    }

    // The root block must stay type-valid or `mount` bails before any of
    // the interesting traversal code runs. Its *contents* (hash table,
    // name, checksum) stay corrupted.
    let root_off = (TOTAL_BLOCKS / 2) * BLOCK_SIZE;
    image[root_off..root_off + 4].copy_from_slice(&2i32.to_be_bytes());
    image[root_off + 0x1FC..root_off + 0x200].copy_from_slice(&1i32.to_be_bytes());

    // Plant plausible headers so the traversal actually goes somewhere.
    //
    // Without this the run plateaus almost immediately: a hash table full
    // of random pointers lands on blocks whose `sec_type` matches neither
    // ST_FILE nor ST_USERDIR, `dir_entry_from_header` returns None, and
    // every chain ends after one hop — so the reversed data-block table
    // and the extension-block chain, the two places the July bugs lived,
    // are never reached. Giving a handful of blocks a valid type lets the
    // fuzzer build real chains between them and mutate what they contain.
    let planted = 24usize;
    for i in 0..planted {
        let blk = 2 + i;
        if blk == TOTAL_BLOCKS / 2 {
            continue;
        }
        let off = blk * BLOCK_SIZE;
        image[off..off + 4].copy_from_slice(&2i32.to_be_bytes()); // T_HEADER
        // Alternate file and directory headers.
        let sec_type: i32 = if i % 2 == 0 { -3 } else { 2 };
        image[off + 0x1FC..off + 0x200].copy_from_slice(&sec_type.to_be_bytes());
        // A short name, so BCPL string reads have something to chew on
        // without the length byte itself being the only interesting input.
        image[off + 0x1B0] = (data[1 + (i % (data.len() - 1))] % 31) + 1;
    }

    // Point the root's first few hash buckets at the planted blocks, so
    // `list_dir` has entries to return on the very first iteration
    // instead of depending on the fuzzer to guess a valid block number.
    for i in 0..8usize {
        let bucket = root_off + 0x018 + i * 4;
        let target = (2 + i) as u32;
        image[bucket..bucket + 4].copy_from_slice(&target.to_be_bytes());
    }

    // The reader takes a path, so round-trip through a temp file.
    let path = std::env::temp_dir().join(format!(
        "m68k_fuzz_amigados_{:?}.adf",
        std::thread::current().id()
    ));
    if std::fs::write(&path, &image).is_err() {
        return;
    }

    if let Ok(mut backend) = AdfBackend::open(&path) {
        if let Ok(mut fs) = AmigaFs::mount(&mut backend, DD_TRACKS) {
            let root = fs.root_block_num();

            // Directory traversal: hash buckets and hash chains.
            if let Ok(entries) = fs.list_dir(root) {
                // Reading a file exercises the reversed data-block table
                // and the extension-block chain.
                for entry in entries.iter().take(8) {
                    if entry.is_dir {
                        let _ = fs.list_dir(entry.block);
                    } else {
                        let _ = fs.read_file(entry.block);
                    }
                    let _ = fs.find_entry(root, &entry.name);
                }
            }

            // Recursive walk: the depth bound is what stops a cyclic
            // directory chain from running forever.
            let _ = fs.walk("", MAX_WALK_DEPTH);

            // Path resolution over attacker-controlled names.
            let _ = fs.resolve_path("C/List");
            let _ = fs.read_file_at_path("Disk.info");
            let _ = fs.list_dir_at_path("C");

            // A block number straight from the fuzz input, to reach
            // read_file with an arbitrary header block.
            let raw_block =
                u32::from_be_bytes([data[0], data[1], data[2], data[3]]) % (TOTAL_BLOCKS as u32);
            let _ = fs.read_file(raw_block);
            let _ = fs.list_dir(raw_block);
        }
    }

    // Writer side: checksum verification and repair over the same
    // corrupted image.
    let _ = adf_writer::verify_bootblock_checksum(&image);
    let _ = adf_writer::fix_bootblock_checksum(&mut image);

    let _ = std::fs::remove_file(&path);
});
