//! The project build pipeline.
//!
//! Assembles the manifest's entry file — with include directories and a
//! source root, so `INCLUDE` across files resolves — and emits the
//! artifact the manifest asks for, into `build/`.

use std::path::PathBuf;

use serde::Serialize;

use m68k_asm::amiga_hunk_writer::generate_hunk_exe;
use m68k_asm::assembler::Assembler;
use m68k_asm::output::generate_binary;
use m68k_asm::output::{generate_intel_hex, generate_srecord};

use super::{OutputFormat, Project};

/// A diagnostic tied to a source location.
#[derive(Debug, Clone, Serialize)]
pub struct BuildDiagnostic {
    /// Project-relative file the diagnostic belongs to.
    pub file: String,
    /// 1-based line number; 0 when the error carries no location.
    pub line: usize,
    pub message: String,
    /// `error` or `warning`.
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildResult {
    pub success: bool,
    /// Project-relative path of the artifact, when one was produced.
    pub artifact: Option<String>,
    /// Absolute artifact path, for handing to an emulator.
    pub artifact_path: Option<String>,
    pub byte_count: usize,
    pub diagnostics: Vec<BuildDiagnostic>,
    /// Symbol table of the finished build, for the map view.
    pub symbols: Vec<SymbolInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub address: u32,
    pub section: Option<String>,
}

impl BuildResult {
    fn failure(file: &str, line: usize, message: String) -> Self {
        Self {
            success: false,
            artifact: None,
            artifact_path: None,
            byte_count: 0,
            diagnostics: vec![BuildDiagnostic {
                file: file.to_string(),
                line,
                message,
                severity: "error".to_string(),
            }],
            symbols: Vec::new(),
        }
    }
}

/// Assemble the project and write its artifact to `build/`.
pub fn build_project(project: &Project) -> BuildResult {
    let manifest = project.manifest();
    let main_rel = manifest.build.main.clone();

    let main_path = match project.main_file() {
        Ok(p) => p,
        Err(e) => return BuildResult::failure(&main_rel, 0, e.to_string()),
    };
    let source = match std::fs::read_to_string(&main_path) {
        Ok(s) => s,
        Err(e) => {
            return BuildResult::failure(&main_rel, 0, format!("{main_rel} nicht lesbar: {e}"));
        }
    };
    let origin = match manifest.origin_address() {
        Ok(o) => o,
        Err(e) => return BuildResult::failure(&main_rel, 0, e),
    };

    let mut asm = Assembler::new(origin);
    asm.set_cpu(&manifest.build.cpu);
    // Without a source root, a relative INCLUDE resolves against the
    // server's working directory rather than the project — the reason
    // multi-file projects could not build before.
    if let Some(parent) = main_path.parent() {
        asm.set_source_root(parent.to_path_buf());
    }
    for dir in project.include_dirs() {
        asm.add_include_path(dir);
    }

    if let Err(e) = asm.assemble(&source) {
        return BuildResult::failure(&main_rel, e.line_no.unwrap_or(0), e.message);
    }

    let symbols = collect_symbols(&asm);

    let format = manifest.build.format;
    let bytes = match render_artifact(&asm, format) {
        Ok(b) => b,
        Err(e) => return BuildResult::failure(&main_rel, 0, e),
    };

    let build_dir = match project.build_dir() {
        Ok(d) => d,
        Err(e) => return BuildResult::failure(&main_rel, 0, e),
    };
    let file_name = artifact_name(manifest.output_name(), format);
    let out_path: PathBuf = build_dir.join(&file_name);

    if let Err(e) = std::fs::write(&out_path, &bytes) {
        return BuildResult::failure(&main_rel, 0, format!("Artefakt nicht schreibbar: {e}"));
    }

    BuildResult {
        success: true,
        artifact: Some(format!("build/{file_name}")),
        artifact_path: Some(out_path.to_string_lossy().to_string()),
        byte_count: bytes.len(),
        diagnostics: Vec::new(),
        symbols,
    }
}

/// Artifact file name for a format. Hunk executables carry no extension,
/// the way AmigaOS binaries are named.
fn artifact_name(base: &str, format: OutputFormat) -> String {
    let ext = format.extension();
    if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    }
}

fn collect_symbols(asm: &Assembler) -> Vec<SymbolInfo> {
    let mut out: Vec<SymbolInfo> = asm
        .symbols
        .iter()
        .filter(|(_, e)| e.defined)
        .map(|(name, e)| SymbolInfo {
            name: name.clone(),
            address: e.value,
            section: e.section.clone(),
        })
        .collect();
    out.sort_by_key(|s| s.address);
    out
}

/// Turn the assembled program into the requested container format.
fn render_artifact(asm: &Assembler, format: OutputFormat) -> Result<Vec<u8>, String> {
    match format {
        OutputFormat::Hunk => {
            let hunk = generate_hunk_exe(&asm.sections, &asm.symbols);
            if hunk.is_empty() {
                return Err(
                    "Hunk-Ausgabe leer: das Programm enthält keine nicht-leere SECTION.".into(),
                );
            }
            Ok(hunk)
        }
        OutputFormat::Adf => {
            let (code, _) = generate_binary(&asm.code)
                .ok_or_else(|| "Programm enthält keinen Code für die ADF.".to_string())?;
            crate::amiga::build_bootable_adf(&code)
        }
        OutputFormat::Binary => {
            let (code, _) = generate_binary(&asm.code)
                .ok_or_else(|| "Programm enthält keinen Code.".to_string())?;
            Ok(code)
        }
        OutputFormat::SRecord => Ok(generate_srecord(&asm.code, "m68k").into_bytes()),
        OutputFormat::IntelHex => Ok(generate_intel_hex(&asm.code).into_bytes()),
    }
}
