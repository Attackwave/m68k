//! Workspace-wide symbol indexing and cross-file INCLUDE resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::document::Document;

/// Workspace index storing symbols across multiple files.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceIndex {
    /// Cached documents by file path.
    pub documents: HashMap<PathBuf, Document>,
    /// Search directories for `INCLUDE` files.
    pub include_dirs: Vec<PathBuf>,
}

impl WorkspaceIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an include search path (e.g. from compiler -I flags).
    pub fn add_include_dir(&mut self, dir: PathBuf) {
        if !self.include_dirs.contains(&dir) {
            self.include_dirs.push(dir);
        }
    }

    /// Index or update a document in the workspace.
    pub fn update_document(&mut self, uri: &Url, text: String, version: i32) {
        let path = uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()));
        let doc = Document::new(uri.clone(), version, text);
        self.documents.insert(path, doc);
    }

    /// Remove a document from the workspace index.
    pub fn remove_document(&mut self, uri: &Url) {
        let path = uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()));
        self.documents.remove(&path);
    }

    /// Resolve an `INCLUDE "file.i"` path relative to the current document and search dirs.
    pub fn resolve_include(&self, current_doc_path: &Path, include_name: &str) -> Option<PathBuf> {
        let clean_name = include_name.trim_matches('"').trim_matches('\'');

        // 1. Try relative to current document's directory
        if let Some(parent) = current_doc_path.parent() {
            let candidate = parent.join(clean_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // 2. Try include search directories
        for dir in &self.include_dirs {
            let candidate = dir.join(clean_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }

    /// Search for a symbol definition across the entire workspace.
    pub fn find_symbol_definition(
        &self,
        current_doc: &Document,
        symbol_name: &str,
    ) -> Option<Location> {
        let clean_name = symbol_name.trim();

        // 1. Check current document first
        if let Some(sym) = current_doc.symbols.get(clean_name) {
            return Some(Location {
                uri: current_doc.uri.clone(),
                range: Range {
                    start: Position {
                        line: sym.line_idx as u32,
                        character: sym.character as u32,
                    },
                    end: Position {
                        line: sym.line_idx as u32,
                        character: (sym.character + sym.name.len()) as u32,
                    },
                },
            });
        }

        // 2. Check explicitly included files
        let current_path = current_doc
            .uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(current_doc.uri.path()));

        for parsed in &current_doc.parsed_lines {
            if parsed.mnemonic.eq_ignore_ascii_case("include")
                && !parsed.operands.is_empty()
                && let Some(inc_path) = self.resolve_include(&current_path, &parsed.operands[0])
                && let Some(inc_doc) = self.documents.get(&inc_path)
                && let Some(sym) = inc_doc.symbols.get(clean_name)
            {
                return Some(Location {
                    uri: inc_doc.uri.clone(),
                    range: Range {
                        start: Position {
                            line: sym.line_idx as u32,
                            character: sym.character as u32,
                        },
                        end: Position {
                            line: sym.line_idx as u32,
                            character: (sym.character + sym.name.len()) as u32,
                        },
                    },
                });
            }
        }

        // 3. Fallback: Search all open workspace documents
        for doc in self.documents.values() {
            if doc.uri != current_doc.uri
                && let Some(sym) = doc.symbols.get(clean_name)
            {
                return Some(Location {
                    uri: doc.uri.clone(),
                    range: Range {
                        start: Position {
                            line: sym.line_idx as u32,
                            character: sym.character as u32,
                        },
                        end: Position {
                            line: sym.line_idx as u32,
                            character: (sym.character + sym.name.len()) as u32,
                        },
                    },
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_cross_file_definition() {
        let mut ws = WorkspaceIndex::new();
        let base_dir = std::env::temp_dir().join("m68k_lsp_test");
        let path1 = base_dir.join("header.i");
        let path2 = base_dir.join("main.s");

        let uri1 = Url::from_file_path(&path1)
            .unwrap_or_else(|_| Url::parse("file:///workspace/header.i").unwrap());
        let uri2 = Url::from_file_path(&path2)
            .unwrap_or_else(|_| Url::parse("file:///workspace/main.s").unwrap());

        ws.update_document(&uri1, "CUSTOM_REG EQU $DFF000\n".to_string(), 1);
        ws.update_document(
            &uri2,
            "    INCLUDE \"header.i\"\n    move.l CUSTOM_REG,a0\n".to_string(),
            1,
        );

        let main_doc = Document::new(
            uri2,
            1,
            "    INCLUDE \"header.i\"\n    move.l CUSTOM_REG,a0\n".to_string(),
        );

        let loc = ws
            .find_symbol_definition(&main_doc, "CUSTOM_REG")
            .expect("should resolve symbol from header.i");

        assert_eq!(loc.uri, uri1);
        assert_eq!(loc.range.start.line, 0);
    }
}
