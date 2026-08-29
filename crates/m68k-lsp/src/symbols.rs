//! Document symbol provider for outline view and symbol navigation.

use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Position, Range, SymbolKind};

use crate::document::{Document, SymbolDefKind};

/// Compute document symbol outline.
pub fn compute_document_symbols(doc: &Document) -> DocumentSymbolResponse {
    let mut symbols = Vec::new();

    for sym in doc.symbols.values() {
        let kind = match sym.kind {
            SymbolDefKind::Label => SymbolKind::FUNCTION,
            SymbolDefKind::Equate => SymbolKind::CONSTANT,
            SymbolDefKind::Macro => SymbolKind::STRUCT,
            SymbolDefKind::Section => SymbolKind::MODULE,
        };

        #[allow(deprecated)]
        symbols.push(DocumentSymbol {
            name: sym.name.clone(),
            detail: sym.value_str.clone(),
            kind,
            tags: None,
            deprecated: None,
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
            selection_range: Range {
                start: Position {
                    line: sym.line_idx as u32,
                    character: sym.character as u32,
                },
                end: Position {
                    line: sym.line_idx as u32,
                    character: (sym.character + sym.name.len()) as u32,
                },
            },
            children: None,
        });
    }

    DocumentSymbolResponse::Nested(symbols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_document_symbols() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    SECTION Code,code\nSTART:\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let res = compute_document_symbols(&doc);

        if let DocumentSymbolResponse::Nested(syms) = res {
            assert!(syms.iter().any(|s| s.name == "START"));
        } else {
            panic!("Expected nested symbols");
        }
    }
}
