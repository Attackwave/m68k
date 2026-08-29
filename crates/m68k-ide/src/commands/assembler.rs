//! Assembler build commands for m68k-ide.

use serde::{Deserialize, Serialize};

use m68k_asm::assembler::Assembler;
use m68k_asm::output::generate_binary;

/// Input parameters for assembling source code.
#[derive(Debug, Clone, Deserialize)]
pub struct AssembleRequest {
    pub source: String,
    pub cpu: Option<String>,
    pub base_address: Option<u32>,
}

/// Output result from assembling source code.
#[derive(Debug, Clone, Serialize)]
pub struct AssembleResponse {
    pub success: bool,
    pub byte_count: usize,
    pub errors: Vec<AssembleErrorItem>,
    pub warnings: Vec<String>,
    pub binary_base64: Option<String>,
    pub hex_dump: Option<String>,
}

/// Individual error item with source location.
#[derive(Debug, Clone, Serialize)]
pub struct AssembleErrorItem {
    pub line: usize,
    pub message: String,
}

pub fn assemble_code(req: AssembleRequest) -> AssembleResponse {
    let cpu_str = req.cpu.as_deref().unwrap_or("68000");
    let origin = req.base_address.unwrap_or(0);
    let mut asm = Assembler::new(origin);
    asm.set_cpu(cpu_str);

    match asm.assemble(&req.source) {
        Ok(_) => {
            let binary_data = if let Some((data, _)) = generate_binary(&asm.code) {
                data
            } else {
                Vec::new()
            };

            let hex_dump = binary_data
                .chunks(16)
                .map(|chunk| {
                    let hex_bytes = chunk
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii = chunk
                        .iter()
                        .map(|&b| {
                            if (32..=126).contains(&b) {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect::<String>();
                    format!("{:<48} | {} |", hex_bytes, ascii)
                })
                .collect::<Vec<_>>()
                .join("\n");

            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&binary_data);

            AssembleResponse {
                success: true,
                byte_count: binary_data.len(),
                errors: Vec::new(),
                warnings: Vec::new(),
                binary_base64: Some(b64),
                hex_dump: Some(hex_dump),
            }
        }
        Err(err) => {
            let mut errors: Vec<AssembleErrorItem> = asm
                .errors
                .errors
                .into_iter()
                .map(|e| AssembleErrorItem {
                    line: e.line_no.unwrap_or(0),
                    message: e.message,
                })
                .collect();

            if errors.is_empty() {
                errors.push(AssembleErrorItem {
                    line: err.line_no.unwrap_or(0),
                    message: err.message,
                });
            }

            let warnings = asm
                .errors
                .warnings
                .into_iter()
                .map(|w| format!("Line {}: {}", w.line_no.unwrap_or(0), w.message))
                .collect();

            AssembleResponse {
                success: false,
                byte_count: 0,
                errors,
                warnings,
                binary_base64: None,
                hex_dump: None,
            }
        }
    }
}
