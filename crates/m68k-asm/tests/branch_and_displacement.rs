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

// --- Reservation-only source and RS struct offsets --------------------

/// A source file holding nothing but `DS` reservations is a valid BSS
/// file: it asks for a zero-filled image of the reserved size, but was
/// rejected because no instruction existed to take a base address from.
#[test]
fn reservation_only_source_produces_a_zero_filled_image() {
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    asm.assemble("    ORG $8000\nv:  DS.L 1\nw:  DS.B 6\n")
        .expect("a pure BSS source must assemble");

    let (bytes, base) =
        m68k_asm::output::generate_binary_from(&asm.code, Some(asm.end_pc()), Some(asm.origin()))
            .expect("reservations must produce an image");

    assert_eq!(base, ORIGIN);
    assert_eq!(bytes.len(), 10, "DS.L 1 + DS.B 6 = 10 bytes");
    assert!(bytes.iter().all(|&b| b == 0), "reserved space is zeroed");
}

/// A file that reserves nothing really is empty, and must stay an error
/// — that guard catches accidentally empty sources.
#[test]
fn truly_empty_source_still_produces_nothing() {
    let mut asm = Assembler::new(ORIGIN);
    asm.set_cpu("68000");
    asm.assemble("    ORG $8000\n").expect("must assemble");

    assert!(
        m68k_asm::output::generate_binary_from(&asm.code, Some(asm.end_pc()), Some(asm.origin()))
            .is_none(),
        "a source with neither code nor reservations has no image"
    );
}

/// `RS` counts bytes from a struct base. `RS.L 1` failed outright with
/// "undefined symbol: l" (the size suffix was evaluated as the count),
/// and the labels resolved to the current PC instead of their offset.
#[test]
fn rs_directives_number_struct_fields_by_byte_offset() {
    let source = "\
        RSRESET
TK_PC   RS.L 1
TK_SR   RS.W 1
TK_NAME RS.B 16
TK_SIZE RS.B 0
        ORG $8000
        MOVE.W #TK_PC,D0
";
    let (_, symbols) = assemble_with_symbols(source, false).expect("RS must assemble");

    assert_eq!(symbol(&symbols, "TK_PC"), Some(0));
    assert_eq!(symbol(&symbols, "TK_SR"), Some(4), "after RS.L 1");
    assert_eq!(symbol(&symbols, "TK_NAME"), Some(6), "after RS.W 1");
    // RS.B 0 reserves nothing: the running total is the struct size.
    assert_eq!(symbol(&symbols, "TK_SIZE"), Some(22), "after RS.B 16");
}

/// The offsets have to survive into an addressing mode, which is what
/// they exist for.
#[test]
fn rs_offsets_work_as_displacements() {
    let source = "\
        RSRESET
F_A     RS.L 1
F_B     RS.W 1
        ORG $8000
        MOVE.L F_A(A0),D0
        MOVE.W F_B(A0),D1
";
    let bytes = assemble(source).expect("RS offsets must address");
    // move.l 0(a0),d0 then move.w 4(a0),d1
    assert_eq!(&bytes[0..4], &[0x20, 0x28, 0x00, 0x00]);
    assert_eq!(&bytes[4..8], &[0x32, 0x28, 0x00, 0x04]);
}

/// `RSSET` starts the counter somewhere other than zero.
#[test]
fn rsset_starts_the_counter_at_a_given_value() {
    let source = "\
        RSSET $10
F1      RS.W 1
F2      RS.L 1
        ORG $8000
        MOVE.W #F1,D0
";
    let (_, symbols) = assemble_with_symbols(source, false).expect("RSSET must assemble");
    assert_eq!(symbol(&symbols, "F1"), Some(0x10));
    assert_eq!(symbol(&symbols, "F2"), Some(0x12));
}

/// An unknown size on RS is a typo, not a word-sized default.
#[test]
fn rs_rejects_an_unknown_size_suffix() {
    let err =
        assemble("        RSRESET\nF   RS.Q 1\n").expect_err("RS.Q is not a size the 68k has");
    assert!(err.to_lowercase().contains("size"), "got: {err}");
}

// --- Absolute-short shortening range ---------------------------------

/// Absolute short is sign-extended, so it reaches `$0000..$7FFF` and the
/// top of the address space — not the unsigned `$0000..$FFFF`. Testing
/// the unsigned range let `$8000..$FFFF` through as short, and the
/// encoder then rejected the operand it had been handed: `--optimize`
/// failed outright on any program based at `$8000`, rather than leaving
/// the long form in place. Shortening is opportunistic.
#[test]
fn optimize_keeps_the_long_form_for_unreachable_addresses() {
    let source = "\
    ORG $8000
    CLR.L v
    LEA v,A0
    MOVE.L v,D0
v:  DC.L 0
";
    let (bytes, _) = assemble_with_symbols(source, true)
        .expect("an address outside short range must keep the long form, not fail");

    // clr.l, lea and move.l each in their long-absolute encoding.
    assert_eq!(&bytes[0..2], &[0x42, 0xB9], "clr.l must stay long");
    assert_eq!(&bytes[6..8], &[0x41, 0xF9], "lea must stay long");
    assert_eq!(&bytes[12..14], &[0x20, 0x39], "move.l must stay long");
}

/// The whole range, in one place: what shortens and what does not.
#[test]
fn optimize_shortens_exactly_the_reachable_addresses() {
    // (address, expected opword for `CLR.L <addr>`)
    const SHORT: [u32; 3] = [0x0000_1000, 0x0000_7FFF, 0xFFFF_8000];
    const LONG: [u32; 4] = [0x0000_8000, 0x0000_FFFF, 0x0001_0000, 0x00DF_F180];

    for addr in SHORT {
        let source = format!("    ORG $1000\n    CLR.L ${addr:08X}\n");
        let (bytes, _) = assemble_with_symbols(&source, true)
            .unwrap_or_else(|e| panic!("${addr:08X} must assemble: {e}"));
        assert_eq!(
            &bytes[0..2],
            &[0x42, 0xB8],
            "${addr:08X} is reachable and should shorten"
        );
    }

    for addr in LONG {
        let source = format!("    ORG $1000\n    CLR.L ${addr:08X}\n");
        let (bytes, _) = assemble_with_symbols(&source, true)
            .unwrap_or_else(|e| panic!("${addr:08X} must assemble: {e}"));
        assert_eq!(
            &bytes[0..2],
            &[0x42, 0xB9],
            "${addr:08X} is not reachable and must stay long"
        );
    }
}

/// The high half must survive the round trip: `$FFFF8000` and `-32768`
/// name the same location and encode to the same extension word.
#[test]
fn high_addresses_shorten_to_a_sign_extended_word() {
    let (bytes, _) =
        assemble_with_symbols("    ORG $1000\n    CLR.L $FFFF8000\n", true).expect("must assemble");
    assert_eq!(&bytes[0..4], &[0x42, 0xB8, 0x80, 0x00]);

    let (top, _) =
        assemble_with_symbols("    ORG $1000\n    CLR.L $FFFFFFFF\n", true).expect("must assemble");
    assert_eq!(&top[0..4], &[0x42, 0xB8, 0xFF, 0xFF]);
}

/// Without `--optimize` nothing shortens, whatever the address.
#[test]
fn addresses_stay_long_without_optimize() {
    let (bytes, _) =
        assemble_with_symbols("    ORG $1000\n    CLR.L $1000\n", false).expect("must assemble");
    assert_eq!(
        &bytes[0..2],
        &[0x42, 0xB9],
        "no shortening without the flag"
    );
}
