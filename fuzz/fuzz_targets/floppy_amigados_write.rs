//! Fuzz the AmigaDOS filesystem *writer*.
//!
//! The reader is fuzzed separately (`floppy_amigados`); this target covers
//! the write side, which is riskier in a different way. The reader only
//! follows pointers out of an image. The writer follows them *and* edits
//! them: it walks hash chains to unlink an entry, rewrites `hash_chain`
//! fields in neighbouring blocks, walks extension chains to free a file's
//! blocks, and flips bitmap bits based on block numbers it read out of the
//! very structure being modified.
//!
//! That makes a corrupt volume more dangerous here than on the read side.
//! A hash chain that loops back on itself is an infinite loop; a
//! `high_seq` larger than the table is an out-of-bounds read; a bitmap
//! page pointer aimed past the end of the image is an out-of-bounds write.
//! All three are reachable only by mounting something malformed and then
//! asking the writer to modify it.
//!
//! Random bytes would bounce off `mount`'s signature and root-block
//! checks, so the target formats a real volume first, lets the fuzzer
//! corrupt its interior, and only then drives the write operations. The
//! image buffer's length is never changed, so any out-of-bounds access is
//! a genuine bug rather than an artefact of the harness.

#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::adf_writer;
use m68k_floppy::amigados_write::AmigaFsWriter;

/// Standard double-density Amiga floppy.
const DD_SIZE: usize = 80 * 2 * 11 * 512;

fuzz_target!(|data: &[u8]| {
    // Need a few bytes to steer the operation mix; anything shorter cannot
    // exercise a meaningful sequence.
    if data.len() < 8 {
        return;
    }

    let ffs = data[0] & 1 == 1;
    let mut image = match if ffs {
        adf_writer::format_empty_ffs_disk("FUZZ")
    } else {
        adf_writer::format_empty_ofs_disk("FUZZ")
    } {
        Ok(img) => img,
        Err(_) => return,
    };
    debug_assert_eq!(image.len(), DD_SIZE);

    // Corrupt the volume's interior with the fuzzer's bytes. The bootblock
    // (blocks 0-1) is left alone so `mount` still gets far enough to reach
    // the code under test; everything else — root block, bitmap, hash
    // tables, any structure written later — is fair game.
    let body = &data[1..];
    if !body.is_empty() {
        let start = 2 * 512;
        for (i, &b) in body.iter().enumerate() {
            // Spread the corruption across the image rather than clustering
            // it in the first few blocks.
            let pos = start + (i * 7919) % (image.len() - start);
            image[pos] = b;
        }
    }

    let mut fs = match AmigaFsWriter::mount(&mut image) {
        Ok(fs) => fs,
        // A corrupted root block or bitmap pointer must be rejected, not
        // worked with. That rejection is the intended behaviour.
        Err(_) => return,
    };

    // Drive a sequence of operations chosen by the remaining bytes. Every
    // one may legitimately fail on a corrupt volume; none may panic, hang,
    // or read/write outside the image.
    let mut names = [
        String::from("A"),
        String::from("B"),
        String::from("DIR"),
        String::from("DIR/C"),
    ];
    for (step, &op) in data.iter().enumerate().take(64) {
        let name = &names[(op as usize >> 3) % names.len()];
        match op % 8 {
            0 => {
                let size = (op as usize) * 37 % 4096;
                let _ = fs.write_file(name, &vec![op; size]);
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
                let other = &names[(step + 1) % names.len()].clone();
                let _ = fs.rename(name, other);
            }
            5 => {
                let _ = fs.set_comment(name, "fuzz");
            }
            6 => {
                let _ = fs.set_protection(name, op as u32);
            }
            _ => {
                let _ = fs.resolve(name);
                let _ = fs.free_blocks();
            }
        }
    }

    // Mutating the name set exercises hash buckets the fixed names miss.
    names[0] = format!("N{}", data[1] as u32);
    let _ = fs.write_file(&names[0].clone(), b"x");
});
