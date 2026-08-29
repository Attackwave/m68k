//! Language Server Protocol Backend implementation using `tower-lsp`.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::completion::compute_completion;
use crate::definition::compute_definition;
use crate::diagnostics::{LinterConfig, compute_diagnostics};
use crate::document::Document;
use crate::formatting::compute_formatting;
use crate::hover::compute_hover;
use crate::references::compute_references;
use crate::rename::{compute_rename, prepare_rename};
use crate::semantic_tokens::{compute_semantic_tokens, get_semantic_tokens_legend};
use crate::signature::compute_signature_help;
use crate::symbols::compute_document_symbols;
use crate::workspace::WorkspaceIndex;

/// The m68k Language Server backend state.
#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, Document>>>,
    pub workspace: Arc<RwLock<WorkspaceIndex>>,
    pub config: Arc<RwLock<LinterConfig>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(WorkspaceIndex::new())),
            config: Arc::new(RwLock::new(LinterConfig::default())),
        }
    }

    async fn validate_document(&self, uri: &Url) {
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            let config = *self.config.read().await;
            let diags = compute_diagnostics(doc, &config);
            self.client
                .publish_diagnostics(uri.clone(), diags, Some(doc.version))
                .await;
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
}
