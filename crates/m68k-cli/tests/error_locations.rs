//! CLI regressions for source locations after preprocessing and for labels
//! whose names also happen to be assembler directives.

use std::path::Path;
use std::process::{Command, Output};

fn assemble(source: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m68k-asm"))
        .arg(source)
        .arg("-o")
        .arg(output)
        .output()
        .expect("run m68k-asm")
}

#[test]
fn error_after_rept_in_include_names_its_real_file_and_line() {
    let dir = std::env::temp_dir().join(format!("m68k-include-error-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("one.s");
    let included = dir.join("two.s");
    let binary = dir.join("out.bin");
    std::fs::write(
        &main,
        "    ORG 0\n    NOP\n    INCLUDE \"two.s\"\n    NOP\n",
    )
    .unwrap();

    std::fs::write(
        &included,
        "a:\n    REPT 100\n    NOP\n    ENDR\n    BNE.S a\n",
    )
    .unwrap();
    let result = assemble(&main, &binary);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "out-of-range branch was accepted");
    assert!(
        stderr.contains("two.s:5: bne.s target is out of range"),
        "{stderr}"
    );

    // A pass-2 operand error after the same expansion must also point at
    // the included file, rather than at a line in the top-level source.
    std::fs::write(
        &included,
        "a:\n    REPT 3\n    NOP\n    ENDR\n    JMP nowhere\n",
    )
    .unwrap();
    let result = assemble(&main, &binary);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "undefined symbol was accepted");
    assert!(
        stderr.contains("two.s:5: undefined symbol: nowhere"),
        "{stderr}"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sized_branches_can_target_a_label_named_fail() {
    let dir = std::env::temp_dir().join(format!("m68k-fail-label-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("branch.s");
    let binary = dir.join("branch.bin");

    for (mnemonic, opcode) in [("BRA.S", 0x60), ("BNE.S", 0x66), ("BPL.S", 0x6a)] {
        std::fs::write(
            &source,
            format!("    ORG $1000\n    {mnemonic} fail\n    NOP\nfail: RTS\n"),
        )
        .unwrap();
        let result = assemble(&source, &binary);
        assert!(
            result.status.success(),
            "{mnemonic}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            std::fs::read(&binary).unwrap(),
            [opcode, 0x02, 0x4e, 0x71, 0x4e, 0x75]
        );
    }

    std::fs::remove_dir_all(dir).unwrap();
}
