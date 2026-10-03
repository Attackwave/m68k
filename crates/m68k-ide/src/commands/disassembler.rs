//! Disassembler commands for m68k-ide.

use serde::{Deserialize, Serialize};

use m68k_disasm::disassembler::Disassembler;

#[derive(Debug, Clone, Deserialize)]
pub struct DisassembleRequest {
    pub bytes: Vec<u8>,
    pub origin: Option<u32>,
    pub cpu: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisassembleLineItem {
    pub address: u32,
    pub address_hex: String,
    pub instruction_hex: String,
    pub text: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisassembleResponse {
    pub lines: Vec<DisassembleLineItem>,
}

pub fn disassemble_bytes(req: DisassembleRequest) -> DisassembleResponse {
    let cpu_str = req.cpu.as_deref().unwrap_or("68000");
    let origin = req.origin.unwrap_or(0);
    let mut dis = Disassembler::new(req.bytes, origin);
    dis.set_cpu(cpu_str);

    let output_lines = dis.disassemble();

    let lines = output_lines
        .into_iter()
        .map(|l| DisassembleLineItem {
            address: l.address,
            address_hex: format!("${:08X}", l.address),
            instruction_hex: l
                .raw_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
            text: l.text,
            comment: l.comment,
        })
        .collect();

    DisassembleResponse { lines }
}
