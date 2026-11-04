//! The example programs under `examples/` assemble against the system
//! includes under `include/`, so a change to either is caught here.

use std::path::{Path, PathBuf};

use m68k_asm::assembler::Assembler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

#[test]
fn every_example_assembles_against_the_system_includes() {
    let root = repo_root();
    let mut found = 0;
    for system in std::fs::read_dir(root.join("examples")).expect("examples/") {
        let dir = system.expect("entry").path();
        for entry in std::fs::read_dir(&dir).expect("system directory") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("s") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("source");
            let mut asm = Assembler::new(0);
            asm.set_cpu("68000");
            asm.add_include_path(root.join("include"));
            let bytes = asm
                .assemble_bytes(&source)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.message));
            assert!(!bytes.is_empty(), "{}", path.display());
            found += 1;
        }
    }
    assert!(found >= 3, "expected the system examples, found {found}");
}
