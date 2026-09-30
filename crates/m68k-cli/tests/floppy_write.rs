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
