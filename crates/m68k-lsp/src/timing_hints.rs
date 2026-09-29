//! Cycle counts as inlay hints, and cycle sums over a line range.

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::{
    InlayHint, InlayHintLabel, InlayHintTooltip, Position, Range, TextDocumentIdentifier,
};

use crate::analysis::cycles::instruction_timing;
use crate::document::Document;

/// Timing hints for the instructions on lines `range` covers.
pub fn compute_inlay_hints(doc: &Document, range: Range) -> Vec<InlayHint> {
    lines_in(doc, range.start.line, range.end.line)
        .filter_map(|(line, raw, mnemonic, size, operands)| {
            let timing = instruction_timing(mnemonic, size, operands)?;
            Some(InlayHint {
                position: Position::new(line as u32, raw.encode_utf16().count() as u32),
                label: InlayHintLabel::String(timing.label),
                kind: None,
                text_edits: None,
                tooltip: Some(InlayHintTooltip::String(format!(
                    "68000: {}",
                    timing.detail
                ))),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleSumParams {
    pub text_document: TextDocumentIdentifier,
    pub start_line: u32,
    pub end_line: u32,
}

/// Sum of the instruction timings on a line range.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CycleSum {
    pub min: u32,
    /// `None` once any instruction has no upper bound.
    pub max: Option<u32>,
    pub instructions: u32,
    /// Instructions without 68000 timing (68020+ forms, unknown macros).
    pub unknown: u32,
}

pub fn cycle_sum(doc: &Document, start_line: u32, end_line: u32) -> CycleSum {
    let mut sum = CycleSum {
        max: Some(0),
        ..Default::default()
    };
    for (_, _, mnemonic, size, operands) in lines_in(doc, start_line, end_line) {
        if mnemonic.is_empty() {
            continue;
        }
        match instruction_timing(mnemonic, size, operands) {
            Some(t) => {
                sum.instructions += 1;
                sum.min += t.min;
                sum.max = sum.max.zip(t.max).map(|(a, b)| a + b);
            }
            None if is_directive(mnemonic) => {}
            None => sum.unknown += 1,
        }
    }
    sum
}

type LineParts<'a> = (usize, &'a str, &'a str, &'a str, &'a [String]);

fn lines_in(doc: &Document, start: u32, end: u32) -> impl Iterator<Item = LineParts<'_>> {
    doc.parsed_lines
        .iter()
        .skip(start as usize)
        .take((end.saturating_sub(start) + 1) as usize)
        .map(|l| {
            (
                l.line_idx,
                l.raw.as_str(),
                l.mnemonic.as_str(),
                l.size.as_str(),
                l.operands.as_slice(),
            )
        })
}

/// Directives and data emit no instructions, so they are not "unknown".
fn is_directive(mnemonic: &str) -> bool {
    matches!(
        mnemonic.to_ascii_lowercase().as_str(),
        "dc" | "ds"
            | "dcb"
            | "equ"
            | "set"
            | "="
            | "section"
            | "even"
            | "cnop"
            | "align"
            | "include"
            | "incbin"
            | "incdir"
            | "macro"
            | "endm"
            | "rept"
            | "endr"
            | "if"
            | "ifd"
            | "ifnd"
            | "ifeq"
            | "ifne"
            | "ifgt"
            | "iflt"
            | "ifge"
            | "ifle"
            | "else"
            | "endif"
            | "endc"
            | "org"
            | "rs"
            | "rsreset"
            | "rsset"
            | "so"
            | "opt"
            | "end"
            | "xdef"
            | "xref"
            | "public"
            | "code"
            | "data"
            | "bss"
            | "machine"
            | "fail"
            | "printt"
            | "printv"
            | "ttl"
            | "idnt"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn doc(text: &str) -> Document {
        Document::new(Url::parse("file:///t.s").unwrap(), 1, text.to_string())
    }

    #[test]
    fn hints_sit_at_line_end() {
        let d = doc("start:\n    moveq #0,d0 ; clear\n    dc.w 1\n    rts\n");
        let hints = compute_inlay_hints(&d, Range::new(Position::new(0, 0), Position::new(3, 0)));
        let got: Vec<(u32, u32, String)> = hints
            .into_iter()
            .map(|h| match h.label {
                InlayHintLabel::String(s) => (h.position.line, h.position.character, s),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            got,
            vec![(1, 23, "4".to_string()), (3, 7, "16".to_string())]
        );
    }

    #[test]
    fn sums_ranges_and_counts_unknowns() {
        let d = doc("loop:\n move.w d0,(a0)+\n dbf d7,loop\n dc.w 0\n mystery d0\n");
        assert_eq!(
            cycle_sum(&d, 1, 2),
            CycleSum {
                min: 18,
                max: Some(22),
                instructions: 2,
                unknown: 0
            }
        );
        assert_eq!(cycle_sum(&d, 0, 4).unknown, 1);
        let open = doc(" lsl.l d1,d0\n nop\n");
        assert_eq!(cycle_sum(&open, 0, 1).max, None);
    }
}
