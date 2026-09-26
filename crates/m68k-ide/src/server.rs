//! HTTP & JSON-RPC REST Server for m68k Studio IDE.

use std::net::SocketAddr;

use axum::extract::Json;
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use rust_embed::RustEmbed;
use tower_http::cors::{Any, CorsLayer};

use crate::commands::assembler::{assemble_code, AssembleRequest};
use crate::commands::bitplane::{convert_image_to_bitplanes, ConvertImageRequest};
use crate::commands::copper::parse_copperlist;
use crate::commands::disassembler::{disassemble_bytes, DisassembleRequest};
use crate::commands::emulator::{
    detect_available_emulators, launch_emulator, LaunchEmulatorRequest,
};
use crate::commands::floppy::{
    create_new_adf, inspect_adf, write_file_to_adf, CreateAdfRequest, WriteFileToAdfRequest,
};
use crate::commands::lsp_bridge::{
    lsp_completion, lsp_definition, lsp_diagnostics, lsp_format, lsp_hover, lsp_semantic_tokens,
    lsp_symbols, LspQueryRequest,
};
use crate::commands::project::{load_project_config, scaffold_project, ScaffoldProjectRequest};

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
            Some(content) => {
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], content.data).into_response()
            }
            None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
        },
    }
}

/// Build the Axum API router.
pub fn create_router() -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
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
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
        )
        .route(
            "/api/floppy/write",
            post(|Json(req): Json<WriteFileToAdfRequest>| async move {
                match write_file_to_adf(req) {
                    Ok(bytes) => (StatusCode::OK, Json(bytes)).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
        )
        .route(
            "/api/floppy/inspect",
            post(|Json(bytes): Json<Vec<u8>>| async move {
                match inspect_adf(bytes) {
                    Ok(info) => (StatusCode::OK, Json(info)).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
        )
        .route(
            "/api/bitplane/convert",
            post(|Json(req): Json<ConvertImageRequest>| async move {
                match convert_image_to_bitplanes(req) {
                    Ok(res) => (StatusCode::OK, Json(res)).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
        )
        .route(
            "/api/copper/parse",
            post(|Json(bytes): Json<Vec<u8>>| async move { Json(parse_copperlist(bytes)) }),
        )
        .route(
            "/api/project/scaffold",
            post(|Json(req): Json<ScaffoldProjectRequest>| async move {
                match scaffold_project(req) {
                    Ok(cfg) => (StatusCode::OK, Json(cfg)).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
        )
        .route(
            "/api/project/load",
            post(|Json(path): Json<String>| async move {
                match load_project_config(path) {
                    Ok(cfg) => (StatusCode::OK, Json(cfg)).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
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
                    Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
                }
            }),
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
