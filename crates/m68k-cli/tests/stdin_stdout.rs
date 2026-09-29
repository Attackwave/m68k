//! Integration tests for the `-` (stdin/stdout) convention documented in the
//! README for `m68k-asm` and `m68k-disasm`.
//!
//! These drive the real binaries via `CARGO_BIN_EXE_*`, because the behaviour
//! under test lives entirely in the CLI wrappers — the library types never see
//! a path. Both tools previously aborted with `cannot read '-'` despite the
//! README promising stdin support.

use std::io::Write;
use std::process::{Command, Stdio};

const SOURCE: &str = "\tmove.w\t#1,d0\n\trts\n";
/// `MOVE.W #1,D0` + `RTS`, as produced by the reference assembler.
const EXPECTED_BYTES: &[u8] = &[0x30, 0x3c, 0x00, 0x01, 0x4e, 0x75];

/// Runs a binary with `stdin_data` piped in, returning (status ok, stdout, stderr).
fn run(bin: &str, args: &[&str], stdin_data: &[u8]) -> (bool, Vec<u8>, String) {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn binary");
    child
        .stdin
        .as_mut()
        .expect("stdin was piped")
        .write_all(stdin_data)
        .expect("failed to write stdin");
    let out = child.wait_with_output().expect("failed to wait for binary");
    (
        out.status.success(),
        out.stdout,
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn asm() -> &'static str {
    env!("CARGO_BIN_EXE_m68k-asm")
}

fn disasm() -> &'static str {
    env!("CARGO_BIN_EXE_m68k-disasm")
}

#[test]
fn asm_reads_stdin_and_writes_stdout() {
    let (ok, stdout, stderr) = run(asm(), &["-", "-o", "-"], SOURCE.as_bytes());
    assert!(ok, "assembler failed: {}", stderr);
    assert_eq!(stdout, EXPECTED_BYTES);
}

#[test]
fn asm_stdin_matches_file_input() {
    // Name is unique per process and per test binary, so a concurrent
    // `cargo test` run (or a recycled PID) cannot collide here.
    let dir = std::env::temp_dir().join(format!(
        "m68k-cli-stdin-{}-{}",
        std::process::id(),
        env!("CARGO_CRATE_NAME")
    ));
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let src = dir.join("t.s");
    let out = dir.join("t.bin");
    std::fs::write(&src, SOURCE).expect("failed to write source");

    let (ok, _, stderr) = run(
        asm(),
        &[
            src.to_str().expect("temp path is utf-8"),
            "-o",
            out.to_str().expect("temp path is utf-8"),
        ],
        b"",
    );
    assert!(ok, "assembler failed on file input: {}", stderr);
    let from_file = std::fs::read(&out).expect("failed to read output");

    let (ok, from_stdin, stderr) = run(asm(), &["-", "-o", "-"], SOURCE.as_bytes());
    assert!(ok, "assembler failed on stdin input: {}", stderr);

    assert_eq!(
        from_file, from_stdin,
        "stdin path must produce byte-identical output to the file path"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asm_stdin_without_output_is_an_error() {
    // There is no input filename to derive a default output path from, so the
    // tool must say so rather than write to a file named after `-`.
    let (ok, _, stderr) = run(asm(), &["-"], SOURCE.as_bytes());
    assert!(!ok, "expected failure when -o is omitted with stdin input");
    assert!(
        stderr.contains("requires an explicit -o"),
        "unexpected error message: {}",
        stderr
    );
}

#[test]
fn asm_writes_text_formats_to_stdout() {
    // The non-binary formats used to bypass the stdout path entirely and
    // always `fs::write`, creating a file literally named `-`.
    let (ok, stdout, stderr) = run(asm(), &["-", "-f", "srecord", "-o", "-"], SOURCE.as_bytes());
    assert!(ok, "assembler failed: {}", stderr);
    let text = String::from_utf8(stdout).expect("s-record output is text");
    assert!(text.starts_with("S0"), "unexpected s-record: {}", text);
    assert!(
        text.contains("303C00014E75"),
        "s-record lacks the encoded instructions: {}",
        text
    );
    assert!(
        !std::path::Path::new("-").exists(),
        "output was written to a file named '-' instead of stdout"
    );
}

#[test]
fn disasm_reads_stdin() {
    let (ok, stdout, stderr) = run(disasm(), &["-"], EXPECTED_BYTES);
    assert!(ok, "disassembler failed: {}", stderr);
    let text = String::from_utf8(stdout).expect("disassembly is text");
    assert!(
        text.contains("move.w") && text.contains("rts"),
        "unexpected disassembly: {}",
        text
    );
}

#[test]
fn asm_to_disasm_pipeline_roundtrips() {
    let (ok, bytes, stderr) = run(asm(), &["-", "-o", "-"], SOURCE.as_bytes());
    assert!(ok, "assembler failed: {}", stderr);
    let (ok, stdout, stderr) = run(disasm(), &["-"], &bytes);
    assert!(ok, "disassembler failed: {}", stderr);
    let text = String::from_utf8(stdout).expect("disassembly is text");
    assert!(
        text.contains("move.w") && text.contains("rts"),
        "piped roundtrip lost instructions: {}",
        text
    );
}

#[test]
fn disasm_empty_stdin_is_an_error() {
    let (ok, _, stderr) = run(disasm(), &["-"], b"");
    assert!(!ok, "expected failure on empty stdin");
    assert!(
        stderr.contains("empty input"),
        "unexpected error message: {}",
        stderr
    );
}

/// Symbols differing only in case sorted equal, so their order came from
/// hash-map iteration and the file changed between identical runs.
#[test]
fn symbol_table_is_deterministic() {
    let dir = std::env::temp_dir().join(format!("m68k-sym-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("s.s");
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!("ERR_{i} EQU {i}\nerr_{i}: nop\n"));
    }
    std::fs::write(&src, text).unwrap();

    let tables: Vec<String> = (0..5)
        .map(|n| {
            let sym = dir.join(format!("{n}.sym"));
            let out = Command::new(asm())
                .args([
                    src.to_str().unwrap(),
                    "-o",
                    dir.join("s.bin").to_str().unwrap(),
                ])
                .args(["--sym", sym.to_str().unwrap()])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::fs::read_to_string(sym).unwrap()
        })
        .collect();
    assert!(
        tables.windows(2).all(|w| w[0] == w[1]),
        "symbol table differs between runs"
    );
    // Exact-name tie break: upper case first.
    let first = tables[0]
        .lines()
        .find(|l| l.to_lowercase().starts_with("err_0 "))
        .unwrap();
    assert!(first.starts_with("ERR_0 "), "{first}");
    std::fs::remove_dir_all(&dir).ok();
}
