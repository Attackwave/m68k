//! Golden vector tests using the 2-pass assembler end-to-end.

use m68k_asm::assembler::Assembler;

fn assemble(source: &str, cpu: &str, origin: u32) -> Result<Vec<u8>, String> {
    let mut asm = Assembler::new(origin);
    asm.set_cpu(cpu);
    // Golden vectors that reference `label` expect it to name the branch
    // instruction's own address (branch-to-self), matching how the Python
    // reference generates these vectors.
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
