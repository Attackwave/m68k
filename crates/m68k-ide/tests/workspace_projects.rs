//! End-to-end tests for the on-disk project model: create a project,
//! build it, and confirm a real artifact lands in `build/`.

use m68k_ide::commands::workspace::{
    CreateProjectRequest, ProjectPathRequest, ReadFileRequest, WriteFileRequest, build,
    create_project, list_files, read_file, write_file,
};

/// Each test gets its own directory, removed first so a previous run
/// cannot make a later one pass or fail spuriously.
fn fresh_dir(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("m68k_ide_ws_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir.to_string_lossy().to_string()
}

fn create(name: &str, template: &str) -> String {
    let root = fresh_dir(name);
    let res = create_project(CreateProjectRequest {
        root: root.clone(),
        name: name.to_string(),
        template: template.to_string(),
    })
    .expect("project should be created");
    assert_eq!(res.manifest.name, name);
    root
}

#[test]
fn creates_a_project_on_disk_with_manifest_and_sources() {
    let root = create("amiga_create", "amiga-assembly");
    let dir = std::path::Path::new(&root);

    assert!(dir.join("m68k.json").is_file(), "manifest missing");
    assert!(dir.join("src/main.s").is_file(), "entry source missing");
    assert!(dir.join("includes/custom.i").is_file(), "include missing");
    // Build output must not dirty version control from the first build.
    assert!(dir.join(".gitignore").is_file(), ".gitignore missing");
}

#[test]
fn refuses_to_overwrite_an_existing_project() {
    let root = create("amiga_twice", "amiga-assembly");
    let again = create_project(CreateProjectRequest {
        root,
        name: "amiga_twice".into(),
        template: "amiga-assembly".into(),
    });
    assert!(
        again.is_err(),
        "creating over an existing project must fail"
    );
}

#[test]
fn amiga_assembly_template_builds_a_bootable_adf() {
    let root = create("amiga_build", "amiga-assembly");

    let result = build(ProjectPathRequest { root: root.clone() }).expect("build should run");
    assert!(
        result.success,
        "template must assemble cleanly: {:?}",
        result.diagnostics
    );

    let artifact = result.artifact.expect("artifact path");
    assert_eq!(artifact, "build/amiga_build.adf");

    let bytes = std::fs::read(result.artifact_path.unwrap()).expect("artifact readable");
    assert_eq!(bytes.len(), 901_120, "an ADF is always 880 KB");
    assert_eq!(&bytes[0..4], b"DOS\0");
    assert!(
        m68k_floppy::adf_writer::verify_bootblock_checksum(&bytes).unwrap(),
        "Kickstart rejects a bootblock with a bad checksum"
    );
    // The build must find labels, not just emit bytes.
    assert!(
        result.symbols.iter().any(|s| s.name == "Start"),
        "symbol table should list Start"
    );
}

#[test]
fn hunk_template_builds_an_amigaos_executable() {
    let root = create("amiga_hunk", "amiga-hunk");

    let result = build(ProjectPathRequest { root }).expect("build should run");
    assert!(result.success, "{:?}", result.diagnostics);

    let bytes = std::fs::read(result.artifact_path.unwrap()).expect("artifact readable");
    // HUNK_HEADER is $3F3; LoadSeg() checks for it first.
    assert_eq!(&bytes[0..4], &[0x00, 0x00, 0x03, 0xF3], "not a Hunk file");
    // A Hunk executable carries no extension, like a real AmigaOS binary.
    assert_eq!(result.artifact.unwrap(), "build/amiga_hunk");
}

#[test]
fn baremetal_template_builds_a_flat_binary_at_its_origin() {
    let root = create("baremetal_build", "baremetal");
    let result = build(ProjectPathRequest { root }).expect("build should run");
    assert!(result.success, "{:?}", result.diagnostics);
    assert!(result.byte_count > 0);
    assert_eq!(result.artifact.unwrap(), "build/baremetal_build.bin");
}

#[test]
fn build_reports_the_failing_line_instead_of_succeeding() {
    let root = create("bad_source", "baremetal");
    write_file(WriteFileRequest {
        root: root.clone(),
        path: "src/main.s".into(),
        contents: "Start:\n    move.w  d0,d1\n    nonsense.q  d0,d1\n".into(),
    })
    .expect("write should succeed");

    let result = build(ProjectPathRequest { root }).expect("build should run");
    assert!(!result.success, "a bogus mnemonic must not build");
    assert!(!result.diagnostics.is_empty(), "error needs a diagnostic");
    assert_eq!(
        result.diagnostics[0].line, 3,
        "diagnostic must point at the offending line"
    );
}

#[test]
fn multi_file_include_resolves_against_the_project() {
    // The single-string build path could not do this at all: a relative
    // INCLUDE resolved against the server's working directory.
    let root = create("include_build", "amiga-assembly");
    write_file(WriteFileRequest {
        root: root.clone(),
        path: "includes/extra.i".into(),
        contents: "MY_CONSTANT EQU $1234\n".into(),
    })
    .unwrap();
    write_file(WriteFileRequest {
        root: root.clone(),
        path: "src/main.s".into(),
        contents: "    INCLUDE \"extra.i\"\nStart:\n    move.w  #MY_CONSTANT,d0\n    rts\n".into(),
    })
    .unwrap();

    let result = build(ProjectPathRequest { root }).expect("build should run");
    assert!(
        result.success,
        "INCLUDE from includes/ must resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn file_tree_lists_sources_and_hides_build_output() {
    let root = create("tree_listing", "amiga-assembly");
    build(ProjectPathRequest { root: root.clone() }).unwrap();

    let files = list_files(ProjectPathRequest { root }).expect("tree should list");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

    assert!(paths.contains(&"src/main.s"), "{paths:?}");
    assert!(paths.contains(&"includes/custom.i"), "{paths:?}");
    assert!(
        !paths.iter().any(|p| p.starts_with("build")),
        "build/ is output, not source: {paths:?}"
    );
}

#[test]
fn reads_back_what_was_written() {
    let root = create("read_write", "baremetal");
    write_file(WriteFileRequest {
        root: root.clone(),
        path: "src/notes.s".into(),
        contents: "; scratch\n".into(),
    })
    .unwrap();

    let back = read_file(ReadFileRequest {
        root,
        path: "src/notes.s".into(),
    })
    .expect("file should read back");
    assert_eq!(back, "; scratch\n");
}

#[test]
fn file_api_refuses_paths_outside_the_project() {
    let root = create("escape_guard", "baremetal");

    // The server runs with the user's full filesystem rights, so this
    // is a trust boundary, not a tidiness rule.
    assert!(
        read_file(ReadFileRequest {
            root: root.clone(),
            path: "../../../etc/passwd".into(),
        })
        .is_err(),
        "traversal must be rejected"
    );
    assert!(
        write_file(WriteFileRequest {
            root,
            path: "/tmp/m68k_escape_probe".into(),
            contents: "x".into(),
        })
        .is_err(),
        "absolute paths must be rejected"
    );
    assert!(
        !std::path::Path::new("/tmp/m68k_escape_probe").exists(),
        "rejected write must not have touched the filesystem"
    );
}
