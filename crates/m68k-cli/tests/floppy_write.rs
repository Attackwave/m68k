//! Write options of `m68k-floppy` can be repeated; every occurrence is
//! applied. The pair options used to keep only their first occurrence.

use std::process::Command;

#[test]
fn every_repeated_write_option_is_applied() {
    let dir = std::env::temp_dir().join(format!("m68k-floppy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b, adf) = (dir.join("a.txt"), dir.join("b.txt"), dir.join("t.adf"));
    std::fs::write(&a, "aaa").unwrap();
    std::fs::write(&b, "bbb").unwrap();
    let floppy = env!("CARGO_BIN_EXE_m68k-floppy");
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();

    let out = Command::new(floppy)
        .arg(&adf)
        .args(["--format", "TEST", "--make-dir", "S", "--make-dir", "T"])
        .args([
            "--add-file",
            &s(&a),
            "a.txt",
            "--add-file",
            &s(&b),
            "S/b.txt",
        ])
        .args(["--add-file", &s(&a), "c.txt", "--rename", "c.txt", "d.txt"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let list = Command::new(floppy)
        .arg(&adf)
        .arg("--list-all")
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&list.stdout);
    for entry in ["S/", "T/", "S/b.txt", "a.txt", "d.txt"] {
        assert!(
            list.lines().any(|l| l.trim_end().ends_with(entry)),
            "{entry} missing:\n{list}"
        );
    }
    assert!(!list.contains("c.txt"), "{list}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A `.st` image is formatted as FAT12 and written through the same
/// options; what was added is listed and extracted byte for byte.
#[test]
fn st_image_is_formatted_as_fat12_and_round_trips_a_file() {
    let dir = std::env::temp_dir().join(format!("m68k-floppy-st-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (src, st, out) = (dir.join("hello.prg"), dir.join("t.st"), dir.join("out.prg"));
    let data: Vec<u8> = (0..3000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let floppy = env!("CARGO_BIN_EXE_m68k-floppy");
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();

    let run = |args: &[&str]| {
        let out = Command::new(floppy).arg(&st).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    run(&[
        "--format",
        "DISK",
        "--make-dir",
        "AUTO",
        "--add-file",
        &s(&src),
        "AUTO/HELLO.PRG",
    ]);
    assert_eq!(std::fs::metadata(&st).unwrap().len(), 80 * 2 * 9 * 512);
    run(&[
        "--add-file",
        &s(&src),
        "readme.txt",
        "--rename",
        "readme.txt",
        "notes.txt",
    ]);
    let list = run(&["--list-all"]);
    for entry in ["AUTO/", "AUTO/HELLO.PRG", "NOTES.TXT"] {
        assert!(
            list.lines().any(|l| l.trim_end().ends_with(entry)),
            "{entry} missing:\n{list}"
        );
    }
    run(&["--extract", "auto/hello.prg", "-o", &s(&out)]);
    assert_eq!(std::fs::read(&out).unwrap(), data);

    let err = Command::new(floppy)
        .arg(&st)
        .args(["--set-comment", "NOTES.TXT", "x"])
        .output()
        .unwrap();
    assert!(!err.status.success());
    std::fs::remove_dir_all(&dir).ok();
}
