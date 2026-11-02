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

/// The listing times an instruction without operands: `rts` is not a label.
#[test]
fn listing_counts_cycles_of_bare_instructions() {
    let dir = std::env::temp_dir().join(format!("m68k-listing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listing = dir.join("out.lst");
    let (ok, _, stderr) = run(
        asm(),
        &["-o", "-", "-l", listing.to_str().unwrap(), "-"],
        b"\tnop\n\trts\n",
    );
    assert!(ok, "{stderr}");
    let text = std::fs::read_to_string(&listing).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(text.contains("; 68000 cycles: 20 min, 20 max"), "{text}");
}

#[test]
fn pad_and_megadrive_checksum_finish_a_rom_image() {
    // A vector table and header of zeros, then two words of code.
    let source = "\tdcb.b\t$200,0\n\tmove.w\t#1,d0\n\trts\n";
    let (ok, rom, stderr) = run(
        asm(),
        &["-o", "-", "--pad", "pow2", "--checksum", "megadrive", "-"],
        source.as_bytes(),
    );
    assert!(ok, "{stderr}");
    assert_eq!(rom.len(), 0x400, "padded to the next power of two");
    assert!(rom[0x206..].iter().all(|&b| b == 0xFF), "filled with $FF");
    // 303C + 0001 + 4E75 + 0xFD words of FFFF (each -1) mod 65536.
    let expected = 0x303Cu16
        .wrapping_add(0x0001)
        .wrapping_add(0x4E75)
        .wrapping_sub(0xFD);
    assert_eq!(&rom[0x18E..0x190], &expected.to_be_bytes(), "{stderr}");

    let (ok, _, stderr) = run(
        asm(),
        &["-o", "-", "--pad", "$10", "--fill", "0", "-"],
        SOURCE.as_bytes(),
    );
    assert!(ok, "{stderr}");
    let (ok, _, stderr) = run(
        asm(),
        &["-o", "-", "-f", "srecord", "--pad", "$10", "-"],
        SOURCE.as_bytes(),
    );
    assert!(!ok && stderr.contains("binary output only"), "{stderr}");
}

#[test]
fn disasm_exits_cleanly_when_the_reader_closes_the_pipe() {
    // `m68k-disasm image | head` closes stdout long before the listing
    // ends. The tool used to panic on the resulting EPIPE (exit 101).
    let mut child = Command::new(disasm())
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn binary");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&[0u8; 1 << 17])
        .expect("failed to write stdin");
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("failed to wait for binary");
    assert!(
        out.status.success(),
        "exit {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}
