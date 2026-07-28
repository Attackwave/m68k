//! Fuzz the Amiga Hunk executable reader on arbitrary byte input. This
//! format's headers directly drive allocation sizes (hunk count, hunk
//! sizes) read straight from the file, the same class of bug already
//! fixed once for m68k-floppy (adf.rs underflow, mfm.rs OOB read,
//! commit 2d9b812) and flagged as an open DoS risk for amiga_hunk.rs in
//! the July 2026 audit (2.8).
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_core::amiga_hunk::read_hunk_executable;

fuzz_target!(|data: &[u8]| {
    let _ = read_hunk_executable(data, 0x1000);
});
