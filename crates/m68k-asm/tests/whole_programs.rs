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
        (
            "recursion",
            include_str!("../../../tests/programs/recursion.s"),
        ),
        (
            "word_dispatch",
            include_str!("../../../tests/programs/word_dispatch.s"),
        ),
        (
            "inline_data",
            include_str!("../../../tests/programs/inline_data.s"),
        ),
        (
            "self_modifying",
            include_str!("../../../tests/programs/self_modifying.s"),
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

/// A recursive routine does not make the walk loop or stop early.
///
/// The `bsr` inside `fact` targets an address the tracer has already
/// visited *and* is currently inside. A tracer that tracks "visited" per
/// call rather than per address either recurses forever or abandons the
/// rest of the routine; either way the tail after the call disappears.
#[test]
fn recursion_is_traced_without_looping_or_truncating() {
    let lines = run(include_str!("../../../tests/programs/recursion.s"), false);

    // The recursive call resolves to the routine's own entry, not to a
    // fresh label per visit.
    assert_eq!(text_at(&lines, 0x101c), "bsr.w   label0");
    assert!(
        line_at(&lines, 0x100a).is_some_and(|l| l.label.is_some()),
        "the recursive routine's entry has no label"
    );

    // Everything after the recursive call is still decoded: the walk did
    // not stop at the point of recursion.
    assert_eq!(text_at(&lines, 0x1028), "unlk    a6");
    assert_eq!(text_at(&lines, 0x102a), "rts");

    // Frame offsets stay negative displacements off a6; they are not
    // addresses and must not become labels below the origin.
    assert!(
        text_at(&lines, 0x1020).contains("-$4(a6)"),
        "frame offset was resolved as an address: {:?}",
        text_at(&lines, 0x1020)
    );

    assert_byte_complete(&lines, "recursion");
}

/// A table of word *offsets* is data, and a table of `bra` instructions
/// is code — neither contains an address, so neither can be found by a
/// pointer scan.
///
/// These pull in opposite directions, which is why they share a file: a
/// heuristic loose enough to call the offset table data will also call
/// the branch table data and destroy four real instructions.
#[test]
fn offset_table_is_data_and_branch_table_stays_code() {
    let lines = run(
        include_str!("../../../tests/programs/word_dispatch.s"),
        true,
    );

    // The five word offsets are small integers, not instructions.
    for addr in [0x102cu32, 0x102e, 0x1030, 0x1032, 0x1034] {
        let text = text_at(&lines, addr);
        assert!(
            text.starts_with("dc.w"),
            "offset table entry at {:08x} decoded as code: {:?}",
            addr,
            text
        );
    }

    // The branch table is executable and must survive as instructions.
    for addr in [0x1042u32, 0x1046, 0x104a, 0x104e] {
        let text = text_at(&lines, addr);
        assert!(
            text.starts_with("bra.w"),
            "branch table entry at {:08x} was rendered as data: {:?}",
            addr,
            text
        );
    }

    assert_byte_complete(&lines, "word_dispatch");
}

/// Self-modifying code is disassembled as *assembled*, not as patched.
///
/// `patch` rewrites the immediate of the `moveq` at $102a at run time. A
/// disassembler cannot know the patched value and must not speculate: the
/// listing shows what the bytes say, which is what reassembles.
#[test]
fn self_modifying_code_is_shown_as_assembled() {
    let lines = run(
        include_str!("../../../tests/programs/self_modifying.s"),
        false,
    );

    assert_eq!(
        text_at(&lines, 0x102a),
        "moveq   #$00000001, d0",
        "the patched-at-run-time instruction was not shown as assembled"
    );

    // The byte store into the middle of that instruction is an ordinary
    // displacement, not a reference that turns $102b into a label.
    assert!(
        text_at(&lines, 0x101a).contains("$1(a0)"),
        "store into an instruction was resolved as an address: {:?}",
        text_at(&lines, 0x101a)
    );

    assert_byte_complete(&lines, "self_modifying");
}

/// Arguments stored inline after a `jsr` are still read as code.
///
/// Recorded as a known limit rather than asserted as correct. The callee
/// reads its arguments over the return address and resumes past them, so
/// the bytes after the call site are data — but nothing in any opword
/// says so, and every other `jsr` in every other program *is* followed by
/// code. Fixing this needs the tracer to model the callee's adjustment of
/// its own return address, not a stricter data heuristic.
///
/// The test pins the current behaviour so the day it improves is visible.
#[test]
fn inline_arguments_after_a_call_are_still_read_as_code() {
    let lines = run(include_str!("../../../tests/programs/inline_data.s"), false);

    // $100a holds `dc.w 7 / dc.w 9`, decoded as one instruction.
    assert_eq!(
        text_at(&lines, 0x100a),
        "ori.b   #$09, d7",
        "if these are now data, the tracer models inline arguments — \
         update tests/programs/README.md"
    );

    // Whatever the classification, no byte may go missing.
    assert_byte_complete(&lines, "inline_data");
}
