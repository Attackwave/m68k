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
    pub platform: Option<String>,
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
    pub generated_asm: Option<String>,
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
    let platform_str = req.platform.as_deref().unwrap_or("amiga");

    let is_python = req.source.contains("def main")
        || req.source.contains("from m68k")
        || req.source.contains("import m68k")
        || req.source.starts_with("#!python")
        || req.source.starts_with("# Python");

    let (source_to_assemble, generated_asm) = if is_python {
        match crate::languages::python::transpile_python_to_m68k(&req.source, platform_str, cpu_str)
        {
            Ok(transpiled) => (transpiled.clone(), Some(transpiled)),
            Err(e) => {
                return AssembleResponse {
                    success: false,
                    byte_count: 0,
                    errors: vec![AssembleErrorItem {
                        line: 1,
                        message: format!("Python Transpiler Fehler: {}", e),
                    }],
                    warnings: Vec::new(),
                    binary_base64: None,
                    hex_dump: None,
                    generated_asm: None,
                };
            }
        }
    } else {
        (req.source.clone(), None)
    };

    let mut asm = Assembler::new(origin);
    asm.set_cpu(cpu_str);

    match asm.assemble(&source_to_assemble) {
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
                    format!("{:<48} |{}", hex_bytes, ascii)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &binary_data);

            AssembleResponse {
                success: true,
                byte_count: binary_data.len(),
                errors: Vec::new(),
                warnings: Vec::new(),
                binary_base64: Some(b64),
                hex_dump: Some(hex_dump),
                generated_asm,
            }
        }
        Err(e) => {
            let error_items = vec![AssembleErrorItem {
                line: e.line_no.unwrap_or(0),
                message: e.message,
            }];

            AssembleResponse {
                success: false,
                byte_count: 0,
                errors: error_items,
                warnings: Vec::new(),
                binary_base64: None,
                hex_dump: None,
                generated_asm,
            }
        }
    }
}
