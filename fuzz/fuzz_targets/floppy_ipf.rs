//! Fuzz the native (clean-room) IPF chunk parser. Per the July 2026 audit
//! (2.8), several chunk-size fields in `ipf.rs` (up to 4 GB, taken directly
//! from the file) were allocated via `vec![0u8; size as usize]` without
//! validating against the actual remaining file length first.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_floppy::floppy_base::FloppyImageReader;
use m68k_floppy::ipf::NativeIpfBackend;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!(
        "m68k_fuzz_ipf_{:?}.ipf",
        std::thread::current().id()
    ));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    if let Ok(mut backend) = NativeIpfBackend::open(&path) {
        let _ = backend.read_sector(0, 0, 0);
    }
    let _ = std::fs::remove_file(&path);
});
