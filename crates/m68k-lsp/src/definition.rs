//! Definition provider for jumping to labels, constants, and macros.

use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range};

use crate::document::Document;
use crate::workspace::WorkspaceIndex;

/// Find definition location for symbol under cursor.
pub fn compute_definition(
    doc: &Document,
    pos: Position,
    workspace: Option<&WorkspaceIndex>,
) -> Option<GotoDefinitionResponse> {
    let word_info = doc.get_word_at_position(pos)?;
    let clean_word = word_info.word.trim();
    if clean_word.is_empty() {
        return None;
    }

    // Lookup in symbols map
    if let Some(sym) = doc.symbols.get(clean_word) {
        let loc = Location {
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
        };
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // Lookup in macros
    if let Some(m) = doc.macros.get(&clean_word.to_ascii_lowercase()) {
        let loc = Location {
            uri: doc.uri.clone(),
            range: Range {
                start: Position {
                    line: m.line_idx as u32,
                    character: m.character as u32,
                },
                end: Position {
                    line: m.line_idx as u32,
                    character: (m.character + m.name.len()) as u32,
                },
            },
        };
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // Lookup in workspace index if available
    if let Some(ws) = workspace
        && let Some(loc) = ws.find_symbol_definition(doc, clean_word)
    {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_goto_definition() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    bra     TargetLabel\n\nTargetLabel:\n    rts\n";
        let doc = Document::new(uri.clone(), 1, src.to_string());

        let res = compute_definition(
            &doc,
            Position {
                line: 1,
                character: 14,
            },
            None,
        )
        .expect("should find definition");

        if let GotoDefinitionResponse::Scalar(loc) = res {
            assert_eq!(loc.uri, uri);
            assert_eq!(loc.range.start.line, 3);
        } else {
            panic!("Expected scalar location");
        }
    }
}
