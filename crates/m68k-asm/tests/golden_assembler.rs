//! Golden vector tests using the 2-pass assembler end-to-end.
//!
//! # The vector file is frozen, by decision
//!
//! `tests/golden/vectors.json` holds 123 vectors snapshotted at v1.0.2 and
//! is deliberately **not** extended as new instruction forms land. Its value
//! is precisely that it does not move: it pins encoder output produced
//! before several rounds of audits, so a regression that both the encoder
//! and a freshly regenerated expectation would agree on still fails here.
//! A snapshot regenerated from the current implementation could not do that.
//!
//! The consequence is that everything added since v1.0.2 (MMU forms, the
//! FMOVE k-factor syntax, the v2.0.x fixes) is absent from this file. That
//! is intended, not an oversight: `tests/instruction_coverage.rs` is the
//! authoritative instruction-set suite and is where new forms are added.

use m68k_asm::assembler::Assembler;

fn assemble(source: &str, cpu: &str, origin: u32) -> Result<Vec<u8>, String> {
    let mut asm = Assembler::new(origin);
    asm.set_cpu(cpu);
    // Golden vectors that reference `label` expect it to name the branch
    // instruction's own address (branch-to-self).
    let full = format!("    ORG ${:x}\nLABEL:\n{}", origin, source);
    asm.assemble_bytes(&full).map_err(|e| format!("{:?}", e))
}

/// Golden-vector encoder tests run end-to-end through the 2-pass assembler
/// (`Assembler::assemble_bytes`), covering every CPU level present in the
/// golden vectors (68000/68020/68040), not just the 68000 base instruction
/// set.
#[test]
fn test_golden_assembler() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/vectors.json"))
            .expect("Failed to parse golden vectors");
    let tests = data["encoder_tests"].as_array().unwrap();
    let mut fails = Vec::new();
    let origin = 0x1000;

    for t in tests {
        let name = t["name"].as_str().unwrap();
        let input = t["input"].as_str().unwrap();
        let exp = t["expected_hex"].as_str().unwrap_or("").to_lowercase();
        let cpu = t["cpu"].as_str().unwrap_or("68000");
        if exp.is_empty() {
            continue;
        }

        let source = format!("    {}", input.to_uppercase());
        match assemble(&source, cpu, origin) {
            Ok(bytes) => {
                let got: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                if got != exp {
                    fails.push(format!(
                        "{} (cpu={}): expected={} got={}",
                        name, cpu, exp, got
                    ));
                }
            }
            Err(e) => {
                fails.push(format!("{} (cpu={}): err: {}", name, cpu, e));
            }
        }
    }

    if !fails.is_empty() {
        panic!("Failures ({}):\n{}", fails.len(), fails.join("\n"));
    }
}

#[test]
fn test_assembler_roundtrip_basic() {
    // Roundtrip test for basic instructions the decoder can handle
    let cases = [
        ("NOP", "4e71"),
        ("RTS", "4e75"),
        ("RTE", "4e73"),
        ("RTR", "4e77"),
        ("TRAPV", "4e76"),
        ("RESET", "4e70"),
    ];
    let labels = std::collections::HashMap::new();

    for (name, hex) in &cases {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let mut stream = m68k_core::addressing::InstructionStream::new(&bytes, 0);
        match m68k_disasm::decoder::decode_next(&mut stream, "68000") {
            Ok((_, m68k_disasm::decoder::DecodeResult::Instruction(inst))) => {
                let formatted = inst.format(&labels);
                assert!(
                    formatted.to_lowercase().starts_with(&name.to_lowercase()),
                    "{}: expected '{}', got '{}'",
                    name,
                    name,
                    formatted
                );
            }
            Ok((_, m68k_disasm::decoder::DecodeResult::DataWord(dw))) => {
                panic!("{}: decoded as data word: {}", name, dw.format(&labels));
            }
            Err(e) => {
                panic!("{}: decode err: {}", name, e);
            }
        }
    }
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Systematic encode -> decode -> format -> reassemble -> re-encode
/// roundtrip over every golden encoder vector (all CPU levels present:
/// 68000/68020/68040). This is the automated version of the manual ~35-form
/// roundtrip verification done for B9 (see AGENTS.md); it would have
/// caught 1.2 (brief-format Xn/disp swap), 1.3 (ADDA/SUBA/CMPA size-bit
/// position), and 1.6 (disassembler dropping scale bits) immediately, since
/// each of those breaks either the decode step or the re-encode step for
/// any golden vector exercising the affected form.
///
/// Every golden vector round-trips byte-for-byte through the
/// disassembler's text representation: decoding, reformatting, and
/// reassembling any golden vector reproduces the exact original bytes.
#[test]
fn test_golden_vectors_full_roundtrip() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/vectors.json"))
            .expect("Failed to parse golden vectors");
    let tests = data["encoder_tests"].as_array().unwrap();
    let origin: u32 = 0x1000;
    let mut fails = Vec::new();
    let mut checked = 0usize;

    for t in tests {
        let name = t["name"].as_str().unwrap();
        let exp_hex = t["expected_hex"].as_str().unwrap_or("").to_lowercase();
        let cpu = t["cpu"].as_str().unwrap_or("68000");
        if exp_hex.is_empty() {
            continue;
        }

        let original_bytes = hex_to_bytes(&exp_hex);

        // Decode the golden bytes back into text.
        let mut stream = m68k_core::addressing::InstructionStream::new(&original_bytes, origin);
        let decoded_text = match m68k_disasm::decoder::decode_next(&mut stream, cpu) {
            Ok((_, m68k_disasm::decoder::DecodeResult::Instruction(inst))) => {
                inst.format(&std::collections::HashMap::new())
            }
            Ok((_, m68k_disasm::decoder::DecodeResult::DataWord(dw))) => {
                fails.push(format!(
                    "{} (cpu={}): decoded as data word instead of instruction: {}",
                    name,
                    cpu,
                    dw.format(&std::collections::HashMap::new())
                ));
                continue;
            }
            Err(e) => {
                fails.push(format!("{} (cpu={}): decode error: {}", name, cpu, e));
                continue;
            }
        };

        // Re-assemble the disassembler's own text rendering.
        let reasm_source = format!("    ORG ${:x}\n    {}\n", origin, decoded_text);
        let mut asm = Assembler::new(origin);
        asm.set_cpu(cpu);
        match asm.assemble_bytes(&reasm_source) {
            Ok(reencoded_bytes) => {
                let got_hex = bytes_to_hex(&reencoded_bytes);
                if got_hex != exp_hex {
                    fails.push(format!(
                        "{} (cpu={}): decoded '{}' but reencoding it gave {} (expected {})",
                        name,
                        cpu,
                        decoded_text.trim(),
                        got_hex,
                        exp_hex
                    ));
                } else {
                    checked += 1;
                }
            }
            Err(e) => {
                fails.push(format!(
                    "{} (cpu={}): decoded '{}' but reassembling it failed: {:?}",
                    name,
                    cpu,
                    decoded_text.trim(),
                    e
                ));
            }
        }
    }

    assert!(
        checked == tests.len() || !fails.is_empty(),
        "sanity: checked({}) should equal total vectors({}) when no failures",
        checked,
        tests.len()
    );

    if !fails.is_empty() {
        panic!(
            "Roundtrip failures ({}, {} passed):\n{}",
            fails.len(),
            checked,
            fails.join("\n")
        );
    }
}
