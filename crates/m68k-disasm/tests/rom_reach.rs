//! Reach of the disassembler on a real Kickstart ROM.
//!
//! Three counts of instructions found: by the linear scan, by control-flow
//! tracing from the reset vector, and by tracing with pointer-table
//! scanning. Each is checked against a lower bound recorded from a run, so
//! a change that finds less code fails here; one that finds more just
//! raises the numbers it prints.
//!
//! Ignored by default because a ROM is needed:
//! `M68K_ROM=tmp/<rom> cargo test -p m68k-disasm --test rom_reach -- --ignored --nocapture`

use m68k_disasm::disassembler::Disassembler;

/// Lower bounds per ROM size: (linear, traced, traced with table scan).
/// Regression guard: a change that lowers the reach must adjust the
/// baseline deliberately. Recorded from Kickstart 1.3 (256 KiB) and
/// 3.1 (512 KiB).
const BASELINE_256K: (usize, usize, usize) = (80619, 74277, 75084);
const BASELINE_512K: (usize, usize, usize) = (167758, 154467, 157072);

fn instructions(mut disasm: Disassembler) -> usize {
    disasm
        .disassemble()
        .iter()
        .filter(|l| !l.is_error && !l.text.starts_with("dc."))
        .count()
}

#[test]
#[ignore]
fn kickstart_reach_does_not_regress() {
    let Ok(path) = std::env::var("M68K_ROM") else {
        eprintln!("M68K_ROM not set, skipping");
        return;
    };
    let mut image = std::fs::read(&path).expect("read ROM");
    // Some dumps carry a short trailer; the ROM proper is a power of two.
    let (origin, size, baseline) = if image.len() >= 512 * 1024 {
        (0xF8_0000, 512 * 1024, BASELINE_512K)
    } else {
        (0xFC_0000, 256 * 1024, BASELINE_256K)
    };
    image.truncate(size);
    let reset = u32::from_be_bytes(image[4..8].try_into().unwrap());

    let linear = instructions(Disassembler::new(image.clone(), origin));

    let mut traced = Disassembler::new(image.clone(), origin);
    traced.add_entry_points([reset]);
    let traced = instructions(traced);

    let mut scanned = Disassembler::new(image, origin);
    scanned.add_entry_points([reset]);
    scanned.seed_entry_points_from_pointer_tables();
    let scanned = instructions(scanned);

    eprintln!("{path}: linear {linear}, traced {traced}, traced+tables {scanned}");
    assert!(
        linear >= baseline.0,
        "linear reach fell to {linear}, baseline {}",
        baseline.0
    );
    assert!(
        traced >= baseline.1,
        "traced reach fell to {traced}, baseline {}",
        baseline.1
    );
    assert!(
        scanned >= baseline.2,
        "traced+tables reach fell to {scanned}, baseline {}",
        baseline.2
    );
}
