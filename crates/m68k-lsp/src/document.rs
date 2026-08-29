//! Document state management, line indexing, and token position resolution.

use std::collections::HashMap;
use tower_lsp::lsp_types::{Position, Range, Url};

use m68k_core::tokens::{is_local_label, is_mnemonic, is_valid_ident, split_line};

/// The semantic classification of a word under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordKind {
    Mnemonic,
    Directive,
    Register,
    Lvo,
    Label,
    Unknown,
}

/// Information about a word located at a specific text position.
#[derive(Debug, Clone)]
pub struct WordAtPosition {
    pub word: String,
    pub range: Range,
    pub kind: WordKind,
    pub line_idx: usize,
    pub raw_line: String,
    pub mnemonic: String,
    pub size: String,
    pub operands: Vec<String>,
}

/// A parsed representation of a source line for fast IDE queries.
#[derive(Debug, Clone)]
pub struct ParsedSourceLine {
    pub line_idx: usize,
    pub label: Option<String>,
    pub mnemonic: String,
    pub size: String,
    pub operands: Vec<String>,
    pub raw: String,
}

/// A discovered symbol in the document (label, equate, macro, section).
#[derive(Debug, Clone)]
pub struct SymbolDef {
    pub name: String,
    pub line_idx: usize,
    pub character: usize,
    pub kind: SymbolDefKind,
    pub value_str: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolDefKind {
    Label,
    Equate,
    Macro,
    Section,
}

/// An open text document managed by the language server.
#[derive(Debug, Clone)]
pub struct Document {
    pub uri: Url,
    pub version: i32,
    pub text: String,
    pub lines: Vec<String>,
    pub parsed_lines: Vec<ParsedSourceLine>,
    pub symbols: HashMap<String, SymbolDef>,
    pub macros: HashMap<String, SymbolDef>,
}

impl Document {
    /// Create a new document and parse its symbols.
    pub fn new(uri: Url, version: i32, text: String) -> Self {
        let mut doc = Self {
            uri,
            version,
            text: String::new(),
            lines: Vec::new(),
            parsed_lines: Vec::new(),
            symbols: HashMap::new(),
            macros: HashMap::new(),
        };
        doc.update_text(version, text);
        doc
    }

    /// Update the document text and rebuild symbol indices.
    pub fn update_text(&mut self, version: i32, text: String) {
        self.version = version;
        self.text = text;
        self.lines = self.text.lines().map(|s| s.to_string()).collect();
        // Ensure at least an empty line if text is empty
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }

        self.parsed_lines.clear();
        self.symbols.clear();
        self.macros.clear();

        let mut current_global_label: Option<String> = None;

        for (idx, raw_line) in self.lines.iter().enumerate() {
            let (label, mnemonic, size, operands) = split_line(raw_line);

            if let Some(ref lbl) = label {
                let char_idx = raw_line.find(lbl).unwrap_or(0);
                let is_local = is_local_label(lbl);

                let qualified_name = if is_local {
                    if let Some(ref global) = current_global_label {
                        format!("{}@{}", global, lbl.trim_start_matches('.'))
                    } else {
                        lbl.clone()
                    }
                } else {
                    current_global_label = Some(lbl.clone());
                    lbl.clone()
                };

                let sym_kind = if mnemonic.eq_ignore_ascii_case("equ") || mnemonic == "=" {
                    SymbolDefKind::Equate
                } else if mnemonic.eq_ignore_ascii_case("macro") {
                    SymbolDefKind::Macro
                } else if mnemonic.eq_ignore_ascii_case("section") {
                    SymbolDefKind::Section
                } else {
                    SymbolDefKind::Label
                };

                let value_str = if sym_kind == SymbolDefKind::Equate && !operands.is_empty() {
                    Some(operands.join(", "))
                } else {
                    None
                };

                let sym = SymbolDef {
                    name: lbl.clone(),
                    line_idx: idx,
                    character: char_idx,
                    kind: sym_kind,
                    value_str,
                };

                if sym_kind == SymbolDefKind::Macro {
                    self.macros.insert(lbl.to_ascii_lowercase(), sym.clone());
                }
                self.symbols.insert(qualified_name.clone(), sym.clone());
                if is_local {
                    // Also allow lookup by unqualified local label within same file
                    self.symbols.insert(lbl.clone(), sym);
                }
            }

            self.parsed_lines.push(ParsedSourceLine {
                line_idx: idx,
                label,
                mnemonic,
                size,
                operands,
                raw: raw_line.clone(),
            });
        }
    }

    /// Retrieve the word and its semantic context at a given position.
    pub fn get_word_at_position(&self, pos: Position) -> Option<WordAtPosition> {
        let line_idx = pos.line as usize;
        let char_idx = pos.character as usize;

        let raw_line = self.lines.get(line_idx)?;
        if raw_line.is_empty() {
            return None;
        }

        // Find word boundary at char_idx
        let chars: Vec<char> = raw_line.chars().collect();
        if char_idx > chars.len() {
            return None;
        }

        let is_word_char =
            |c: char| c.is_alphanumeric() || c == '_' || c == '.' || c == '$' || c == '@';

        let mut start = char_idx;
        while start > 0 && start <= chars.len() && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        let mut end = char_idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        if start == end {
            return None;
        }

        let word: String = chars[start..end].iter().collect();
        let parsed = self.parsed_lines.get(line_idx)?;

        let kind = self.classify_word(&word, parsed);

        Some(WordAtPosition {
            word,
            range: Range {
                start: Position {
                    line: pos.line,
                    character: start as u32,
                },
                end: Position {
                    line: pos.line,
                    character: end as u32,
                },
            },
            kind,
            line_idx,
            raw_line: raw_line.clone(),
            mnemonic: parsed.mnemonic.clone(),
            size: parsed.size.clone(),
            operands: parsed.operands.clone(),
        })
    }

    fn classify_word(&self, word: &str, parsed: &ParsedSourceLine) -> WordKind {
        let lower = word.to_ascii_lowercase();

        if is_register(&lower) {
            return WordKind::Register;
        }

        if lower.starts_with("_lvo") {
            return WordKind::Lvo;
        }

        if parsed.mnemonic.eq_ignore_ascii_case(word)
            || parsed
                .mnemonic
                .eq_ignore_ascii_case(word.split('.').next().unwrap_or(word))
        {
            if is_mnemonic(&parsed.mnemonic) {
                return WordKind::Mnemonic;
            }
            if crate::analysis::directives_docs::lookup_directive(&parsed.mnemonic).is_some() {
                return WordKind::Directive;
            }
        }

        if is_mnemonic(&lower) {
            return WordKind::Mnemonic;
        }

        if crate::analysis::directives_docs::lookup_directive(&lower).is_some() {
            return WordKind::Directive;
        }

        if is_valid_ident(word) {
            return WordKind::Label;
        }

        WordKind::Unknown
    }
}

/// Check if a lowercase string is a 68k register name.
pub fn is_register(s: &str) -> bool {
    matches!(
        s,
        "d0" | "d1"
            | "d2"
            | "d3"
            | "d4"
            | "d5"
            | "d6"
            | "d7"
            | "a0"
            | "a1"
            | "a2"
            | "a3"
            | "a4"
            | "a5"
            | "a6"
            | "a7"
            | "sp"
            | "pc"
            | "sr"
            | "ccr"
            | "usp"
            | "ssp"
            | "msp"
            | "isp"
            | "vbr"
            | "cacr"
            | "caar"
            | "sfc"
            | "dfc"
            | "fp0"
            | "fp1"
            | "fp2"
            | "fp3"
            | "fp4"
            | "fp5"
            | "fp6"
            | "fp7"
            | "fpcr"
            | "fpsr"
            | "fpiar"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_parsing() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    move.w  #1,d0\n.loop:\n    dbra    d0,.loop\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());

        assert_eq!(doc.lines.len(), 5);
        assert!(doc.symbols.contains_key("START"));
        assert!(doc.symbols.contains_key(".loop"));
    }

    #[test]
    fn test_word_at_position() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    move.w  #$1234,d0\n";
        let doc = Document::new(uri, 1, src.to_string());

        let word_info = doc
            .get_word_at_position(Position {
                line: 0,
                character: 6,
            })
            .expect("should find word");

        assert_eq!(word_info.word, "move.w");
        assert_eq!(word_info.kind, WordKind::Mnemonic);
    }
}
