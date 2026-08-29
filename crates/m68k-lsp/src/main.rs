//! Language Server binary entry point for m68k-lsp.

use clap::Parser;
use tower_lsp::{LspService, Server};

use m68k_core::cpu_gate::validate_cpu_name;
use m68k_lsp::Backend;
use m68k_lsp::diagnostics::LinterConfig;

#[derive(Parser, Debug)]
#[command(
    name = "m68k-lsp",
    about = "Language Server Protocol daemon for Motorola 68000 assembly",
    version
)]
struct Cli {
    /// Target CPU model (68000, 68010, 68020, 68030, 68040, 68060)
    #[arg(short, long, default_value = "68000")]
    cpu: String,

    /// Communicate via stdio (default mode for LSP clients)
    #[arg(long, default_value = "true")]
    stdio: bool,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    if let Err(e) = validate_cpu_name(&args.cpu) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    let min_cpu = match args.cpu.as_str() {
        "68010" => m68k_core::cpu_gate::MinCpu::Mc68010,
        "68020" => m68k_core::cpu_gate::MinCpu::Mc68020,
        "68030" => m68k_core::cpu_gate::MinCpu::Mc68030,
        "68040" => m68k_core::cpu_gate::MinCpu::Mc68040,
        "68060" => m68k_core::cpu_gate::MinCpu::Mc68060,
        _ => m68k_core::cpu_gate::MinCpu::Mc68000,
    };

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(|client| {
        let backend = Backend::new(client);
        // Set initial configured CPU
        let config_clone = backend.config.clone();
        tokio::spawn(async move {
            let mut cfg = config_clone.write().await;
            *cfg = LinterConfig { cpu: min_cpu };
        });
        backend
    })
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
