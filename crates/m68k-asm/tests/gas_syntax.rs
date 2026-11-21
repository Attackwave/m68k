//! GNU as syntax: several files linked into one flat image.
//!
//! The fixture sets under `tests/gas/fixtures/` hold the image, fixups and
//! link symbols that GNU as + ld produced for the same sources, for the
//! CPU in the set's `cpu.txt` (68000 without one)
//! (`tests/gas/gnu_reference.py --regen-fixtures`); this test needs no
//! GNU tools. The tests after them cover what GNU as cannot check
//! because it assembles one object file at a time.

use std::fs;
use std::path::{Path, PathBuf};

use m68k_asm::gas::{GasError, GasImage, SourceFile, assemble};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/gas/fixtures")
}

fn link(sources: &[(&str, &str)]) -> Result<GasImage, Vec<GasError>> {
    let files: Vec<SourceFile> = sources
        .iter()
        .map(|(name, text)| SourceFile {
            name: name.to_string(),
            text: text.to_string(),
        })
        .collect();
    assemble(&files, "68000", 0)
}

fn ok(sources: &[(&str, &str)]) -> GasImage {
    link(sources).unwrap_or_else(|e| panic!("{e:?}"))
}

fn errors(sources: &[(&str, &str)]) -> Vec<String> {
    link(sources)
        .expect_err("must not assemble")
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The fixups a `.reloc_table` at the end of `image` lists.
fn decode_table(table: &[u8]) -> Vec<u32> {
    let first = u32::from_be_bytes(table[..4].try_into().unwrap());
    if first == 0 {
        assert_eq!(table.len(), 4);
        return Vec::new();
    }
    let (mut out, mut at) = (vec![first], first);
    for &b in &table[4..table.len() - 1] {
        match b {
            1 => at += 254,
            n => {
                at += u32::from(n);
                out.push(at);
            }
        }
    }
    assert_eq!(table.last(), Some(&0));
    out
}

#[test]
fn fixtures_match_gnu_as_and_ld() {
    let mut sets = 0;
    for dir in fs::read_dir(fixtures()).unwrap() {
        let dir = dir.unwrap().path();
        let Ok(list) = fs::read_to_string(dir.join("files.txt")) else {
            continue;
        };
        let files: Vec<SourceFile> = list
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|name| SourceFile {
                name: name.to_string(),
                text: fs::read_to_string(dir.join(name)).unwrap(),
            })
            .collect();
        let cpu = fs::read_to_string(dir.join("cpu.txt")).unwrap_or_else(|_| "68000".into());
        let image = assemble(&files, cpu.trim(), 0).unwrap_or_else(|e| panic!("{dir:?}: {e:?}"));
        let expected = dir.join("expected");
        let bytes = fs::read(expected.join("image.bin")).unwrap();
        assert!(
            image.image == bytes,
            "{dir:?}: image differs from GNU as + ld"
        );
        let fixups: Vec<u32> = fs::read_to_string(expected.join("fixups.txt"))
            .unwrap()
            .split_whitespace()
            .map(|x| u32::from_str_radix(x, 16).unwrap())
            .collect();
        assert_eq!(image.fixups, fixups, "{dir:?}");
        let symbols = fs::read_to_string(expected.join("symbols.txt")).unwrap();
        let value = |name: &str| {
            let line = symbols.lines().find(|l| l.starts_with(name)).unwrap();
            u32::from_str_radix(line.split_whitespace().nth(1).unwrap(), 16).unwrap()
        };
        assert_eq!(
            (image.image_end, image.bss_start, image.bss_end),
            (
                value("__image_end"),
                value("__bss_start"),
                value("__bss_end")
            ),
            "{dir:?}"
        );
        sets += 1;
    }
    assert!(sets >= 3, "fixture sets missing");
}

/// What a C start-up file needs: sizes as differences of symbols in
/// different sections, the table behind its label at the very end, BSS
/// right behind the image.
#[test]
fn start_up_code_sizes_and_table() {
    let crt0 = " .text\n .globl _start\n_start:\n .long 0x47505232\n .word 0x18, 0x1D\n\
                .long __image_end - _start\n .long __bss_end - __bss_start\n\
                lea _start(%pc),%a1\n move.l %a1,%a0\n add.l #__reloc_table - _start,%a0\n\
                jsr main\n rts\n";
    let main = "\t.text\n\t.globl main\nmain:\n\tmove.l #buf,%d0\n#APP\n\
                | 1311 \"src/c/sys.h\" 1\n\tjsr 0xF8100C\n| 0 \"\" 2\n#NO_APP\n\trts\n\
                \t.data\nptr:\t.long buf, main\n\t.bss\nbuf:\t.zero 10\n";
    let end = "\t.data\n\t.globl __reloc_table\n__reloc_table:\n\t.reloc_table\n";
    let img = ok(&[("crt0.s", crt0), ("main.s", main), ("end.s", end)]);
    let b = &img.image;
    let long = |at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    assert_eq!(img.bss_start, img.image_end);
    assert_eq!(img.image_end as usize, b.len());
    assert_eq!(long(8), img.image_end);
    assert_eq!(long(12), img.bss_end - img.bss_start);
    // adda.l #(table - _start),%a0 behind lea and move.
    assert_eq!(&b[16..22], &[0x43, 0xFA, 0xFF, 0xEE, 0x20, 0x49]);
    assert_eq!(&b[22..24], &[0xD1, 0xFC]);
    let table_at = long(24) as usize;
    // The numeric jsr is absolute long and never fixed up.
    let jsr = b
        .windows(6)
        .position(|w| w == [0x4E, 0xB9, 0x00, 0xF8, 0x10, 0x0C]);
    let jsr = jsr.expect("jsr 0xF8100C") as u32;
    assert!(!img.fixups.contains(&(jsr + 2)));
    // `jsr main`, `move.l #buf`, and the two data longs; no fixup for the
    // differences.
    assert_eq!(img.fixups.len(), 4);
    assert!(img.fixups.iter().all(|&f| f >= 28));
    assert_eq!(decode_table(&b[table_at..]), img.fixups);
}

#[test]
fn reloc_table_must_end_the_image() {
    let a = "\t.text\n\tmove.l #x,%d0\nx:\trts\n\t.data\n\t.reloc_table\n";
    let b = "\t.data\n\t.long 1\n";
    let e = errors(&[("a.s", a), ("b.s", b)]);
    assert!(
        e[0].starts_with("a.s:5: error: .reloc_table must be the last"),
        "{e:?}"
    );
}

#[test]
fn far_fixups_use_skip_steps() {
    let a = "\t.text\n\tnop\n\t.long x\n\t.zero 600\nx:\t.long x\n\t.data\n\t.reloc_table\n";
    let img = ok(&[("a.s", a)]);
    assert_eq!(img.fixups, vec![2, 606]);
    // 604 bytes apart: two skips of 254, then 96.
    assert_eq!(&img.image[612..], &[0, 0, 0, 2, 1, 1, 96, 0]);
}

#[test]
fn empty_reloc_table_is_one_zero_long() {
    let img = ok(&[("a.s", "\t.text\n\trts\n\t.data\n\t.reloc_table\n")]);
    assert_eq!(&img.image[4..], &[0, 0, 0, 0]);
}

#[test]
fn statics_are_per_file_and_globals_shared() {
    let a = "\t.text\n\t.globl g\ng:\twriter: rts\n\t.long writer\n";
    let b = "\t.text\nwriter:\tnop\n\t.long writer, g\n";
    let img = ok(&[("a.s", a), ("b.s", b)]);
    // a: g/writer at 0, rts, long 0; b at 8: nop, long 8, long 0.
    assert_eq!(
        img.image,
        [
            0x4E, 0x75, 0, 0, 0, 0, 0, 0, 0x4E, 0x71, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0
        ]
    );
    let dup = errors(&[("a.s", a), ("c.s", "\t.globl g\ng:\trts\n")]);
    assert!(
        dup[0].contains("c.s:2: error: symbol 'g' is already defined"),
        "{dup:?}"
    );
}

#[test]
fn global_commons_are_merged_and_follow_bss_in_order_of_appearance() {
    let a =
        "\t.comm big,2,2\n\t.comm second,4,4\n\t.bss\n\t.zero 3\n\t.data\n\t.long big, second\n";
    let b = "\t.comm big,8,4\n";
    let img = ok(&[("a.s", a), ("b.s", b)]);
    let long = |at: usize| u32::from_be_bytes(img.image[at..at + 4].try_into().unwrap());
    // .text (empty) and .data of a at 0, .text/.data of b aligned at 8.
    assert_eq!(img.image_end, 8);
    // BSS: a's 3 bytes at 8, b's empty .bss at 12, then big (8 bytes,
    // 4-aligned) and second.
    assert_eq!((long(0), long(4)), (12, 20));
    assert_eq!(img.bss_end, 24);
}

#[test]
fn errors_name_file_and_line() {
    let e = errors(&[(
        "a.s",
        "\t.text\n\tmove.w #x,%d0\nx:\t.word x\n\t.byte 1\n\t.long x\n\t.uleb128 5\n\tjsr nowhere\n",
    )]);
    let want = ["a.s:6: error: unsupported directive '.uleb128'"];
    assert_eq!(e, want);
    let e = errors(&[(
        "a.s",
        "\t.text\n\tmove.w #x,%d0\nx:\t.word x\n\t.word 1\n\t.long x\n\tjsr nowhere\n",
    )]);
    assert!(
        e.iter()
            .any(|m| m.starts_with("a.s:2: error: an address in a field narrower")),
        "{e:?}"
    );
    assert!(
        e.iter()
            .any(|m| m.starts_with("a.s:3: error: an address in a 16-bit field")),
        "{e:?}"
    );
    assert!(
        e.iter()
            .any(|m| m == "a.s:6: error: undefined symbol 'nowhere'"),
        "{e:?}"
    );
    let odd = errors(&[("a.s", "\t.data\n\t.byte 1\n\t.long odd\nodd:\n")]);
    assert_eq!(
        odd,
        ["a.s:3: error: an address at the odd offset $1: a fixup must be at an even offset"]
    );
}

#[test]
fn unsupported_input_is_refused() {
    for (src, what) in [
        (
            "\t.section .init_array,\"aw\"\n",
            "section '.init_array' is not supported",
        ),
        (
            "\t.section .rodata.cst4,\"aM\",@progbits,4\n",
            "only string merging",
        ),
        (
            "\t.pushsection .text\n",
            "unsupported directive '.pushsection'",
        ),
        ("\t.text\n\tbra.l x\nx:\n", "needs a 68020"),
        ("\t.text\n\tfoo %d0\n", "unknown mnemonic"),
    ] {
        let e = errors(&[("a.s", src)]);
        assert!(e.iter().any(|m| m.contains(what)), "{src:?}: {e:?}");
    }
    let at = |cpu: &str, text: &str| {
        assemble(
            &[SourceFile {
                name: "a.s".into(),
                text: text.into(),
            }],
            cpu,
            0,
        )
    };
    assert!(at("68020", "\trts\n").is_ok());
    assert!(at("68888", "\trts\n").is_err());
    // 68020 forms on a 68000.
    for text in [
        "\tlea x(%d0.l*4),%a0\nx:\n",
        "\tlea 1000(%a0,%d0.l),%a0\n",
        "\tmove.l 100000(%a0),%d0\n",
        "\tmuls.l %d1,%d2:%d3\n",
        "\tbfextu %d0{#3:#5},%d1\n",
    ] {
        assert!(at("68000", text).is_err(), "{text:?}");
        assert!(at("68020", text).is_ok(), "{text:?}");
    }
}

#[test]
fn debug_sections_and_cfi_are_skipped() {
    let src = "\t.file 1 \"x.c\"\n\t.text\n.Ltext0:\n\t.cfi_startproc\n\t.loc 1 3 1\n\trts\n\
               \t.cfi_endproc\n\t.section .debug_info,\"\",@progbits\n.Ldebug_info0:\n\
               \t.long 0x1234\n\t.uleb128 0x5\n\t.long .Ltext0\n\t.text\n\tnop\n";
    assert_eq!(ok(&[("a.s", src)]).image, [0x4E, 0x75, 0x4E, 0x71]);
}

#[test]
fn fpu_code_is_checked() {
    let at = |cpu: &str, text: &str| {
        assemble(
            &[SourceFile {
                name: "a.s".into(),
                text: format!("\t.text\n{text}"),
            }],
            cpu,
            0,
        )
        .map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>())
    };
    // No FPU on a 68000; the rounding-precision forms are the 68040's.
    assert!(at("68000", "\tfmove.s #0x3f800000,%fp0\n").is_err());
    assert!(at("68020", "\tfsabs.x %fp0,%fp1\n").is_err());
    assert!(at("68040", "\tfsabs.x %fp0,%fp1\n").is_ok());
    // A bit pattern wider than its format.
    let e = at("68020", "\tfmove.s #0x1ffffffff,%fp0\n").unwrap_err();
    assert!(e.iter().any(|m| m.contains("does not fit")), "{e:?}");
    // `fbXX` stays a word branch: out of reach is an error, where GNU as
    // silently truncates the displacement.
    let e = at("68020", "\tfbne .Lfar\n\t.zero 40000\n.Lfar:\n\trts\n").unwrap_err();
    assert!(
        e.iter().any(|m| m.contains("does not fit 16 bits")),
        "{e:?}"
    );
    // `fjXX` grows instead.
    let image = at("68020", "\tfjne .Lfar\n\t.zero 40000\n.Lfar:\n\trts\n").unwrap();
    assert_eq!(image.image[..6], [0xF2, 0xCE, 0, 0, 0x9C, 0x44]);
}

#[test]
fn hostile_input_is_an_error_not_a_crash() {
    let at = |cpu: &str, origin: u32, text: &str| {
        assemble(
            &[SourceFile {
                name: "a.s".into(),
                text: text.to_string(),
            }],
            cpu,
            origin,
        )
        .map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>())
    };
    let deep = format!(".long {}1{}\n", "(".repeat(5000), ")".repeat(5000));
    let bitfields = format!("\tbftst %d0{}\n", "{#1:#1}".repeat(1000));
    for (text, message) in [
        // Sizes and addresses beyond the image limit or 32 bits.
        (".zero 0x80000000\n.zero 0x80000000\n", "larger than"),
        ("\tnop\n.p2align 30\n", "larger than"),
        (".skip 0xffffffff,1\n", "bytes of fill"),
        // Relaxation encodes at section offsets: found by fuzzing.
        (".zero 0xfffffff8\nx: tst.l x\n", "larger than"),
        (
            ".section .rodata.str1.1,\"aMS\",@progbits,1\n.zero 0x4000000\n",
            "larger than",
        ),
        (
            ".comm a,0xFFFFFFF0,4\n.comm b,0x100,4\n",
            "32-bit address space",
        ),
        // Values that silently lost bits.
        ("\tmovem.l #0x12345,-(%sp)\n", "does not fit 16 bits"),
        ("\tfmovem.x #0x100,-(%sp)\n", "does not fit 8 bits"),
        (".L2: bra.s .L2+1\n", "out of range"),
        (".long 1<<64\n", "out of range"),
        // Arithmetic overflow and unbounded nesting.
        ("x: nop\n.long x*0x7fffffffffffffff+x\n", "overflow"),
        (
            "x: nop\n\tmove.l (0-x*(-0x7fffffffffffffff-1))(%a0),%d0\n",
            "overflow",
        ),
        (&deep, "operators"),
        (&bitfields, "bad bitfield"),
        // Input the parser used to misread.
        (".data\n.ascii \"x\"y\"\n", "quoted string"),
        ("\tmove.l %d+1,%d01\n", "cannot parse"),
        (
            ".globl foo\n.data\nfoo: .long 1\n.comm foo,4,4\n",
            "also .comm",
        ),
        (
            "\tnop\n.section .note.GNU-stack,\"\",@progbits\n.previous\n\tnop\n",
            "unsupported directive",
        ),
    ] {
        let e = at("68020", 0, text).expect_err(text);
        assert!(e.iter().any(|m| m.contains(message)), "{text:?}: {e:?}");
    }
    // An image that ends beyond 4 GiB; one whose moved copy for the
    // fixup search wraps is fine.
    let e = at("68020", 0xFFFF_FFFC, "\tnop\n\tnop\n").unwrap_err();
    assert!(
        e.iter().any(|m| m.contains("32-bit address space")),
        "{e:?}"
    );
    // An alignment coarser than the probe's 4 KiB step: found by fuzzing.
    let image = at("68020", 0, "\tnop\n\t.p2align 15\nx:\t.long x\n").unwrap();
    assert_eq!(image.fixups, [0x8000]);
    // Near the top the fixups are still found.
    for origin in [0xFFFE_EFF0, 0xFFFF_FEF0] {
        let image = at("68020", origin, "\ttst.l %d0\nx:\t.long x\n").unwrap();
        assert_eq!(image.fixups, [2]);
    }
}

#[test]
fn branch_chains_relax_however_long() {
    // Each branch reaches past the next one only after that one grew, so
    // every round of relaxation grows one more.
    let mut src = String::from("\t.text\n");
    for i in (1..=1100).rev() {
        let target = if i > 1 {
            format!(".LA{}", i - 1)
        } else {
            ".LT".into()
        };
        src += &format!(
            "\tjra {target}\n.LA{i}:\n\t.zero {}\n",
            if i > 1 { 124 } else { 128 }
        );
    }
    src += ".LT:\tnop\n";
    let image = ok(&[("a.s", &src)]);
    // The first branch grew from a byte to a word.
    assert_eq!(image.image[..4], [0x60, 0, 0, 0x82]);
}

#[test]
fn weak_symbols_and_aliases() {
    let at = |cpu: &str, files: &[(&str, &str)]| {
        let files: Vec<SourceFile> = files
            .iter()
            .map(|(name, text)| SourceFile {
                name: name.to_string(),
                text: text.to_string(),
            })
            .collect();
        assemble(&files, cpu, 0).map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>())
    };
    // An undefined weak symbol is 0 and needs no fixup. Calling it
    // becomes `jsr` absolute on a 68020 as well: a 32-bit displacement to
    // a fixed address would not survive relocation.
    let weak = "\t.text\n\tjbsr maybe\n\tlea maybe,%a0\n\trts\n\t.weak maybe\n";
    for cpu in ["68000", "68020"] {
        let image = at(cpu, &[("a.s", weak)]).unwrap();
        assert_eq!(
            image.image[..14],
            [0x4E, 0xB9, 0, 0, 0, 0, 0x41, 0xF9, 0, 0, 0, 0, 0x4E, 0x75],
            "{cpu}"
        );
        assert!(image.fixups.is_empty());
    }
    // A definition that is not weak wins, in either order; two weak ones
    // keep the first.
    let weak_def = "\t.text\n\t.weak f\nf:\tmoveq #1,%d0\n\trts\n";
    let strong = "\t.text\n\t.globl f\nf:\tmoveq #2,%d0\n\trts\n";
    let user = "\t.text\n\t.globl start\nstart:\tjbsr f\n";
    for files in [
        [("w.s", weak_def), ("s.s", strong), ("u.s", user)],
        [("s.s", strong), ("w.s", weak_def), ("u.s", user)],
    ] {
        let image = at("68000", &files).unwrap();
        let f = image.symbols.iter().find(|s| s.name == "f").unwrap();
        assert_eq!(image.image[f.address as usize + 1], 2, "{files:?}");
    }
    let e = at("68000", &[("s.s", strong), ("t.s", strong)]).unwrap_err();
    assert!(e.iter().any(|m| m.contains("already defined")), "{e:?}");
    // `.set` in terms of itself.
    let e = at("68000", &[("a.s", "\t.set a,b\n\t.set b,a+1\n\t.long a\n")]).unwrap_err();
    assert!(e.iter().any(|m| m.contains("in terms of itself")), "{e:?}");
}
