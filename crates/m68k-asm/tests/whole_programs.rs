//! Whole-program tests over `tests/programs/`.
//!
//! The other suites check instruction *forms* in isolation. These check
//! what only a complete program can exercise: whether the disassembler
//! separates code from data correctly when the two are interleaved, which
//! is where every real-world defect in this area has been.
//!
//! The programs are ours, so the truth is known — see
//! `tests/programs/README.md`. Assertions here state that truth directly
//! rather than comparing against a recorded snapshot, so a failure names
//! the property that broke instead of showing a diff.

use m68k_asm::assembler::Assembler;
use m68k_disasm::disassembler::{DisassembledLine, Disassembler};

const ORIGIN: u32 = 0x1000;

/// Assemble one of the corpus programs and disassemble it back, with the
/// entry point the programs all use.
fn run(source: &str, scan_tables: bool) -> Vec<DisassembledLine> {
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    let bytes = asm
        .assemble_bytes(source)
        .unwrap_or_else(|e| panic!("corpus program failed to assemble: {:?}", e));

    let mut disasm = Disassembler::new(bytes, ORIGIN);
    disasm.set_cpu("68000");
    disasm.add_entry_points([ORIGIN]);
    if scan_tables {
        disasm.seed_entry_points_from_pointer_tables();
    }
    disasm.disassemble()
}

fn line_at(lines: &[DisassembledLine], addr: u32) -> Option<&DisassembledLine> {
    lines.iter().find(|l| l.address == addr)
}

/// Text of the line at `addr`, for assertions that want to read it.
fn text_at(lines: &[DisassembledLine], addr: u32) -> String {
    line_at(lines, addr).map_or_else(
        || format!("<no line at {:08x}>", addr),
        |l| l.text.trim().to_string(),
    )
}

/// Every byte of the image appears in exactly one line, in address order.
///
/// A gap means bytes vanished from the listing; an overlap means two lines
/// claim the same byte. Either way the output cannot reassemble to the
/// original, which is the one property every program here must have.
fn assert_byte_complete(lines: &[DisassembledLine], label: &str) {
    let mut next = ORIGIN;
    for line in lines {
        assert_eq!(
            line.address, next,
            "{}: gap or overlap before {:08x} (expected {:08x})",
            label, line.address, next
        );
        assert!(
            !line.raw_bytes.is_empty(),
            "{}: line at {:08x} claims no bytes",
            label,
            line.address
        );
        next = line.address + line.raw_bytes.len() as u32;
    }
}

#[test]
fn nested_loops_is_all_code_and_invents_no_data() {
    // The control case: a program with no data at all. The heuristics must
    // not invent any, and this direction of error is easy to introduce
    // while fixing the opposite one.
    let lines = run(
        include_str!("../../../tests/programs/nested_loops.s"),
        false,
    );

    let data: Vec<String> = lines
        .iter()
        .filter(|l| l.text.trim_start().starts_with("dc."))
        .map(|l| format!("{:08x}: {}", l.address, l.text.trim()))
        .collect();
    assert!(
        data.is_empty(),
        "pure code was rendered as data: {:#?}",
        data
    );
    assert_byte_complete(&lines, "nested_loops");
}

#[test]
fn strings_between_code_are_recognized_as_text() {
    let lines = run(
        include_str!("../../../tests/programs/strings_between_code.s"),
        false,
    );

    let text: Vec<&DisassembledLine> = lines
        .iter()
        .filter(|l| l.text.contains('"'))
        .collect::<Vec<_>>();
    assert!(
        text.iter().any(|l| l.text.contains("Hello, Amiga!")),
        "the greeting was not recognized as text; got: {:#?}",
        lines
            .iter()
            .map(|l| format!("{:08x}: {}", l.address, l.text.trim()))
            .collect::<Vec<_>>()
    );
    assert_byte_complete(&lines, "strings_between_code");
}

#[test]
fn every_corpus_program_is_byte_complete() {
    // Byte-completeness is the invariant that must hold for all of them,
    // whatever the classification decisions turn out to be.
    for (name, src) in [
        (
            "jump_table",
            include_str!("../../../tests/programs/jump_table.s"),
        ),
        (
            "strings_between_code",
            include_str!("../../../tests/programs/strings_between_code.s"),
        ),
        (
            "pc_relative_data",
            include_str!("../../../tests/programs/pc_relative_data.s"),
        ),
        (
            "nested_loops",
            include_str!("../../../tests/programs/nested_loops.s"),
        ),
        (
            "mixed_data_code",
            include_str!("../../../tests/programs/mixed_data_code.s"),
        ),
    ] {
        assert_byte_complete(&run(src, false), name);
        assert_byte_complete(&run(src, true), name);
    }
}

/// Finding 1, now fixed: a three-entry jump table is recognized, and its
/// targets get labels.
///
/// Two separate causes, both real. `MIN_POINTER_TABLE` demanded four
/// consecutive pointers, and a 3-way `switch` is ordinary. Worse, the
/// scan walked a *longword* grid, so it could only ever see tables whose
/// distance from the origin is a multiple of four — this table sits at
/// origin+$1a and was invisible regardless of the threshold.
#[test]
fn three_entry_jump_table_is_recognized_and_labelled() {
    let lines = run(include_str!("../../../tests/programs/jump_table.s"), true);

    // The table is at $101a; the three cases are at $100e/$1012/$1016.
    for (i, addr) in [0x101au32, 0x101e, 0x1022].iter().enumerate() {
        let text = text_at(&lines, *addr);
        assert!(
            text.starts_with("dc.l"),
            "table entry {} at {:08x} decoded as code: {:?}",
            i,
            addr,
            text
        );
        assert!(
            text.contains("label"),
            "table entry {} at {:08x} should name its target: {:?}",
            i,
            addr,
            text
        );
    }

    // Each dispatch target carries a label definition, so the listing
    // reassembles to the same table rather than to raw addresses.
    for addr in [0x100eu32, 0x1012, 0x1016] {
        assert!(
            line_at(&lines, addr).is_some_and(|l| l.label.is_some()),
            "dispatch target {:08x} has no label",
            addr
        );
    }
}

/// Finding 2, half fixed: a string run clamped against traced code must
/// still be long enough to *be* a string.
///
/// `clamp_to_traced` cuts a run where proven code resumes, which could
/// leave two bytes — and those were emitted as `dc.b "Nu"`, swallowing
/// the `rts` tail of a routine. The minimum length is now re-checked
/// after clamping, so the remainder decodes as the code it is.
#[test]
fn clamped_string_run_below_minimum_is_not_text() {
    // With `negate` given as an entry point, the trace claims $101c, so
    // the run starting at $101a is clamped to two bytes.
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    let bytes = asm
        .assemble_bytes(include_str!("../../../tests/programs/pc_relative_data.s"))
        .expect("corpus program failed to assemble");
    let mut disasm = Disassembler::new(bytes, ORIGIN);
    disasm.set_cpu("68000");
    disasm.add_entry_points([ORIGIN, 0x101c]);
    let lines = disasm.disassemble();

    assert_eq!(
        text_at(&lines, 0x101a),
        "rts",
        "a two-byte clamped run was emitted as text"
    );
}

/// The other half of finding 2 is *not* fixed, and cannot be by this
/// mechanism: with only the program's own entry point, nothing proves
/// `negate` is code, and `4e75 4440 4e75` ("NuD@Nu") is a genuine
/// NUL-terminated printable run over the length threshold.
///
/// Recorded rather than left implicit: this is the residual false
/// positive of text detection, and the honest fix is better reachability
/// (an entry point, a trace), not a stricter string test — which would
/// start losing real strings instead.
#[test]
fn unreachable_printable_code_is_still_misread_as_text() {
    let lines = run(
        include_str!("../../../tests/programs/pc_relative_data.s"),
        false,
    );

    assert!(
        text_at(&lines, 0x101a).contains("NuD@Nu"),
        "if this is now code, reachability improved — update \
         tests/programs/README.md finding 2"
    );
}
