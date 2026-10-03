//! In-process LSP query bridge connecting the Monaco Editor UI directly to m68k-lsp.

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::{Position, Url};

use m68k_core::cpu_gate::MinCpu;
use m68k_lsp::completion::compute_completion;
use m68k_lsp::definition::compute_definition;
use m68k_lsp::diagnostics::{LinterConfig, compute_diagnostics};
use m68k_lsp::document::Document;
use m68k_lsp::formatting::format_source;
use m68k_lsp::hover::compute_hover;
use m68k_lsp::semantic_tokens::compute_semantic_tokens;
use m68k_lsp::symbols::compute_document_symbols;

#[derive(Debug, Clone, Deserialize)]
pub struct LspQueryRequest {
    pub source: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeHoverResponse {
    pub contents: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeCompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeDiagnosticItem {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub message: String,
    pub severity: u32, // 1 = Error, 2 = Warning
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeSymbolItem {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeLocationItem {
    pub line: u32,
    pub character: u32,
}

fn create_temp_doc(source: String) -> Document {
    let uri = Url::parse("file:///workspace/current.s").unwrap();
    Document::new(uri, 1, source)
}

pub fn lsp_hover(req: LspQueryRequest) -> Option<IdeHoverResponse> {
    let doc = create_temp_doc(req.source);
    let pos = Position {
        line: req.line,
        character: req.character,
    };
    let hover = compute_hover(&doc, pos)?;

    let text = match hover.contents {
        tower_lsp::lsp_types::HoverContents::Scalar(s) => match s {
            tower_lsp::lsp_types::MarkedString::String(s) => s,
            tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
        },
        tower_lsp::lsp_types::HoverContents::Markup(m) => m.value,
        _ => return None,
    };

    Some(IdeHoverResponse { contents: text })
}

pub fn lsp_completion(req: LspQueryRequest) -> Vec<IdeCompletionItem> {
    let doc = create_temp_doc(req.source);
    let pos = Position {
        line: req.line,
        character: req.character,
    };
    let resp = compute_completion(&doc, pos);

    let items = match resp {
        tower_lsp::lsp_types::CompletionResponse::Array(arr) => arr,
        tower_lsp::lsp_types::CompletionResponse::List(list) => list.items,
    };

    items
        .into_iter()
        .map(|item| {
            let doc_text = item.documentation.map(|d| match d {
                tower_lsp::lsp_types::Documentation::String(s) => s,
                tower_lsp::lsp_types::Documentation::MarkupContent(m) => m.value,
            });

            IdeCompletionItem {
                label: item.label,
                kind: format!(
                    "{:?}",
                    item.kind
                        .unwrap_or(tower_lsp::lsp_types::CompletionItemKind::KEYWORD)
                ),
                detail: item.detail,
                documentation: doc_text,
                insert_text: item.insert_text,
            }
        })
        .collect()
}

pub fn lsp_diagnostics(source: String, cpu: Option<String>) -> Vec<IdeDiagnosticItem> {
    let doc = create_temp_doc(source);
    let min_cpu = match cpu.as_deref().unwrap_or("68000") {
        "68010" => MinCpu::Mc68010,
        "68020" => MinCpu::Mc68020,
        "68030" => MinCpu::Mc68030,
        "68040" => MinCpu::Mc68040,
        "68060" => MinCpu::Mc68060,
        _ => MinCpu::Mc68000,
    };

    let config = LinterConfig { cpu: min_cpu };

    let diags = compute_diagnostics(&doc, &config);

    diags
        .into_iter()
        .map(|d| IdeDiagnosticItem {
            start_line: d.range.start.line,
            start_col: d.range.start.character,
            end_line: d.range.end.line,
            end_col: d.range.end.character,
            message: d.message,
            severity: match d.severity {
                Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR) => 1,
                _ => 2,
            },
        })
        .collect()
}

pub fn lsp_format(source: String, tab_size: Option<u32>) -> String {
    let config = m68k_lsp::formatting::AsmFormatConfig {
        indent_spaces: tab_size.unwrap_or(4) as usize,
        ..Default::default()
    };
    format_source(&source, &config)
}

pub fn lsp_symbols(source: String) -> Vec<IdeSymbolItem> {
    let doc = create_temp_doc(source);
    let resp = compute_document_symbols(&doc);

    match resp {
        tower_lsp::lsp_types::DocumentSymbolResponse::Flat(symbols) => symbols
            .into_iter()
            .map(|s| IdeSymbolItem {
                name: s.name,
                kind: format!("{:?}", s.kind),
                line: s.location.range.start.line,
                character: s.location.range.start.character,
            })
            .collect(),
        tower_lsp::lsp_types::DocumentSymbolResponse::Nested(symbols) => symbols
            .into_iter()
            .map(|s| IdeSymbolItem {
                name: s.name,
                kind: format!("{:?}", s.kind),
                line: s.range.start.line,
                character: s.range.start.character,
            })
            .collect(),
    }
}

pub fn lsp_definition(req: LspQueryRequest) -> Option<IdeLocationItem> {
    let doc = create_temp_doc(req.source);
    let pos = Position {
        line: req.line,
        character: req.character,
    };
    let resp = compute_definition(&doc, pos, None)?;

    match resp {
        tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc) => Some(IdeLocationItem {
            line: loc.range.start.line,
            character: loc.range.start.character,
        }),
        _ => None,
    }
}

pub fn lsp_semantic_tokens(source: String) -> Vec<u32> {
    let doc = create_temp_doc(source);
    let tokens = compute_semantic_tokens(&doc);
    let mut out = Vec::new();
    for t in tokens.data {
        out.push(t.delta_line);
        out.push(t.delta_start);
        out.push(t.length);
        out.push(t.token_type);
        out.push(t.token_modifiers_bitset);
    }
    out
}
