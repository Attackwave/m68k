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

/// Finding 1 (`tests/programs/README.md`): a three-entry jump table — an
/// entirely ordinary `switch` size — is not recognized, because
/// `MIN_POINTER_TABLE` requires four consecutive pointers. Its longwords
/// render as `ori.b` instead.
///
/// This test states what happens *today*, deliberately: it is the
/// reproduction case for the finding. When the threshold is revisited,
/// this test should be inverted to assert `dc.l` and a resolved label,
/// which is exactly the signal that the finding was addressed.
#[test]
fn jump_table_of_three_entries_is_not_yet_recognized() {
    let lines = run(
        include_str!("../../../tests/programs/jump_table.s"),
        true, // even with --scan-tables
    );

    // The table starts at $101a; case0 is at $100e.
    assert_eq!(
        text_at(&lines, 0x101a),
        "ori.b   #$0e, d0",
        "if this now says `dc.l`, the 3-entry threshold was fixed — \
         invert this test (see tests/programs/README.md finding 1)"
    );
}

/// Finding 2: text detection can swallow real code. In
/// `pc_relative_data.s` the bytes `4e75 4440 4e75` are `rts; neg.w d0;
/// rts` — all printable ("NuD@Nu") — and the `dc.w 1,2,3,4` after them
/// supplies the NUL terminator the heuristic requires.
///
/// The cause is reachability: `negate` is reached only through the pointer
/// table, so the walk never claims those bytes, and unclaimed bytes are
/// what the text test may judge. Like the test above, this records
/// today's behaviour as the reproduction case.
#[test]
fn text_detection_can_swallow_unreached_code() {
    let lines = run(
        include_str!("../../../tests/programs/pc_relative_data.s"),
        false,
    );

    // $101a is `rts`, the tail of `double`, which is real code.
    let at = text_at(&lines, 0x101a);
    assert!(
        at.contains("NuD@Nu"),
        "if this is now `rts`, the swallowing was fixed — invert this \
         test (see tests/programs/README.md finding 2); got {:?}",
        at
    );
}
