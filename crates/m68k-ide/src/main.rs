//! Binary entry point for m68k-ide desktop app.

use m68k_ide::start_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(6800);

    let no_open = std::env::args().any(|arg| arg == "--no-open");

    start_server(port, !no_open).await
}
