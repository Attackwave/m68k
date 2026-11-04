//! The Amiga example, assembled as a Hunk executable, must load and run
//! under vamos (amitools' AmigaOS emulation on a 68k core) and print its
//! line. This exercises the Hunk writer against a real LoadSeg()
//! implementation, including the relocation of the string address.
//!
//! Ignored by default because vamos must be installed:
//! `cargo test -p m68k-cli --test vamos_examples -- --ignored --nocapture`

use std::process::Command;

#[test]
#[ignore]
fn amiga_hello_runs_under_vamos() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("m68k-vamos-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("hello");

    let asm = Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
        .args(["-f", "hunk-exe", "-o"])
        .arg(&exe)
        .arg(root.join("examples/amiga/hello.s"))
        .output()
        .expect("run m68k-asm");
    assert!(
        asm.status.success(),
        "{}",
        String::from_utf8_lossy(&asm.stderr)
    );

    let run = match Command::new("vamos").arg("-q").arg(&exe).output() {
        Ok(run) => run,
        Err(e) => {
            eprintln!("vamos not runnable ({e}), skipping");
            return;
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "vamos exited with {:?}:\n{}{}",
        run.status.code(),
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("Hello from m68k-asm"),
        "output was: {stdout:?}"
    );
}
