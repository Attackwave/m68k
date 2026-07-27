//! Fuzz the UAE extended ADF backend (raw per-track MFM bitstream format).
//! Per the July 2026 audit (2.8), `uae.rs`'s track-size field was
//! previously uncapped (unlike `num_entries`, which was already bounded)
//! before being used to size an allocation read from the file header.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::floppy_base::FloppyImageReader;
use m68k_floppy::uae::UaeExtendedBackend;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!(
        "m68k_fuzz_uae_{:?}.adf",
        std::thread::current().id()
    ));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    if let Ok(mut backend) = UaeExtendedBackend::open(&path) {
        let _ = backend.read_sector(0, 0, 0);
    }
    let _ = std::fs::remove_file(&path);
});
