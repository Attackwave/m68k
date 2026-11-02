//! `m68k-disasm --system`: the example programs, assembled against the
//! shipped include files, read back with the names those files define.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assemble(example: &str) -> Vec<u8> {
    let out = Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
        .args(["-I", "include", "-f", "binary", "-o", "-", example])
        .current_dir(root())
        .output()
        .expect("failed to run m68k-asm");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn disassemble(args: &[&str], image: &[u8]) -> String {
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn megadrive_cartridge_reads_with_vdp_names_vectors_and_header() {
    let rom = assemble("examples/megadrive/hello.s");
    let listing = disassemble(&["--system", "megadrive"], &rom);
    assert!(listing.contains("VDP_CTRL"), "{listing}");
    assert!(listing.contains("VDP_DATA"), "{listing}");
    // The reset vector is a pointer to the code it names, not an opword.
    assert!(listing.contains("00000004:  dc.l     label"), "{listing}");
    // The header is data with its fields labelled.
    assert!(listing.contains("HEADER_CHECKSUM:"), "{listing}");
    assert!(listing.contains("00000100:  dc.w"), "{listing}");
    // Without the system nothing of that is known.
    let plain = disassemble(&[], &rom);
    assert!(!plain.contains("VDP_CTRL"));
}

#[test]
fn neogeo_cartridge_reads_with_register_names_and_header_entries() {
    let rom = assemble("examples/neogeo/hello.s");
    let listing = disassemble(&["--system", "neogeo"], &rom);
    assert!(listing.contains("REG_"), "{listing}");
    assert!(listing.contains("BIOS_"), "{listing}");
    // The USER entry in the header is a JMP, decoded as code.
    assert!(listing.contains("00000122:  jmp"), "{listing}");
}

#[test]
fn atarist_program_names_its_trap_calls() {
    let program = assemble("examples/atarist/hello.s");
    let listing = disassemble(&["--system", "atarist", "-a", "$20000"], &program);
    assert!(listing.contains("; Cconws"), "{listing}");
    assert!(listing.contains("; Vsync"), "{listing}");
    assert!(listing.contains("; Pterm0"), "{listing}");
}
