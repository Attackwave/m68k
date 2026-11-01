//! Language Server Protocol Backend implementation using `tower-lsp`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::completion::compute_completion;
use crate::definition::compute_definition;
use crate::diagnostics::{LinterConfig, compute_diagnostics, compute_unit_diagnostics};
use crate::document::Document;
use crate::formatting::compute_formatting;
use crate::hover::compute_hover;
use crate::references::compute_references;
use crate::rename::{compute_rename, prepare_rename};
use crate::semantic_tokens::{compute_semantic_tokens, get_semantic_tokens_legend};
use crate::signature::compute_signature_help;
use crate::symbols::compute_document_symbols;
use crate::timing_hints::{CycleSum, CycleSumParams, compute_inlay_hints, cycle_sum};
use crate::workspace::WorkspaceIndex;

/// The m68k Language Server backend state.
#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, Document>>>,
    pub workspace: Arc<RwLock<WorkspaceIndex>>,
    pub config: Arc<RwLock<LinterConfig>>,
    /// Counts validation requests, so a run started before the next
    /// edit publishes nothing stale.
    generation: Arc<AtomicU64>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(WorkspaceIndex::new())),
            config: Arc::new(RwLock::new(LinterConfig::default())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Cycle counts come from the 68000 tables; later CPUs time differently.
    async fn timing_applies(&self) -> bool {
        self.config.read().await.cpu == m68k_core::cpu_gate::MinCpu::Mc68000
    }

    /// `m68k/cycleSum`: total cycles of the instructions on a line range,
    /// for a selection readout in the editor.
    pub async fn cycle_sum(&self, params: CycleSumParams) -> Result<Option<CycleSum>> {
        if !self.timing_applies().await {
            return Ok(None);
        }
        let docs = self.documents.read().await;
        Ok(docs
            .get(&params.text_document.uri)
            .map(|doc| cycle_sum(doc, params.start_line, params.end_line)))
    }

    async fn validate_document(&self, uri: &Url) {
        let config = self.config.read().await.clone();
        let Some(main) = config.main_file.clone() else {
            let docs = self.documents.read().await;
            if let Some(doc) = docs.get(uri) {
                let diags = compute_diagnostics(doc, &config);
                self.client
                    .publish_diagnostics(uri.clone(), diags, Some(doc.version))
                    .await;
            }
            return;
        };
        // One assembly of the main program covers every open document, so
        // all of them are republished. A large program takes seconds:
        // wait for the typing to pause, and keep the executor free.
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let docs = self.documents.read().await.clone();
        let Ok(results) =
            tokio::task::spawn_blocking(move || compute_unit_diagnostics(&main, &docs, &config))
                .await
        else {
            return;
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let docs = self.documents.read().await;
        for (uri, diags) in results {
            let version = docs.get(&uri).map(|d| d.version);
            self.client.publish_diagnostics(uri, diags, version).await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Register workspace root if present
        if let Some(root_uri) = params.root_uri
            && let Ok(root_path) = root_uri.to_file_path()
        {
            let mut ws = self.workspace.write().await;
            ws.add_include_dir(root_path.clone());
            ws.add_include_dir(root_path.join("include"));
            ws.add_include_dir(root_path.join("includes"));
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "m68k-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "#".to_string(),
                        "_".to_string(),
                        ":".to_string(),
                    ]),
                    all_commit_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                    completion_item: None,
                }),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                document_formatting_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec![",".to_string(), " ".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: get_semantic_tokens_legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "m68k-lsp language server v{} initialized",
                    env!("CARGO_PKG_VERSION")
                ),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        let doc = Document::new(uri.clone(), version, text.clone());
        {
            let mut docs = self.documents.write().await;
            docs.insert(uri.clone(), doc);

            let mut ws = self.workspace.write().await;
            ws.update_document(&uri, text, version);
        }
        self.validate_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let version = params.text_document.version;
            {
                let mut docs = self.documents.write().await;
                if let Some(doc) = docs.get_mut(&uri) {
                    doc.update_text(version, change.text.clone());
                } else {
                    docs.insert(
                        uri.clone(),
                        Document::new(uri.clone(), version, change.text.clone()),
                    );
                }

                let mut ws = self.workspace.write().await;
                ws.update_document(&uri, change.text, version);
            }
            self.validate_document(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.validate_document(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut docs = self.documents.write().await;
        docs.remove(&uri);

        let mut ws = self.workspace.write().await;
        ws.remove_document(&uri);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(compute_hover(doc, pos))
        } else {
            Ok(None)
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(Some(compute_completion(doc, pos)))
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        let ws = self.workspace.read().await;

        if let Some(doc) = docs.get(&uri) {
            Ok(compute_definition(doc, pos, Some(&ws)))
        } else {
            Ok(None)
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(Some(compute_document_symbols(doc)))
        } else {
            Ok(None)
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let include_decl = params.context.include_declaration;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(Some(compute_references(doc, pos, include_decl)))
        } else {
            Ok(None)
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let pos = params.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri)
            && let Some(range) = prepare_rename(doc, pos)
        {
            return Ok(Some(PrepareRenameResponse::Range(range)));
        }
        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = params.new_name;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(compute_rename(doc, pos, &new_name))
        } else {
            Ok(None)
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let options = params.options;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(Some(compute_formatting(doc, &options)))
        } else {
            Ok(None)
        }
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(compute_signature_help(doc, pos))
        } else {
            Ok(None)
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&uri) {
            Ok(Some(SemanticTokensResult::Tokens(compute_semantic_tokens(
                doc,
            ))))
        } else {
            Ok(None)
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        if !self.timing_applies().await {
            return Ok(None);
        }
        let docs = self.documents.read().await;
        Ok(docs
            .get(&params.text_document.uri)
            .map(|doc| compute_inlay_hints(doc, params.range)))
    }
}
