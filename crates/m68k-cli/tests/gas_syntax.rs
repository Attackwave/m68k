//! `--syntax gas`: several input files linked into one binary image.

use std::process::Command;

#[test]
fn several_files_link_into_one_image() {
    let dir = std::env::temp_dir().join(format!("m68k-gas-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.s");
    let b = dir.join("b.s");
    let out = dir.join("out.bin");
    std::fs::write(&a, "\t.text\n\t.globl _start\n_start:\n\tjbsr f\n\trts\n").unwrap();
    std::fs::write(
        &b,
        "\t.text\n\t.globl f\nf:\trts\n\t.data\n\t.reloc_table\n",
    )
    .unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
            .args(args)
            .output()
            .expect("run m68k-asm")
    };
    let result = run(&[
        "--syntax".as_ref(),
        "gas".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
        a.as_os_str(),
        b.as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // jsr f (absolute: f is global), rts, pad, f: rts, pad, table.
    assert_eq!(
        std::fs::read(&out).unwrap(),
        [
            0x4E, 0xB9, 0, 0, 0, 8, 0x4E, 0x75, 0x4E, 0x75, 0, 0, 0, 0, 0, 2, 0
        ]
    );

    let result = run(&[a.as_os_str(), b.as_os_str()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("need --syntax gas"));
    std::fs::remove_dir_all(dir).unwrap();
}
