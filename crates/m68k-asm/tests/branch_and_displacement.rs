//! Regressions for branch sizing and displacement parsing.
//!
//! These came out of building a large multi-file program rather than
//! constructed snippets, so each test states the symptom as it appeared
//! in practice.

use m68k_asm::assembler::Assembler;

const ORIGIN: u32 = 0x8000;

fn assemble(source: &str) -> Result<Vec<u8>, String> {
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    asm.assemble_bytes(source).map_err(|e| e.message)
}

/// An assembled image together with its defined symbols.
type Assembled = (Vec<u8>, Vec<(String, u32)>);

/// Assemble and return the symbol table alongside the image — the pair
/// that has to agree, and did not when a branch was silently widened.
fn assemble_with_symbols(source: &str, optimize: bool) -> Result<Assembled, String> {
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    asm.set_optimize(optimize);
    let bytes = asm.assemble_bytes(source).map_err(|e| e.message)?;
    let symbols = asm
        .symbols
        .iter()
        .filter(|(_, e)| e.defined)
        .map(|(n, e)| (n.clone(), e.value))
        .collect();
    Ok((bytes, symbols))
}

fn symbol(symbols: &[(String, u32)], name: &str) -> Option<u32> {
    symbols.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
}

// --- Parenthesized expressions in a displacement ---------------------

/// A displacement like `STRIDE*(HEIGHT-8)(A0)` was rejected as
/// unparseable because the parser split at the first `(`, reading an
/// empty displacement.
#[test]
fn displacement_may_be_a_parenthesized_expression() {
    let source = "\
A   EQU 80
B   EQU 256
    ORG $8000
    LEA A*8(A0),A1
    LEA (A*8)(A0),A1
    LEA A*(B-8)(A0),A1
    LEA (A*(B-8))(A0),A1
    MOVE.W #A*(B-8),D0
";
    let bytes = assemble(source).expect("all four LEA forms must assemble");

    // Both spellings of A*8 (=640=$0280) must encode identically, and
    // both spellings of A*(B-8) (=19840=$4D80) likewise. The trailing
    // MOVE.W is the reference value: the same expression outside an
    // addressing mode always parsed correctly.
    assert_eq!(&bytes[0..4], &[0x43, 0xE8, 0x02, 0x80], "A*8(A0)");
    assert_eq!(&bytes[4..8], &[0x43, 0xE8, 0x02, 0x80], "(A*8)(A0)");
    assert_eq!(&bytes[8..12], &[0x43, 0xE8, 0x4D, 0x80], "A*(B-8)(A0)");
    assert_eq!(&bytes[12..16], &[0x43, 0xE8, 0x4D, 0x80], "(A*(B-8))(A0)");
    assert_eq!(&bytes[16..20], &[0x30, 0x3C, 0x4D, 0x80], "#A*(B-8)");
}

/// The ordinary forms must keep working — the fix walks parens backwards
/// and must not mistake an index register's parens for the address part.
#[test]
fn ordinary_displacement_forms_still_parse() {
    let source = "\
OFF EQU 8
    ORG $8000
    MOVE.B  (A0),D1
    MOVE.B  (4,A0),D1
    MOVE.B  OFF(A1),D0
    MOVE.B  (A0,D0.W),D1
    MOVE.B  4(A0,D0.W),D1
    LEA     (A0),A1
";
    assemble(source).expect("standard addressing forms must still parse");
}

// --- Out-of-range `.S` corrupted the symbol table ---------------------

/// The serious one: an explicit `.S` whose target was out of range got
/// silently widened to the word form, but pass 1 had budgeted 2 bytes.
/// Every symbol after it was reported 2 bytes low, so the code was right
/// and the symbol table was wrong — the kind of defect that only shows
/// up once something reads an address out of that table and jumps into
/// the middle of an instruction.
#[test]
fn out_of_range_short_branch_is_rejected_not_silently_widened() {
    for mnemonic in ["BRA", "BSR", "BNE"] {
        let source = format!(
            "\
    ORG $8000
fn: MOVE.L D0,D1
    {mnemonic}.S .far
    DCB.W 200,$4E71
.far:
    NOP
after:
    RTS
"
        );
        let err = assemble(&source)
            .expect_err("an out-of-range .s branch must be an error, not a silent widening");
        assert!(
            err.to_lowercase().contains("out of range"),
            "{mnemonic}.s: error should name the range problem, got: {err}"
        );
    }
}

/// Backwards is the same defect in the other direction.
#[test]
fn out_of_range_backward_short_branch_is_rejected() {
    let source = "\
    ORG $8000
back:
    NOP
    DCB.W 200,$4E71
    BRA.S back
";
    let err = assemble(source).expect_err("backward out-of-range .s must be rejected");
    assert!(err.to_lowercase().contains("out of range"), "got: {err}");
}

/// Without a size suffix the branch relaxes to the word form, and every
/// symbol after it must match the real image layout. This is the case
/// that silently drifted before.
#[test]
fn unsuffixed_branch_keeps_the_symbol_table_aligned_with_the_image() {
    let source = "\
    ORG $8000
fn: MOVE.L D0,D1
    BNE .far
    DCB.W 200,$4E71
.far:
    NOP
after:
    RTS
";
    let (bytes, symbols) = assemble_with_symbols(source, false).expect("must assemble");

    // `after` labels the final RTS, which is the last two bytes.
    let expected = ORIGIN + bytes.len() as u32 - 2;
    assert_eq!(
        symbol(&symbols, "after"),
        Some(expected),
        "symbol table must agree with the {} byte image",
        bytes.len()
    );
}

/// A `.S` branch that does fit must still use the 2-byte form.
#[test]
fn in_range_short_branch_still_encodes_short() {
    let source = "\
    ORG $8000
fn: MOVE.L D0,D1
    BNE.S .near
    NOP
.near:
    NOP
after:
    RTS
";
    let (bytes, symbols) = assemble_with_symbols(source, false).expect("must assemble");
    assert_eq!(bytes.len(), 10, "short branch must stay 2 bytes");
    assert_eq!(symbol(&symbols, "after"), Some(ORIGIN + 8));
}

// --- Zero displacement wasted two bytes -------------------------------

/// `MOVE.L #imm,OFF(A0)` with `OFF EQU 0` emitted the (d16,An) form with
/// a zero displacement word. Under `--optimize` it should use `(An)`.
#[test]
fn zero_displacement_shortens_to_register_indirect_when_optimizing() {
    let source = "\
OFF EQU 0
    ORG $8000
a:  MOVE.L #$12345678,OFF(A0)
b:  MOVE.L #$12345678,(A0)
";
    let (plain, _) = assemble_with_symbols(source, false).expect("must assemble");
    assert_eq!(
        plain.len(),
        14,
        "without --optimize the layout must not move"
    );

    let (optimized, symbols) = assemble_with_symbols(source, true).expect("must assemble");
    assert_eq!(
        optimized.len(),
        12,
        "zero displacement should cost no extra word"
    );
    // Both lines now mean and encode the same thing.
    assert_eq!(&optimized[0..6], &optimized[6..12]);
    assert_eq!(symbol(&symbols, "b"), Some(ORIGIN + 6));
}

/// A non-zero displacement must keep its extension word even when
/// optimizing — the shortcut applies only to a displacement of 0.
#[test]
fn nonzero_displacement_keeps_its_extension_word_when_optimizing() {
    let source = "\
OFF EQU 4
    ORG $8000
    MOVE.L #$12345678,OFF(A0)
";
    let (bytes, _) = assemble_with_symbols(source, true).expect("must assemble");
    assert_eq!(bytes.len(), 8, "displacement 4 still needs its word");
    assert_eq!(&bytes[6..8], &[0x00, 0x04]);
}
