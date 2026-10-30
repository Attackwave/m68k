//! Every instruction form the encoder was once checked against a reference
//! assembler for, pinned to its bytes.
//!
//! The lists under `tests/data/reference/` are the sweep lists from that
//! comparison (base, rest, ea, ext, fpu, mmu: forms that must encode to
//! exactly these bytes; strict: forms that must be rejected). The bytes
//! were recorded from this encoder at the state the comparison had
//! verified, so the files are the project's own expectation from here on:
//! a disagreement is settled against the Programmer's Reference Manual and
//! the file is corrected, never regenerated wholesale.
//!
//! File format: `# cpu <model>` header, then `<source> | <hex>` or
//! `<source> | reject`, one instruction per line.

use m68k_asm::assembler::Assembler;

const LISTS: &[(&str, &str)] = &[
    ("base", include_str!("data/reference/base.txt")),
    ("rest", include_str!("data/reference/rest.txt")),
    ("ea", include_str!("data/reference/ea.txt")),
    ("ext", include_str!("data/reference/ext.txt")),
    ("fpu", include_str!("data/reference/fpu.txt")),
    ("mmu", include_str!("data/reference/mmu.txt")),
    ("strict", include_str!("data/reference/strict.txt")),
];

fn encode(cpu: &str, instruction: &str) -> Result<String, String> {
    let mut asm = Assembler::new(0);
    asm.set_cpu(cpu);
    asm.assemble_bytes(&format!("\t{instruction}\n"))
        .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect())
        .map_err(|e| e.message)
}

#[test]
fn every_listed_form_encodes_to_its_reference_bytes() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, text) in LISTS {
        let cpu = text
            .lines()
            .find_map(|l| l.strip_prefix("# cpu "))
            .unwrap_or_else(|| panic!("{name}: missing `# cpu` header"))
            .trim();
        for (idx, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (source, want) = line
                .split_once('|')
                .unwrap_or_else(|| panic!("{name}:{}: no `|`", idx + 1));
            let (source, want) = (source.trim(), want.trim());
            checked += 1;
            let got = encode(cpu, source);
            let ok = match (&got, want) {
                (Err(_), "reject") => true,
                (Ok(bytes), want) => bytes == want,
                _ => false,
            };
            if !ok {
                failures.push(format!(
                    "{name}:{}: {source}: want {want}, got {}",
                    idx + 1,
                    match got {
                        Ok(b) => b,
                        Err(e) => format!("reject ({e})"),
                    }
                ));
            }
        }
    }
    assert!(
        checked > 900,
        "only {checked} forms checked, lists truncated?"
    );
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
