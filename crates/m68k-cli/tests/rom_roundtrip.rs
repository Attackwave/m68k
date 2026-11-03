//! A real Kickstart ROM, disassembled as source and assembled again, must
//! come back byte for byte. This is the oracle for both tools at once:
//! every line the disassembler emits has to be something the assembler
//! accepts, and has to assemble to the bytes it was decoded from.
//!
//! Ignored by default because a ROM is needed:
//! `M68K_ROM=tmp/<rom> cargo test -p m68k-cli --test rom_roundtrip -- --ignored --nocapture`

use std::process::Command;

#[test]
#[ignore]
fn kickstart_reassembles_byte_for_byte() {
    let Ok(path) = std::env::var("M68K_ROM") else {
        eprintln!("M68K_ROM not set, skipping");
        return;
    };
    let mut rom = std::fs::read(&path).expect("read ROM");
    // Some dumps carry a short trailer; the ROM proper is a power of two.
    let (origin, size) = if rom.len() >= 512 * 1024 {
        ("$f80000", 512 * 1024)
    } else {
        ("$fc0000", 256 * 1024)
    };
    rom.truncate(size);
    let reset = format!("${:x}", u32::from_be_bytes(rom[4..8].try_into().unwrap()));

    let disasm = Command::new(env!("CARGO_BIN_EXE_m68k-disasm"))
        .args([
            "--source",
            "--scan-tables",
            "-a",
            origin,
            "-e",
            &reset,
            &path,
        ])
        .output()
        .expect("run m68k-disasm");
    assert!(
        disasm.status.success(),
        "{}",
        String::from_utf8_lossy(&disasm.stderr)
    );

    let dir = std::env::temp_dir().join(format!("m68k-rom-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("rom.s");
    let binary = dir.join("rom.bin");
    std::fs::write(&source, &disasm.stdout).unwrap();

    let asm = Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
        .args(["-c", "68000", "-o"])
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("run m68k-asm");
    assert!(
        asm.status.success(),
        "{}",
        String::from_utf8_lossy(&asm.stderr)
    );

    let rebuilt = std::fs::read(&binary).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let first_diff = rom.iter().zip(&rebuilt).position(|(a, b)| a != b);
    eprintln!(
        "{path}: {} source lines, {} bytes back",
        disasm.stdout.iter().filter(|&&b| b == b'\n').count(),
        rebuilt.len()
    );
    assert_eq!(rebuilt.len(), rom.len(), "size differs");
    assert_eq!(
        first_diff,
        None,
        "first differing byte at offset {:#x}",
        first_diff.unwrap_or(0)
    );
}
