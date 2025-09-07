use crate::lsp::{
    utils, EnhancedDiagnostic, LspCapabilities, LspDocument, LspError, LspEvent, LspRequest,
    LspResponse, LspResult, LspServerInfo, LspServerStatus,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex, RwLock};
use tower_lsp::lsp_types::*;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// LSP client for managing language servers
#[derive(Debug)]
pub struct LspClient {
    /// Active language servers
    servers: Arc<RwLock<HashMap<String, Arc<LspServer>>>>,

    /// Open documents tracked by the client
    documents: Arc<RwLock<HashMap<Url, LspDocument>>>,

    /// Event sender for UI updates
    event_sender: mpsc::UnboundedSender<LspEvent>,

    /// Request ID counter
    request_id: Arc<Mutex<i64>>,

    /// Pending requests
    pending_requests: Arc<RwLock<HashMap<i64, mpsc::UnboundedSender<Value>>>>,
}

/// Individual LSP server instance
#[derive(Debug)]
pub struct LspServer {
    pub info: LspServerInfo,
    pub capabilities: Option<ServerCapabilities>,
    process: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    request_sender: mpsc::UnboundedSender<LspServerMessage>,
    response_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<Value>>>>,
}

/// Messages that can be sent to an LSP server
#[derive(Debug)]
enum LspServerMessage {
    Request {
        id: i64,
        method: String,
        params: Value,
        response_sender: mpsc::UnboundedSender<Value>,
    },
    Notification {
        method: String,
        params: Value,
    },
    Shutdown,
}

impl LspClient {
    /// Create a new LSP client
    pub fn new(event_sender: mpsc::UnboundedSender<LspEvent>) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            documents: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            request_id: Arc::new(Mutex::new(0)),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a language server
    pub async fn start_server(&self, mut server_info: LspServerInfo) -> LspResult<String> {
        info!("Starting LSP server: {}", server_info.name);

        // Update status to initializing
        server_info.status = LspServerStatus::Initializing;
        self.emit_server_status_changed(&server_info.id, &server_info.status)
            .await?;

        // Spawn the server process
        let mut cmd = Command::new(&server_info.command);
        cmd.args(&server_info.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut process = cmd.spawn().map_err(|e| LspError::InitializationFailed {
            reason: format!("Failed to spawn process: {}", e),
        })?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| LspError::InitializationFailed {
                reason: "Failed to get stdin".to_string(),
            })?;

        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| LspError::InitializationFailed {
                reason: "Failed to get stdout".to_string(),
            })?;

        // Create communication channels
        let (request_sender, mut request_receiver) = mpsc::unbounded_channel();
        let (response_sender, response_receiver) = mpsc::unbounded_channel();

        // Create server instance
        let server = Arc::new(LspServer {
            info: server_info.clone(),
            capabilities: None,
            process: Arc::new(Mutex::new(Some(process))),
            stdin: Arc::new(Mutex::new(Some(stdin))),
            request_sender,
            response_receiver: Arc::new(Mutex::new(Some(response_receiver))),
        });

        // Store server
        let server_id = server_info.id.clone();
        self.servers
            .write()
            .await
            .insert(server_id.clone(), server.clone());

        // Spawn server communication task
        let event_sender = self.event_sender.clone();
        let server_clone = server.clone();
        let pending_requests = self.pending_requests.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run_server_communication(
                server_clone,
                stdout,
                request_receiver,
                response_sender,
                event_sender,
                pending_requests,
            )
            .await
            {
                error!("Server communication error: {}", e);
            }
        });

        // Initialize the server
        self.initialize_server(&server_id).await?;

        Ok(server_id)
    }

    /// Initialize a language server
    async fn initialize_server(&self, server_id: &str) -> LspResult<()> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_id)
            .ok_or_else(|| LspError::ServerNotFound {
                server_name: server_id.to_string(),
            })?;

        // Send initialize request
        let initialize_params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: None,
            initialization_options: server.info.initialization_options.clone(),
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    apply_edit: Some(true),
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        document_changes: Some(true),
                        resource_operations: Some(vec![
                            ResourceOperationKind::Create,
                            ResourceOperationKind::Rename,
                            ResourceOperationKind::Delete,
                        ]),
                        failure_handling: Some(FailureHandlingKind::Abort),
                        normalizes_line_endings: Some(true),
                        change_annotation_support: None,
                    }),
                    did_change_configuration: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    did_change_watched_files: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    symbol: Some(WorkspaceSymbolClientCapabilities {
                        dynamic_registration: Some(true),
                        symbol_kind: Some(SymbolKindCapability {
                            value_set: Some(vec![
                                SymbolKind::FILE,
                                SymbolKind::MODULE,
                                SymbolKind::NAMESPACE,
                                SymbolKind::PACKAGE,
                                SymbolKind::CLASS,
                                SymbolKind::METHOD,
                                SymbolKind::PROPERTY,
                                SymbolKind::FIELD,
                                SymbolKind::CONSTRUCTOR,
                                SymbolKind::ENUM,
                                SymbolKind::INTERFACE,
                                SymbolKind::FUNCTION,
                                SymbolKind::VARIABLE,
                                SymbolKind::CONSTANT,
                                SymbolKind::STRING,
                                SymbolKind::NUMBER,
                                SymbolKind::BOOLEAN,
                                SymbolKind::ARRAY,
                                SymbolKind::OBJECT,
                                SymbolKind::KEY,
                                SymbolKind::NULL,
                                SymbolKind::ENUM_MEMBER,
                                SymbolKind::STRUCT,
                                SymbolKind::EVENT,
                                SymbolKind::OPERATOR,
                                SymbolKind::TYPE_PARAMETER,
                            ]),
                        }),
                        tag_support: None,
                        resolve_support: None,
                    }),
                    execute_command: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    workspace_folders: Some(true),
                    configuration: Some(true),
                    semantic_tokens: None,
                    code_lens: None,
                    file_operations: None,
                    inline_value: None,
                    inlay_hint: None,
                    diagnostic: None,
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: Some(TextDocumentSyncClientCapabilities {
                        dynamic_registration: Some(true),
                        will_save: Some(true),
                        will_save_wait_until: Some(true),
                        did_save: Some(true),
                    }),
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(true),
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(true),
                            commit_characters_support: Some(true),
                            documentation_format: Some(vec![MarkupKind::Markdown]),
                            deprecated_support: Some(true),
                            preselect_support: Some(true),
                            tag_support: Some(CompletionItemTagSupport {
                                value_set: vec![CompletionItemTag::DEPRECATED],
                            }),
                            insert_replace_support: Some(true),
                            resolve_support: Some(CompletionItemCapabilityResolveSupport {
                                properties: vec!["documentation".to_string(), "detail".to_string()],
                            }),
                            insert_text_mode_support: None,
                            label_details_support: Some(true),
                        }),
                        completion_item_kind: Some(CompletionItemKindCapability {
                            value_set: Some(vec![
                                CompletionItemKind::TEXT,
                                CompletionItemKind::METHOD,
                                CompletionItemKind::FUNCTION,
                                CompletionItemKind::CONSTRUCTOR,
                                CompletionItemKind::FIELD,
                                CompletionItemKind::VARIABLE,
                                CompletionItemKind::CLASS,
                                CompletionItemKind::INTERFACE,
                                CompletionItemKind::MODULE,
                                CompletionItemKind::PROPERTY,
                                CompletionItemKind::UNIT,
                                CompletionItemKind::VALUE,
                                CompletionItemKind::ENUM,
                                CompletionItemKind::KEYWORD,
                                CompletionItemKind::SNIPPET,
                                CompletionItemKind::COLOR,
                                CompletionItemKind::FILE,
                                CompletionItemKind::REFERENCE,
                                CompletionItemKind::FOLDER,
                                CompletionItemKind::ENUM_MEMBER,
                                CompletionItemKind::CONSTANT,
                                CompletionItemKind::STRUCT,
                                CompletionItemKind::EVENT,
                                CompletionItemKind::OPERATOR,
                                CompletionItemKind::TYPE_PARAMETER,
                            ]),
                        }),
                        context_support: Some(true),
                        insert_text_mode: None,
                        completion_list: None,
                    }),
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: Some(true),
                        content_format: Some(vec![MarkupKind::Markdown]),
                    }),
                    signature_help: Some(SignatureHelpClientCapabilities {
                        dynamic_registration: Some(true),
                        signature_information: Some(SignatureInformationSettings {
                            documentation_format: Some(vec![MarkupKind::Markdown]),
                            parameter_information: Some(ParameterInformationSettings {
                                label_offset_support: Some(true),
                            }),
                            active_parameter_support: Some(true),
                        }),
                        context_support: Some(true),
                    }),
                    definition: Some(GotoCapability {
                        dynamic_registration: Some(true),
                        link_support: Some(true),
                    }),
                    type_definition: Some(GotoCapability {
                        dynamic_registration: Some(true),
                        link_support: Some(true),
                    }),
                    implementation: Some(GotoCapability {
                        dynamic_registration: Some(true),
                        link_support: Some(true),
                    }),
                    references: Some(ReferenceClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    document_highlight: Some(DocumentHighlightClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    document_symbol: Some(DocumentSymbolClientCapabilities {
                        dynamic_registration: Some(true),
                        symbol_kind: Some(SymbolKindCapability {
                            value_set: Some(vec![
                                SymbolKind::FILE,
                                SymbolKind::MODULE,
                                SymbolKind::NAMESPACE,
                                SymbolKind::PACKAGE,
                                SymbolKind::CLASS,
                                SymbolKind::METHOD,
                                SymbolKind::PROPERTY,
                                SymbolKind::FIELD,
                                SymbolKind::CONSTRUCTOR,
                                SymbolKind::ENUM,
                                SymbolKind::INTERFACE,
                                SymbolKind::FUNCTION,
                                SymbolKind::VARIABLE,
                                SymbolKind::CONSTANT,
                                SymbolKind::STRING,
                                SymbolKind::NUMBER,
                                SymbolKind::BOOLEAN,
                                SymbolKind::ARRAY,
                                SymbolKind::OBJECT,
                                SymbolKind::KEY,
                                SymbolKind::NULL,
                                SymbolKind::ENUM_MEMBER,
                                SymbolKind::STRUCT,
                                SymbolKind::EVENT,
                                SymbolKind::OPERATOR,
                                SymbolKind::TYPE_PARAMETER,
                            ]),
                        }),
                        hierarchical_document_symbol_support: Some(true),
                        tag_support: None,
                    }),
                    code_action: Some(CodeActionClientCapabilities {
                        dynamic_registration: Some(true),
                        code_action_literal_support: Some(CodeActionLiteralSupport {
                            code_action_kind: CodeActionKindLiteralSupport {
                                value_set: vec![
                                    CodeActionKind::EMPTY,
                                    CodeActionKind::QUICKFIX,
                                    CodeActionKind::REFACTOR,
                                    CodeActionKind::REFACTOR_EXTRACT,
                                    CodeActionKind::REFACTOR_INLINE,
                                    CodeActionKind::REFACTOR_REWRITE,
                                    CodeActionKind::SOURCE,
                                    CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                                ],
                            },
                        }),
                        is_preferred_support: Some(true),
                        disabled_support: Some(true),
                        data_support: Some(true),
                        resolve_support: Some(CodeActionCapabilityResolveSupport {
                            properties: vec!["edit".to_string()],
                        }),
                        honors_change_annotations: Some(true),
                    }),
                    formatting: Some(DocumentFormattingClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    range_formatting: Some(DocumentRangeFormattingClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    rename: Some(RenameClientCapabilities {
                        dynamic_registration: Some(true),
                        prepare_support: Some(true),
                        prepare_support_default_behavior: Some(
                            PrepareSupportDefaultBehavior::Identifier,
                        ),
                        honors_change_annotations: Some(true),
                    }),
                    inlay_hint: Some(InlayHintClientCapabilities {
                        dynamic_registration: Some(true),
                        resolve_support: Some(InlayHintResolveClientCapabilities {
                            properties: vec!["tooltip".to_string(), "label".to_string()],
                        }),
                    }),
                    semantic_tokens: Some(SemanticTokensClientCapabilities {
                        dynamic_registration: Some(true),
                        requests: SemanticTokensClientCapabilitiesRequests {
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                        token_types: vec![
                            SemanticTokenType::NAMESPACE,
                            SemanticTokenType::TYPE,
                            SemanticTokenType::CLASS,
                            SemanticTokenType::ENUM,
                            SemanticTokenType::INTERFACE,
                            SemanticTokenType::STRUCT,
                            SemanticTokenType::TYPE_PARAMETER,
                            SemanticTokenType::PARAMETER,
                            SemanticTokenType::VARIABLE,
                            SemanticTokenType::PROPERTY,
                            SemanticTokenType::ENUM_MEMBER,
                            SemanticTokenType::EVENT,
                            SemanticTokenType::FUNCTION,
                            SemanticTokenType::METHOD,
                            SemanticTokenType::MACRO,
                            SemanticTokenType::KEYWORD,
                            SemanticTokenType::MODIFIER,
                            SemanticTokenType::COMMENT,
                            SemanticTokenType::STRING,
                            SemanticTokenType::NUMBER,
                            SemanticTokenType::REGEXP,
                            SemanticTokenType::OPERATOR,
                        ],
                        token_modifiers: vec![
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                            SemanticTokenModifier::READONLY,
                            SemanticTokenModifier::STATIC,
                            SemanticTokenModifier::DEPRECATED,
                            SemanticTokenModifier::ABSTRACT,
                            SemanticTokenModifier::ASYNC,
                            SemanticTokenModifier::MODIFICATION,
                            SemanticTokenModifier::DOCUMENTATION,
                            SemanticTokenModifier::DEFAULT_LIBRARY,
                        ],
                        formats: vec![TokenFormat::RELATIVE],
                        overlapping_token_support: Some(true),
                        multiline_token_support: Some(true),
                        server_cancel_support: Some(true),
                        augments_syntax_tokens: Some(true),
                    }),
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        tag_support: Some(DiagnosticTagSupport {
                            value_set: vec![DiagnosticTag::UNNECESSARY, DiagnosticTag::DEPRECATED],
                        }),
                        version_support: Some(true),
                        code_description_support: Some(true),
                        data_support: Some(true),
                    }),
                    folding_range: Some(FoldingRangeClientCapabilities {
                        dynamic_registration: Some(true),
                        range_limit: Some(5000),
                        line_folding_only: Some(false),
                        folding_range_kind: Some(FoldingRangeKindCapability {
                            value_set: Some(vec![
                                FoldingRangeKind::Comment,
                                FoldingRangeKind::Imports,
                                FoldingRangeKind::Region,
                            ]),
                        }),
                        folding_range: Some(FoldingRangeCapability {
                            collapsed_text: Some(true),
                        }),
                    }),
                    selection_range: Some(SelectionRangeClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    call_hierarchy: Some(CallHierarchyClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    linked_editing_range: Some(LinkedEditingRangeClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    moniker: Some(MonikerClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    type_hierarchy: Some(TypeHierarchyClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    inline_value: Some(InlineValueClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    diagnostic: Some(DiagnosticClientCapabilities {
                        dynamic_registration: Some(true),
                        related_document_support: Some(true),
                    }),
                    // Add missing fields as None
                    on_type_formatting: None,
                    declaration: None,
                    code_lens: None,
                    document_link: None,
                    color_provider: None,
                }),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    show_message: Some(ShowMessageRequestClientCapabilities {
                        message_action_item: Some(MessageActionItemCapabilities {
                            additional_properties_support: Some(true),
                        }),
                    }),
                    show_document: Some(ShowDocumentClientCapabilities { support: true }),
                }),
                general: Some(GeneralClientCapabilities {
                    regular_expressions: Some(RegularExpressionsClientCapabilities {
                        engine: "ECMAScript".to_string(),
                        version: Some("ES2020".to_string()),
                    }),
                    markdown: Some(MarkdownClientCapabilities {
                        parser: "marked".to_string(),
                        version: Some("1.1.0".to_string()),
                        allowed_tags: Some(vec![
                            "a".to_string(),
                            "b".to_string(),
                            "blockquote".to_string(),
                            "br".to_string(),
                            "code".to_string(),
                            "del".to_string(),
                            "em".to_string(),
                            "h1".to_string(),
                            "h2".to_string(),
                            "h3".to_string(),
                            "h4".to_string(),
                            "h5".to_string(),
                            "h6".to_string(),
                            "hr".to_string(),
                            "i".to_string(),
                            "img".to_string(),
                            "li".to_string(),
                            "ol".to_string(),
                            "p".to_string(),
                            "pre".to_string(),
                            "strong".to_string(),
                            "sup".to_string(),
                            "table".to_string(),
                            "tbody".to_string(),
                            "td".to_string(),
                            "th".to_string(),
                            "thead".to_string(),
                            "tr".to_string(),
                            "ul".to_string(),
                        ]),
                    }),
                    position_encodings: Some(vec![PositionEncodingKind::UTF16]),
                    stale_request_support: None,
                }),
                experimental: None,
            },
            trace: Some(TraceValue::Verbose),
            workspace_folders: None,
            client_info: Some(ClientInfo {
                name: "RustIDE".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            locale: Some("en-US".to_string()),
        };

        let response = self
            .send_request(
                server_id,
                "initialize",
                serde_json::to_value(initialize_params)?,
            )
            .await?;

        // Parse initialize response
        let initialize_result: InitializeResult = serde_json::from_value(response)?;

        // Update server capabilities
        {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(server_id) {
                let server_mut = Arc::get_mut(server).unwrap();
                server_mut.capabilities = Some(initialize_result.capabilities.clone());
                server_mut.info.capabilities = initialize_result.capabilities.clone();
                server_mut.info.status = LspServerStatus::Running;
            }
        }

        // Send initialized notification
        self.send_notification(server_id, "initialized", serde_json::json!({}))
            .await?;

        // Emit status change and capabilities
        self.emit_server_status_changed(server_id, &LspServerStatus::Running)
            .await?;
        self.emit_capabilities_changed(
            server_id,
            &LspCapabilities::from(&initialize_result.capabilities),
        )
        .await?;

        info!("LSP server initialized: {}", server_id);
        Ok(())
    }

    /// Stop a language server
    pub async fn stop_server(&self, server_id: &str) -> LspResult<()> {
        info!("Stopping LSP server: {}", server_id);

        // Send shutdown request
        if let Ok(_) = self
            .send_request(server_id, "shutdown", serde_json::json!(null))
            .await
        {
            // Send exit notification
            let _ = self
                .send_notification(server_id, "exit", serde_json::json!({}))
                .await;
        }

        // Remove server and kill process
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.remove(server_id) {
            let mut process = server.process.lock().await;
            if let Some(mut child) = process.take() {
                let _ = child.kill().await;
            }
        }

        self.emit_server_status_changed(server_id, &LspServerStatus::Stopped)
            .await?;
        Ok(())
    }

    /// Send a request to a language server
    pub async fn send_request(
        &self,
        server_id: &str,
        method: &str,
        params: Value,
    ) -> LspResult<Value> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_id)
            .ok_or_else(|| LspError::ServerNotFound {
                server_name: server_id.to_string(),
            })?;

        // Generate request ID
        let id = {
            let mut counter = self.request_id.lock().await;
            *counter += 1;
            *counter
        };

        // Create response channel
        let (response_sender, mut response_receiver) = mpsc::unbounded_channel();

        // Store pending request
        self.pending_requests
            .write()
            .await
            .insert(id, response_sender);

        // Send request
        server
            .request_sender
            .send(LspServerMessage::Request {
                id,
                method: method.to_string(),
                params,
                response_sender: mpsc::unbounded_channel().0, // This is handled differently in the actual implementation
            })
            .map_err(|_| LspError::CommunicationError {
                source: "Failed to send request".to_string(),
            })?;

        // Wait for response with timeout
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(30), response_receiver.recv())
                .await
                .map_err(|_| LspError::RequestFailed {
                    method: method.to_string(),
                    message: "Request timeout".to_string(),
                })?
                .ok_or_else(|| LspError::RequestFailed {
                    method: method.to_string(),
                    message: "No response received".to_string(),
                })?;

        Ok(response)
    }

    /// Send a notification to a language server
    pub async fn send_notification(
        &self,
        server_id: &str,
        method: &str,
        params: Value,
    ) -> LspResult<()> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_id)
            .ok_or_else(|| LspError::ServerNotFound {
                server_name: server_id.to_string(),
            })?;

        server
            .request_sender
            .send(LspServerMessage::Notification {
                method: method.to_string(),
                params,
            })
            .map_err(|_| LspError::CommunicationError {
                source: "Failed to send notification".to_string(),
            })?;

        Ok(())
    }

    /// Open a document in the language server
    pub async fn did_open_document(
        &self,
        uri: Url,
        language_id: String,
        version: i32,
        content: String,
    ) -> LspResult<()> {
        // Store document
        let document = LspDocument {
            uri: uri.clone(),
            language_id: language_id.clone(),
            version,
            content: content.clone(),
            diagnostics: Vec::new(),
            last_modified: std::time::SystemTime::now(),
        };

        self.documents.write().await.insert(uri.clone(), document);

        // Find appropriate server for this language
        let server_id = self.find_server_for_language(&language_id).await?;

        // Send didOpen notification
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id,
                version,
                text: content,
            },
        };

        self.send_notification(
            &server_id,
            "textDocument/didOpen",
            serde_json::to_value(params)?,
        )
        .await?;
        Ok(())
    }

    /// Update a document in the language server
    pub async fn did_change_document(
        &self,
        uri: Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> LspResult<()> {
        // Update document with proper rope-based text editing
        let updated_content = {
            let mut documents = self.documents.write().await;
            let document = documents.get_mut(&uri)
                .ok_or_else(|| LspError::DocumentNotFound {
                    uri: uri.to_string(),
                })?;
            
            document.version = version;
            document.last_modified = std::time::SystemTime::now();
            
            // Use rope for efficient text editing
            let mut rope = Rope::from_str(&document.content);
            
            // Apply changes in reverse order to maintain correct positions
            let mut sorted_changes = changes.clone();
            sorted_changes.sort_by(|a, b| {
                if let (Some(range_a), Some(range_b)) = (&a.range, &b.range) {
                    // Sort by start position in reverse order (later positions first)
                    range_b.start.line.cmp(&range_a.start.line)
                        .then(range_b.start.character.cmp(&range_a.start.character))
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            
            for change in sorted_changes {
                if let Some(range) = change.range {
                    // Range-based change - more complex but accurate
                    let start_offset = self.position_to_offset(&rope, &range.start)?;
                    let end_offset = self.position_to_offset(&rope, &range.end)?;
                    
                    // Validate offsets
                    if start_offset <= end_offset && end_offset <= rope.len() {
                        // Remove the old text
                        rope.remove(start_offset..end_offset);
                        
                        // Insert the new text
                        if !change.text.is_empty() {
                            rope.insert(start_offset, &change.text);
                        }
                    } else {
                        warn!("Invalid range for text change: {:?}", range);
                    }
                } else {
                    // Full document change
                    rope = Rope::from_str(&change.text);
                }
            }
            
            let new_content = rope.to_string();
            document.content = new_content.clone();
            new_content
        };
        
        // Find server for this document
        let language_id = {
            let documents = self.documents.read().await;
            documents.get(&uri)
                .map(|doc| doc.language_id.clone())
                .ok_or_else(|| LspError::DocumentNotFound {
                    uri: uri.to_string(),
                })?
        };
        
        let server_id = self.find_server_for_language(&language_id).await?;
        
        // Send didChange notification with the processed changes
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: changes,
        };
        
        self.send_notification(&server_id, "textDocument/didChange", serde_json::to_value(params)?).await?;
        
        // Trigger ownership analysis for Rust files
        if language_id == "rust" {
            // This would trigger ownership analysis in a background task
            let uri_clone = uri.clone();
            let content_clone = updated_content;
            tokio::spawn(async move {
                // Background ownership analysis
                if let Ok(mut analyzer) = tokio::task::spawn_blocking(|| OwnershipAnalyzer::new()).await {
                    if let Err(e) = analyzer.analyze_document(uri_clone, &content_clone, version).await {
                        error!("Ownership analysis failed: {}", e);
                    }
                }
            });
        }
        
        Ok(())
    }

    /// Close a document in the language server
    pub async fn did_close_document(&self, uri: Url) -> LspResult<()> {
        // Remove document
        let document = self.documents.write().await.remove(&uri).ok_or_else(|| {
            LspError::DocumentNotFound {
                uri: uri.to_string(),
            }
        })?;

        let server_id = self.find_server_for_language(&document.language_id).await?;

        // Send didClose notification
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        };

        self.send_notification(
            &server_id,
            "textDocument/didClose",
            serde_json::to_value(params)?,
        )
        .await?;
        Ok(())
    }

    /// Find the appropriate server for a language
    async fn find_server_for_language(&self, language_id: &str) -> LspResult<String> {
        let servers = self.servers.read().await;

        for (server_id, server) in servers.iter() {
            if server.info.language_id == language_id
                && server.info.status == LspServerStatus::Running
            {
                return Ok(server_id.clone());
            }
        }

        Err(LspError::ServerNotFound {
            server_name: format!("No server found for language: {}", language_id),
        })
    }

    /// Handle server communication
    async fn run_server_communication(
        server: Arc<LspServer>,
        stdout: tokio::process::ChildStdout,
        mut request_receiver: mpsc::UnboundedReceiver<LspServerMessage>,
        _response_sender: mpsc::UnboundedSender<Value>,
        event_sender: mpsc::UnboundedSender<LspEvent>,
        _pending_requests: Arc<RwLock<HashMap<i64, mpsc::UnboundedSender<Value>>>>,
    ) -> LspResult<()> {
        let mut reader = BufReader::new(stdout);
        let mut stdin = server.stdin.lock().await;
        let stdin = stdin.as_mut().ok_or_else(|| LspError::CommunicationError {
            source: "No stdin available".to_string(),
        })?;

        loop {
            tokio::select! {
                // Handle incoming requests/notifications
                message = request_receiver.recv() => {
                    match message {
                        Some(LspServerMessage::Request { id, method, params, .. }) => {
                            let request = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": method,
                                "params": params
                            });

                            let content = serde_json::to_string(&request)?;
                            let message = format!("Content-Length: {}\r\n\r\n{}", content.len(), content);

                            if let Err(e) = stdin.write_all(message.as_bytes()).await {
                                error!("Failed to write to server stdin: {}", e);
                                break;
                            }

                            if let Err(e) = stdin.flush().await {
                                error!("Failed to flush server stdin: {}", e);
                                break;
                            }
                        }
                        Some(LspServerMessage::Notification { method, params }) => {
                            let notification = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": method,
                                "params": params
                            });

                            let content = serde_json::to_string(&notification)?;
                            let message = format!("Content-Length: {}\r\n\r\n{}", content.len(), content);

                            if let Err(e) = stdin.write_all(message.as_bytes()).await {
                                error!("Failed to write notification to server: {}", e);
                                break;
                            }

                            if let Err(e) = stdin.flush().await {
                                error!("Failed to flush notification to server: {}", e);
                                break;
                            }
                        }
                        Some(LspServerMessage::Shutdown) => {
                            break;
                        }
                        None => {
                            debug!("Request channel closed");
                            break;
                        }
                    }
                }

                // Handle server responses (simplified - real implementation would parse LSP protocol)
                line = reader.read_line(&mut String::new()) => {
                    match line {
                        Ok(0) => {
                            debug!("Server stdout closed");
                            break;
                        }
                        Ok(_) => {
                            // Handle LSP protocol parsing here
                            // This is simplified - real implementation would:
                            // 1. Parse Content-Length header
                            // 2. Read exact number of bytes
                            // 3. Parse JSON-RPC message
                            // 4. Route responses back to pending requests
                            // 5. Handle notifications from server
                        }
                        Err(e) => {
                            error!("Failed to read from server stdout: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        // Emit server stopped event
        let _ = event_sender.send(LspEvent::ServerStatusChanged {
            server_id: server.info.id.clone(),
            status: LspServerStatus::Stopped,
        });

        Ok(())
    }

    /// Emit server status changed event
    async fn emit_server_status_changed(
        &self,
        server_id: &str,
        status: &LspServerStatus,
    ) -> LspResult<()> {
        self.event_sender
            .send(LspEvent::ServerStatusChanged {
                server_id: server_id.to_string(),
                status: status.clone(),
            })
            .map_err(|_| LspError::CommunicationError {
                source: "Failed to send event".to_string(),
            })?;
        Ok(())
    }

    /// Emit capabilities changed event
    async fn emit_capabilities_changed(
        &self,
        server_id: &str,
        capabilities: &LspCapabilities,
    ) -> LspResult<()> {
        self.event_sender
            .send(LspEvent::CapabilitiesChanged {
                server_id: server_id.to_string(),
                capabilities: capabilities.clone(),
            })
            .map_err(|_| LspError::CommunicationError {
                source: "Failed to send event".to_string(),
            })?;
        Ok(())
    }

    /// Get all active servers
    pub async fn get_servers(&self) -> HashMap<String, LspServerInfo> {
        let servers = self.servers.read().await;
        servers
            .iter()
            .map(|(id, server)| (id.clone(), server.info.clone()))
            .collect()
    }

    /// Get server by ID
    pub async fn get_server(&self, server_id: &str) -> Option<LspServerInfo> {
        let servers = self.servers.read().await;
        servers.get(server_id).map(|server| server.info.clone())
    }

    /// Get all open documents
    pub async fn get_documents(&self) -> HashMap<Url, LspDocument> {
        self.documents.read().await.clone()
    }

    /// Get document by URI
    pub async fn get_document(&self, uri: &Url) -> Option<LspDocument> {
        self.documents.read().await.get(uri).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn create_test_client() -> LspClient {
        let (sender, _) = mpsc::unbounded_channel();
        LspClient::new(sender)
    }

    #[tokio::test]
    async fn test_client_creation() {
        let client = create_test_client();
        assert!(client.get_servers().await.is_empty());
        assert!(client.get_documents().await.is_empty());
    }

    #[tokio::test]
    async fn test_document_management() {
        let client = create_test_client();
        let uri = Url::parse("file:///test.rs").unwrap();

        // Test document opening would fail without a server, which is expected
        // In a real test, we'd mock the server communication
    }

    #[test]
    fn test_server_info_creation() {
        let info = LspServerInfo {
            id: "test".to_string(),
            name: "Test Server".to_string(),
            language_id: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
            initialization_options: None,
            capabilities: ServerCapabilities::default(),
            status: LspServerStatus::NotStarted,
        };

        assert_eq!(info.id, "test");
        assert_eq!(info.language_id, "rust");
        assert_eq!(info.status, LspServerStatus::NotStarted);
    }
}
