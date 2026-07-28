//! Fuzz the ADF backend's header/size validation. Panic history in this
//! crate (commit 2d9b812): a sectors-per-track underflow when a file was a
//! valid multiple of the sector size but too small to hold a full image.
//! `AdfBackend::open` only reads file metadata (size), not file content
//! directly, but takes a path rather than in-memory bytes, so each
//! iteration round-trips the fuzz input through a temp file.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::adf::AdfBackend;
use m68k_floppy::floppy_base::FloppyImageReader;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!(
        "m68k_fuzz_adf_{:?}.adf",
        std::thread::current().id()
    ));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    if let Ok(mut backend) = AdfBackend::open(&path) {
        let _ = backend.read_sector(0, 0, 0);
        let _ = backend.get_bootblock();
    }
    let _ = std::fs::remove_file(&path);
});
