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

#[test]
fn sym_and_map_describe_the_linked_image() {
    let dir = std::env::temp_dir().join(format!("m68k-gas-sym-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.s");
    let b = dir.join("b.s");
    let (out, sym, map) = (dir.join("o.bin"), dir.join("o.sym"), dir.join("o.map"));
    // `s` is static in both files, `.L1` is GCC's own, `c` a common.
    std::fs::write(
        &a,
        "\t.text\n\t.globl _start\n_start:\tjbsr f\n.L1:\trts\ns:\trts\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        "\t.text\n\t.globl f\nf:\trts\ns:\tnop\n\t.data\nd:\t.long 0\n\t.comm c,4,2\n",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
        .args(["--syntax", "gas", "--sym"])
        .arg(&sym)
        .arg("--map")
        .arg(&map)
        .arg("-o")
        .arg(&out)
        .args([&a, &b])
        .output()
        .expect("run m68k-asm");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let sym = std::fs::read_to_string(&sym).unwrap();
    let rows: Vec<&str> = sym.lines().filter(|l| l.contains(" = $")).collect();
    let expected = [
        ("__bss_end", 0x18),
        ("__bss_start", 0x14),
        ("__image_end", 0x14),
        ("_start", 0),
        ("a.s:s", 8),
        ("b.s:s", 0xE),
        ("c", 0x14),
        ("d", 0x10),
        ("f", 0xC),
    ]
    .map(|(n, v)| format!("{n:<20} = ${v:06X}"));
    assert_eq!(rows, expected, "{sym}");
    let map = std::fs::read_to_string(&map).unwrap();
    let rows: Vec<&str> = map.lines().filter(|l| l.starts_with('$')).collect();
    assert_eq!(rows.len(), 4, "{map}");
    for (row, (range, size, name)) in rows.iter().zip([
        ("$000000-$000009", 10, "a.s  .text"),
        ("$00000C-$00000F", 4, "b.s  .text"),
        ("$000010-$000013", 4, "b.s  .data"),
        ("$000014-$000017", 4, "-  COMMON"),
    ]) {
        assert!(row.starts_with(range), "{row}");
        assert!(row.contains(&format!("{size} bytes")), "{row}");
        assert!(row.ends_with(name), "{row}");
    }
    assert!(
        map.contains("Image end $000014, BSS $000014-$000018"),
        "{map}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
