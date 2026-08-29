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
use crate::hover::compute_hover;
use crate::symbols::compute_document_symbols;

/// The m68k Language Server backend state.
#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, Document>>>,
    pub config: Arc<RwLock<LinterConfig>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
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
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
        let doc = Document::new(
            uri.clone(),
            params.text_document.version,
            params.text_document.text,
        );
        {
            let mut docs = self.documents.write().await;
            docs.insert(uri.clone(), doc);
        }
        self.validate_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut docs = self.documents.write().await;
                if let Some(doc) = docs.get_mut(&uri) {
                    doc.update_text(params.text_document.version, change.text);
                } else {
                    docs.insert(
                        uri.clone(),
                        Document::new(uri.clone(), params.text_document.version, change.text),
                    );
                }
            }
            self.validate_document(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.validate_document(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut docs = self.documents.write().await;
        docs.remove(&params.text_document.uri);
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
        if let Some(doc) = docs.get(&uri) {
            Ok(compute_definition(doc, pos))
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
}
