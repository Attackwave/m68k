//! Fuzz the FAT12 reader and writer together.
//!
//! The reader follows cluster chains and directory regions out of the
//! image; the writer follows them and edits them, allocating clusters by
//! what it read from the FAT and extending directories by what it read
//! from their chains. A looping chain, a cluster number past the FAT or
//! a directory entry whose offset lies beyond its chain must all be
//! rejected, never read or written out of bounds.
//!
//! Random bytes bounce off the boot sector check, so the target formats a
//! real disk, lets the fuzzer corrupt the FATs, the root directory and
//! the first data clusters, and drives both sides over the result. The
//! image length never changes, so an out-of-bounds access is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::fat12::Fat12;
use m68k_floppy::fat12_write::{Fat12Writer, format_empty_st_disk};
use m68k_floppy::st::StBackend;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let sides = 1 + (data[0] & 1) as u32;
    let mut image = match format_empty_st_disk("FUZZ", sides) {
        Ok(img) => img,
        Err(_) => return,
    };
    let len = image.len();
    // Half the inputs also corrupt the parameter block, so impossible
    // layouts reach `mount`; the rest keep it and reach the chains.
    let start = if data[0] & 2 == 0 { 512 } else { 11 };
    for (i, &b) in data[1..].iter().enumerate() {
        let pos = start + (i * 7919) % (len - start);
        image[pos] = b;
    }

    if let Ok(mut st) = StBackend::from_image(image.clone())
        && let Ok(mut fs) = Fat12::mount(&mut st)
        && let Ok(entries) = fs.walk("", 64)
    {
        for (path, entry) in entries {
            let _ = fs.read_file(&entry);
            let _ = fs.read_file_at_path(&path);
        }
    }

    let mut fs = match Fat12Writer::mount(&mut image) {
        Ok(fs) => fs,
        Err(_) => return,
    };
    let names = ["A", "B.BIN", "DIR", "DIR/C", "DIR/SUB", "DIR/SUB/D"];
    for (step, &op) in data.iter().enumerate().take(64) {
        let name = names[(op as usize >> 3) % names.len()];
        match op % 6 {
            0 => {
                let _ = fs.write_file(name, &vec![op; (op as usize) * 53 % 6000]);
            }
            1 => {
                let _ = fs.delete_file(name);
            }
            2 => {
                let _ = fs.create_dir(name);
            }
            3 => {
                let _ = fs.delete_dir(name);
            }
            4 => {
                let _ = fs.rename(name, names[(step + 1) % names.len()]);
            }
            _ => {
                let _ = fs.list_dir(name);
                let _ = fs.free_clusters();
            }
        }
    }
    assert_eq!(fs.into_image().len(), len);
});
