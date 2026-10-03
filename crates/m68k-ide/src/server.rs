//! HTTP & JSON-RPC REST Server for m68k Studio IDE.

use std::net::SocketAddr;

use axum::Router;
use axum::extract::Json;
use axum::http::{StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use rust_embed::RustEmbed;
use serde::Serialize;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::commands::assembler::{AssembleRequest, assemble_code};
use crate::commands::bitplane::{ConvertImageRequest, convert_image_to_bitplanes};
use crate::commands::copper::parse_copperlist;
use crate::commands::disassembler::{DisassembleRequest, disassemble_bytes};
use crate::commands::emulator::{
    InstallEmulatorRequest, LaunchEmulatorRequest, detect_available_emulators,
    install_emulator_package, launch_emulator,
};
use crate::commands::floppy::{
    CreateAdfRequest, WriteFileToAdfRequest, create_new_adf, inspect_adf, write_file_to_adf,
};
use crate::commands::lsp_bridge::{
    LspQueryRequest, lsp_completion, lsp_definition, lsp_diagnostics, lsp_format, lsp_hover,
    lsp_semantic_tokens, lsp_symbols,
};
use crate::commands::project::{ScaffoldProjectRequest, load_project_config, scaffold_project};
use crate::commands::workspace::{
    CreateProjectRequest, ProjectPathRequest, ReadFileRequest, WriteFileRequest, build,
    create_project, list_files, list_templates, open_project, read_file, recent_projects,
    write_file,
};

#[derive(Serialize)]
pub struct ApiErrorResponse {
    pub success: bool,
    pub error: String,
}

impl ApiErrorResponse {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            success: false,
            error: error.into(),
        }
    }
}

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct EmbeddedFrontend;

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let raw_path = uri.path().trim_start_matches('/');
    let path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    match EmbeddedFrontend::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => match EmbeddedFrontend::get("index.html") {
            Some(content) => (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                content.data,
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
        },
    }
}

/// Build the Axum API router.
pub fn create_router() -> Router {
    // The API can read and write anywhere in an opened project, so it
    // must not be reachable from arbitrary web pages: with
    // `allow_origin(Any)` any site the user visited could drive this
    // server. Only the IDE's own origin and the Vite dev server are
    // allowed.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin.to_str().is_ok_and(|o| {
                o.starts_with("http://127.0.0.1:") || o.starts_with("http://localhost:")
            })
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route(
            "/api/assemble",
            post(|Json(req): Json<AssembleRequest>| async move { Json(assemble_code(req)) }),
        )
        .route(
            "/api/disassemble",
            post(|Json(req): Json<DisassembleRequest>| async move { Json(disassemble_bytes(req)) }),
        )
        .route(
            "/api/floppy/create",
            post(|Json(req): Json<CreateAdfRequest>| async move {
                match create_new_adf(req) {
                    Ok(bytes) => (StatusCode::OK, Json(bytes)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/floppy/write",
            post(|Json(req): Json<WriteFileToAdfRequest>| async move {
                match write_file_to_adf(req) {
                    Ok(bytes) => (StatusCode::OK, Json(bytes)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/floppy/inspect",
            post(|Json(bytes): Json<Vec<u8>>| async move {
                match inspect_adf(bytes) {
                    Ok(info) => (StatusCode::OK, Json(info)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/bitplane/convert",
            post(|Json(req): Json<ConvertImageRequest>| async move {
                match convert_image_to_bitplanes(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/copper/parse",
            post(|Json(bytes): Json<Vec<u8>>| async move { Json(parse_copperlist(bytes)) }),
        )
        .route(
            "/api/workspace/open",
            post(|Json(req): Json<ProjectPathRequest>| async move {
                match open_project(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/workspace/create",
            post(|Json(req): Json<CreateProjectRequest>| async move {
                match create_project(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/workspace/recent",
            get(|| async move { Json(recent_projects()) }),
        )
        .route(
            "/api/workspace/templates",
            get(|| async move { Json(list_templates()) }),
        )
        .route(
            "/api/workspace/files",
            post(|Json(req): Json<ProjectPathRequest>| async move {
                match list_files(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/workspace/read",
            post(|Json(req): Json<ReadFileRequest>| async move {
                match read_file(req) {
                    Ok(contents) => (StatusCode::OK, Json(contents)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/workspace/write",
            post(|Json(req): Json<WriteFileRequest>| async move {
                match write_file(req) {
                    Ok(()) => (StatusCode::OK, Json(true)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/workspace/build",
            post(|Json(req): Json<ProjectPathRequest>| async move {
                match build(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/project/scaffold",
            post(|Json(req): Json<ScaffoldProjectRequest>| async move {
                match scaffold_project(req) {
                    Ok(cfg) => (StatusCode::OK, Json(cfg)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/project/load",
            post(|Json(path): Json<String>| async move {
                match load_project_config(path) {
                    Ok(cfg) => (StatusCode::OK, Json(cfg)).into_response(),
                    Err(e) => {
                        (StatusCode::BAD_REQUEST, Json(ApiErrorResponse::new(e))).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/emulator/detect",
            get(|| async move { Json(detect_available_emulators()) }),
        )
        .route(
            "/api/emulator/launch",
            post(|Json(req): Json<LaunchEmulatorRequest>| async move {
                match launch_emulator(req) {
                    Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                    Err(e) => (StatusCode::OK, Json(ApiErrorResponse::new(e))).into_response(),
                }
            }),
        )
        .route(
            "/api/emulator/install",
            post(|Json(req): Json<InstallEmulatorRequest>| async move {
                match install_emulator_package(req) {
                    Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                    Err(e) => (StatusCode::OK, Json(ApiErrorResponse::new(e))).into_response(),
                }
            }),
        )
        .route(
            "/api/python/transpile",
            post(
                |Json(req): Json<crate::languages::python::TranspilePythonRequest>| async move {
                    let platform = req.target_platform.as_deref().unwrap_or("amiga");
                    let cpu = req.target_cpu.as_deref().unwrap_or("68000");
                    match crate::languages::python::transpile_python_to_m68k(
                        &req.python_code.0,
                        platform,
                        cpu,
                    ) {
                        Ok(asm) => Json(crate::languages::python::TranspilePythonResponse {
                            success: true,
                            asm_code: asm,
                            errors: Vec::new(),
                            warnings: Vec::new(),
                        }),
                        Err(err) => Json(crate::languages::python::TranspilePythonResponse {
                            success: false,
                            asm_code: String::new(),
                            errors: vec![err],
                            warnings: Vec::new(),
                        }),
                    }
                },
            ),
        )
        .route(
            "/api/lsp/hover",
            post(|Json(req): Json<LspQueryRequest>| async move { Json(lsp_hover(req)) }),
        )
        .route(
            "/api/lsp/completion",
            post(|Json(req): Json<LspQueryRequest>| async move { Json(lsp_completion(req)) }),
        )
        .route(
            "/api/lsp/diagnostics",
            post(
                |Json((source, cpu)): Json<(String, Option<String>)>| async move {
                    Json(lsp_diagnostics(source, cpu))
                },
            ),
        )
        .route(
            "/api/lsp/format",
            post(
                |Json((source, tab_size)): Json<(String, Option<u32>)>| async move {
                    Json(lsp_format(source, tab_size))
                },
            ),
        )
        .route(
            "/api/lsp/symbols",
            post(|Json(source): Json<String>| async move { Json(lsp_symbols(source)) }),
        )
        .route(
            "/api/lsp/definition",
            post(|Json(req): Json<LspQueryRequest>| async move { Json(lsp_definition(req)) }),
        )
        .route(
            "/api/lsp/semantic_tokens",
            post(|Json(source): Json<String>| async move { Json(lsp_semantic_tokens(source)) }),
        )
        .route(
            "/api/analysis/cycles",
            post(
                |Json(req): Json<crate::commands::analysis::CycleAnalysisRequest>| async move {
                    Json(crate::commands::analysis::analyze_source_cycles(req))
                },
            ),
        )
        .route(
            "/api/hardware/registers",
            get(
                || async move { Json(crate::commands::analysis::get_hardware_register_database()) },
            ),
        );

    api_routes.layer(cors).fallback(static_handler)
}

/// Start the local server on the given port.
pub async fn start_server(port: u16, open_browser: bool) -> Result<(), Box<dyn std::error::Error>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let app = create_router();

    println!("m68k Studio Server running at http://{}", addr);

    if open_browser {
        let _ = open::that(format!("http://{}", addr));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
