//! Diagnostics computation and error reporting for Motorola 68000 assembly files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use m68k_asm::assembler::Assembler;
use m68k_core::cpu_gate::{MinCpu, min_cpu_for_mnemonic};
use m68k_core::errors::Severity;

use crate::document::Document;

/// Target architecture configuration for linting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinterConfig {
    pub cpu: MinCpu,
    /// Extra `INCLUDE` search directories (the assembler's `-I`).
    pub include_paths: Vec<PathBuf>,
    /// Program whose assembly gives the diagnostics of every file it
    /// includes. Without it each file is assembled on its own, which
    /// reports the symbols and macros an included file takes from its
    /// includer as undefined.
    pub main_file: Option<PathBuf>,
}

impl Default for LinterConfig {
    fn default() -> Self {
        Self {
            cpu: MinCpu::Mc68000,
            include_paths: Vec::new(),
            main_file: None,
        }
    }
}

/// Compute diagnostics for a document assembled on its own.
pub fn compute_diagnostics(doc: &Document, config: &LinterConfig) -> Vec<Diagnostic> {
    let mut diagnostics = lint_lines(doc, config);

    // Assemble in memory for everything the per-line pass cannot see:
    // operands, branch distances, expressions, directives. Every error
    // and warning the assembler collected is reported, not just the one
    // it stopped on.
    let mut asm = new_assembler(config);
    // Relative INCLUDEs resolve against the file's own directory, as they
    // do when the CLI assembles it.
    if let Ok(path) = doc.uri.to_file_path()
        && let Some(dir) = path.parent()
    {
        asm.set_source_root(dir.to_path_buf());
    }
    let _ = asm.assemble(&doc.text);

    for diag in asm.diagnostics() {
        // The top-level source has no file name here (none was given to
        // the assembler); anything that names a file is in an included
        // file, which has no range in this document. Such a diagnostic is
        // shown at the top with its own location in the message.
        let (line_idx, message) = if diag.filename.is_empty() {
            (diag.line_no.unwrap_or(1).saturating_sub(1), diag.message)
        } else {
            let line = diag.line_no.map(|l| format!(":{l}")).unwrap_or_default();
            (0, format!("{}{}: {}", diag.filename, line, diag.message))
        };
        diagnostics.push(asm_diagnostic(doc, line_idx, diag.severity, message));
    }

    diagnostics
}

/// Diagnostics of every open document, from one assembly of `main` with
/// the open documents' text in place of their files. A document that is
/// neither `main` nor included by it is assembled on its own.
pub fn compute_unit_diagnostics(
    main: &Path,
    docs: &HashMap<Url, Document>,
    config: &LinterConfig,
) -> HashMap<Url, Vec<Diagnostic>> {
    let main = std::path::absolute(main).unwrap_or_else(|_| main.to_path_buf());
    let main_name = main.to_string_lossy().into_owned();
    let mut asm = new_assembler(config);
    if let Some(dir) = main.parent() {
        asm.set_source_root(dir.to_path_buf());
    }
    asm.errors.filename = main_name.clone();
    let paths: HashMap<&Url, PathBuf> = docs
        .iter()
        .filter_map(|(uri, doc)| {
            let path = uri.to_file_path().ok()?;
            let path = std::path::absolute(&path).unwrap_or(path);
            asm.set_file_text(path.clone(), doc.text.clone());
            Some((uri, path))
        })
        .collect();
    let main_text = asm
        .file_overlays
        .get(&main)
        .cloned()
        .or_else(|| std::fs::read_to_string(&main).ok())
        .unwrap_or_default();
    let _ = asm.assemble(&main_text);

    // Diagnostics name the file as the INCLUDE wrote it; the assembler
    // kept where that resolved to.
    let mut resolved: HashMap<String, PathBuf> = asm.include_paths_resolved.clone();
    resolved.insert(main_name, main);
    let mut by_file: HashMap<PathBuf, Vec<m68k_core::errors::Diagnostic>> = HashMap::new();
    for diag in asm.diagnostics() {
        if let Some(path) = resolved.get(&diag.filename) {
            by_file.entry(path.clone()).or_default().push(diag);
        }
    }

    docs.iter()
        .map(|(uri, doc)| {
            let Some(path) = paths
                .get(uri)
                .filter(|p| resolved.values().any(|r| r == *p))
            else {
                return (uri.clone(), compute_diagnostics(doc, config));
            };
            let mut diagnostics = lint_lines(doc, config);
            for diag in by_file.remove(path).unwrap_or_default() {
                let line_idx = diag.line_no.unwrap_or(1).saturating_sub(1);
                diagnostics.push(asm_diagnostic(doc, line_idx, diag.severity, diag.message));
            }
            (uri.clone(), diagnostics)
        })
        .collect()
}

fn new_assembler(config: &LinterConfig) -> Assembler {
    let mut asm = Assembler::new(0);
    asm.set_cpu(config.cpu.name());
    for dir in &config.include_paths {
        asm.add_include_path(dir.clone());
    }
    asm
}

/// An assembler diagnostic placed on a line of `doc`, from its first
/// non-blank character to its end.
fn asm_diagnostic(
    doc: &Document,
    line_idx: usize,
    severity: Severity,
    message: String,
) -> Diagnostic {
    let line_text = doc.lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
    let char_start = line_text.len().saturating_sub(line_text.trim_start().len()) as u32;
    let char_end = line_text.len() as u32;
    let (severity, code) = match severity {
        Severity::Error => (DiagnosticSeverity::ERROR, "ASM_ERR"),
        Severity::Warning => (DiagnosticSeverity::WARNING, "ASM_WARN"),
    };
    Diagnostic {
        range: Range {
            start: Position {
                line: line_idx as u32,
                character: char_start,
            },
            end: Position {
                line: line_idx as u32,
                character: char_end.max(char_start + 1),
            },
        },
        severity: Some(severity),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            code.to_string(),
        )),
        source: Some("m68k-lsp".to_string()),
        message,
        ..Default::default()
    }
}

/// Fast per-line checks: CPU gating of mnemonics and custom chip
/// register use.
fn lint_lines(doc: &Document, config: &LinterConfig) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (line_idx, parsed) in doc.parsed_lines.iter().enumerate() {
        let raw = &parsed.raw;
        let line_num = line_idx as u32;

        if parsed.mnemonic.is_empty() {
            continue;
        }

        // Check CPU-level requirement for mnemonic
        if let Some(req_cpu) = min_cpu_for_mnemonic(&parsed.mnemonic) {
            let req_level = match req_cpu {
                MinCpu::Mc68000 => 0,
                MinCpu::Mc68010 => 1,
                MinCpu::Mc68020 => 2,
                MinCpu::Mc68030 => 3,
                MinCpu::Mc68040 => 4,
                MinCpu::Mc68060 => 5,
            };
            let current_level = match config.cpu {
                MinCpu::Mc68000 => 0,
                MinCpu::Mc68010 => 1,
                MinCpu::Mc68020 => 2,
                MinCpu::Mc68030 => 3,
                MinCpu::Mc68040 => 4,
                MinCpu::Mc68060 => 5,
            };

            if req_level > current_level {
                let col_start = raw.to_ascii_lowercase().find(&parsed.mnemonic).unwrap_or(0) as u32;
                let col_end = col_start + parsed.mnemonic.len() as u32;

                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: line_num,
                            character: col_start,
                        },
                        end: Position {
                            line: line_num,
                            character: col_end,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        "CPU_LEVEL".to_string(),
                    )),
                    source: Some("m68k-lsp".to_string()),
                    message: format!(
                        "Instruction '{}' requires {} or later (target configured for {})",
                        parsed.mnemonic.to_ascii_uppercase(),
                        req_cpu.name(),
                        config.cpu.name()
                    ),
                    ..Default::default()
                });
            }
        }
    }

    diagnostics.extend(crate::custom_regs::diagnostics(doc));
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    /// An included file checked as part of its main program sees the
    /// includer's symbols; its own faults land on its own lines, and an
    /// unsaved buffer counts instead of the file on disk.
    #[test]
    fn included_file_is_checked_within_its_main_program() {
        let dir = std::env::temp_dir().join(format!("m68k-lsp-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("main.s");
        let inc = dir.join("inc.s");
        std::fs::write(&main, "SIZE equ 4\n    include \"inc.s\"\n    rts\n").unwrap();
        std::fs::write(&inc, "    moveq #SIZE,d0\n").unwrap();

        let inc_uri = Url::from_file_path(&inc).unwrap();
        // Standalone, SIZE is unknown.
        let doc = Document::new(inc_uri.clone(), 1, "    moveq #SIZE,d0\n".to_string());
        assert!(!compute_diagnostics(&doc, &LinterConfig::default()).is_empty());

        // The unsaved buffer adds a bad line below the good one.
        let doc = Document::new(
            inc_uri.clone(),
            2,
            "    moveq #SIZE,d0\n    bogus d0\n".to_string(),
        );
        let docs = HashMap::from([(inc_uri.clone(), doc)]);
        let all = compute_unit_diagnostics(&main, &docs, &LinterConfig::default());
        let diags = &all[&inc_uri];
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].range.start.line, 1);
        assert!(diags[0].message.contains("BOGUS"), "{}", diags[0].message);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_valid_source_has_no_error_diagnostics() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    move.w  #1,d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());

        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
    }

    #[test]
    fn test_cpu_gate_warning() {
        let uri = Url::parse("file:///test.s").unwrap();
        // extb requires 68020+
        let src = "    extb.l  d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(
            &doc,
            &LinterConfig {
                cpu: MinCpu::Mc68000,
                ..Default::default()
            },
        );

        assert!(!diags.is_empty());
        assert!(diags.iter().any(|d| d.message.contains("requires 68020")));
    }

    #[test]
    fn every_error_line_is_reported_not_just_the_first() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src =
            "    move.l  d0,nowhere\n    nop\n    bra.s   far\n    dcb.b   300,0\nfar:\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());
        let error_lines: Vec<u32> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(error_lines, vec![0, 2], "{diags:?}");
    }

    #[test]
    fn assembler_warnings_are_reported_as_warnings() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    moveq   #200,d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::WARNING)
                    && d.range.start.line == 0
                    && d.message.contains("MOVEQ")),
            "{diags:?}"
        );
    }
}
