//! Reference search provider (Find All References) for labels, constants, and macros.

use tower_lsp::lsp_types::{Location, Position, Range};

use crate::document::{Document, byte_to_utf16_col};

/// Compute all reference locations for the symbol under the cursor.
pub fn compute_references(
    doc: &Document,
    pos: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let mut locations = Vec::new();

    let word_info = match doc.get_word_at_position(pos) {
        Some(w) => w,
        None => return locations,
    };

    let target = word_info.word.trim();
    if target.is_empty() {
        return locations;
    }
    // A local label is the same symbol only within its own scope.
    let key = doc.key_at(target, word_info.line_idx);
    let declaration = doc.symbols.get(&key);
    let is_ident = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '$');

    for (line_idx, line) in doc.lines.iter().enumerate() {
        if doc.key_at(target, line_idx) != key {
            continue;
        }
        let code_end = code_length(line);
        for (actual_idx, _) in line.match_indices(target) {
            // A name inside a comment or a string literal is text, not a
            // reference: renaming it would rewrite data the program emits.
            if actual_idx >= code_end || inside_quotes(&line[..actual_idx]) {
                continue;
            }
            let end = actual_idx + target.len();
            let whole_word = !line[..actual_idx].chars().next_back().is_some_and(is_ident)
                && !line[end..].chars().next().is_some_and(is_ident);
            if !whole_word {
                continue;
            }
            let is_declaration =
                declaration.is_some_and(|s| s.line_idx == line_idx && s.character == actual_idx);
            if is_declaration && !include_declaration {
                continue;
            }
            locations.push(Location {
                uri: doc.uri.clone(),
                range: Range {
                    start: Position {
                        line: line_idx as u32,
                        character: byte_to_utf16_col(line, actual_idx),
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: byte_to_utf16_col(line, end),
                    },
                },
            });
        }
    }

    locations
}

/// Byte length of the code part of `line`: everything before a `;`
/// comment (outside quotes); 0 for a `*` full-line comment.
fn code_length(line: &str) -> usize {
    if line.starts_with('*') {
        return 0;
    }
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ';') => return i,
            _ => {}
        }
    }
    line.len()
}

/// Whether the end of `prefix` lies inside a quoted string.
fn inside_quotes(prefix: &str) -> bool {
    let mut quote = None;
    for c in prefix.chars() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    quote.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    /// Each routine's `.loop` is its own symbol: definition, references,
    /// rename and completion stay within the routine the cursor is in.
    #[test]
    fn local_labels_are_scoped_to_their_routine() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "A:\n.loop:\tbra.s\t.loop\nB:\n.loop:\tdbra\td0,.loop\nV:\tdc.l\tA\n";
        let doc = Document::new(uri.clone(), 1, src.to_string());
        let at = |line, character| Position { line, character };

        let def = crate::definition::compute_definition(&doc, at(1, 14), None);
        let Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc)) = def else {
            panic!("{def:?}");
        };
        assert_eq!(loc.range.start.line, 1);

        let lines: Vec<u32> = compute_references(&doc, at(1, 2), true)
            .iter()
            .map(|l| l.range.start.line)
            .collect();
        assert_eq!(lines, [1, 1]);

        let symbols = crate::symbols::compute_document_symbols(&doc);
        let tower_lsp::lsp_types::DocumentSymbolResponse::Nested(symbols) = symbols else {
            panic!();
        };
        let mut loops: Vec<u32> = symbols
            .iter()
            .filter(|s| s.name == ".loop")
            .map(|s| s.range.start.line)
            .collect();
        loops.sort();
        assert_eq!(loops, [1, 3]);

        // Renaming onto a name already defined would merge two symbols.
        assert!(crate::rename::compute_rename(&doc, at(4, 1), "B").is_none());
        assert!(crate::rename::compute_rename(&doc, at(4, 1), "W").is_some());

        let tower_lsp::lsp_types::CompletionResponse::Array(items) =
            crate::completion::compute_completion(&doc, at(3, 10))
        else {
            panic!();
        };
        let locals: Vec<&str> = items
            .iter()
            .map(|i| i.label.as_str())
            .filter(|l| l.contains("loop"))
            .collect();
        assert_eq!(locals, [".loop"]);
    }

    #[test]
    fn test_compute_references() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    bra     Loop\nLoop:\n    dbra    d0,Loop\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());

        let refs = compute_references(
            &doc,
            Position {
                line: 2,
                character: 1,
            },
            true,
        );

        assert_eq!(
            refs.len(),
            3,
            "Expected 3 occurrences of 'Loop', found: {:?}",
            refs
        );
    }

    #[test]
    fn references_skip_comments_and_strings() {
        let src = "Lbl:\n bra Lbl ; Größe Lbl\n; Lbl\n* Lbl\n dc.b \"Lbl\",0\n dc.l Lbl\n";
        let doc = Document::new(Url::parse("file:///t.s").unwrap(), 1, src.to_string());
        let refs = compute_references(&doc, Position::new(0, 0), true);
        let lines: Vec<u32> = refs.iter().map(|l| l.range.start.line).collect();
        assert_eq!(lines, [0, 1, 5]);
        assert_eq!(refs[1].range.start.character, 5);
    }
}
