//! `m68k-disasm --symbols` and `--equates`: names from a symbol file and
//! from an assembler include file.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn scratch(name: &str, content: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("m68k-symbol-files-{}-{}", std::process::id(), name));
    std::fs::write(&path, content).unwrap();
    path
}

fn disassemble(args: &[&str], image: &[u8]) -> Result<String, String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_m68k-disasm"))
        .args(args)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run m68k-disasm");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(image)
        .expect("failed to write stdin");
    let out = child.wait_with_output().unwrap();
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `jsr $1000; rts` at address 0.
const IMAGE: &[u8] = &[0x4E, 0xB9, 0x00, 0x00, 0x10, 0x00, 0x4E, 0x75];

#[test]
fn symbol_file_names_targets_in_both_line_forms() {
    let file = scratch(
        "syms.txt",
        "# a comment\n$1000 draw\n; another\nexit = $000006\n",
    );
    let listing = disassemble(&["--symbols", file.to_str().unwrap()], IMAGE).unwrap();
    assert!(listing.contains("jsr     draw"), "{listing}");
    assert!(listing.contains("exit:"), "{listing}");
}

#[test]
fn a_malformed_symbol_line_is_reported_with_its_number() {
    let file = scratch("bad.txt", "$1000 draw\nnot a symbol line\n");
    let err = disassemble(&["--symbols", file.to_str().unwrap()], IMAGE).unwrap_err();
    assert!(err.contains(":2: expected 'address name'"), "{err}");
}

#[test]
fn equates_name_addresses_outside_the_image_only() {
    // move.b ($fa01).w,d0 — the short form sign-extends to $fffffa01,
    // which on a 24-bit machine is the equate's $fffa01.
    let image = [0x10, 0x38, 0xFA, 0x01, 0x4E, 0x75];
    let file = scratch(
        "hw.i",
        "MFP_GPIP\tEQU\t$FFFA01\nREG_NUMBER\tEQU\t4\nTEXT\tEQU\t0\n",
    );
    let listing = disassemble(&["--equates", file.to_str().unwrap()], &image).unwrap();
    assert!(listing.contains("MFP_GPIP.w"), "{listing}");
    // Values inside the image (0 and 4) do not become labels.
    assert!(!listing.contains("REG_NUMBER"), "{listing}");
    assert!(!listing.contains("TEXT"), "{listing}");
}
