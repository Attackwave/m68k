//! `m68k-disasm --project`: analysis notes from a file.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn scratch(name: &str, content: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("m68k-project-file-{}-{}", std::process::id(), name));
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
        Ok(String::from_utf8(out.stdout).unwrap())
    } else {
        Err(String::from_utf8(out.stderr).unwrap())
    }
}

/// `jmp (a0)` at $1000 ends static tracing; the routine at $1004 is only
/// reached through the project's `entry` (the filler before it still
/// decodes linearly, as unreached bytes do), and the word at $1008 is
/// declared data rather than decoded.
const IMAGE: &[u8] = &[
    0x4E, 0xD0, // jmp (a0)
    0x4E, 0x71, // nop (unreached filler)
    0x4E, 0x71, // nop  <- entry
    0x4E, 0x75, // rts
    0x4E, 0x71, // "nop" that is declared data
    0x00, 0x00, 0x10, 0x04, // pointer to the entry
];

#[test]
fn project_directives_drive_the_disassembly() {
    let project = scratch(
        "notes",
        "; notes\nentry $1004\ndata $1008 $100a\npointer $100a\nlabel $1004 handler\ncomment $1006 back\n",
    );
    let out = disassemble(
        &["-a", "$1000", "--project", project.to_str().unwrap()],
        IMAGE,
    )
    .unwrap();
    assert!(out.contains("handler:\n00001004:  nop"), "{out}");
    assert!(out.contains("rts") && out.contains("; back"), "{out}");
    assert!(out.contains("00001008:  dc.w     $4e71"), "{out}");
    assert!(out.contains("dc.l     handler"), "{out}");
}

#[test]
fn a_bad_project_line_is_reported_with_its_number() {
    let project = scratch("bad", "entry $1004\ndata $1008\n");
    let err = disassemble(&["--project", project.to_str().unwrap()], IMAGE).unwrap_err();
    assert!(err.contains(":2:") && err.contains("data $1008"), "{err}");
}

#[test]
fn trace_only_needs_a_trace() {
    let err = disassemble(&["--trace-only"], IMAGE).unwrap_err();
    assert!(err.contains("--pc-trace"), "{err}");
}
