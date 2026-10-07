//! Amiga Copperlist parser and visualizer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CopperOpType {
    Move,
    Wait,
    Skip,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopperInstructionItem {
    pub op_type: CopperOpType,
    pub word1: u16,
    pub word2: u16,
    pub description: String,
    pub vpos: Option<u16>,
    pub hpos: Option<u16>,
    pub reg_name: Option<String>,
    pub color_preview_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseCopperResponse {
    pub instructions: Vec<CopperInstructionItem>,
}

fn amiga_custom_reg_name(offset: u16) -> &'static str {
    match offset & 0x01FE {
        0x0080 => "COP1LCH",
        0x0082 => "COP1LCL",
        0x0084 => "COP2LCH",
        0x0086 => "COP2LCL",
        0x0088 => "COPJMP1",
        0x008A => "COPJMP2",
        0x00E0 => "BPL1PTH",
        0x00E2 => "BPL1PTL",
        0x00E4 => "BPL2PTH",
        0x00E6 => "BPL2PTL",
        0x00E8 => "BPL3PTH",
        0x00EA => "BPL3PTL",
        0x00EC => "BPL4PTH",
        0x00EE => "BPL4PTL",
        0x0100 => "BPLCON0",
        0x0102 => "BPLCON1",
        0x0104 => "BPLCON2",
        0x0108 => "BPL1MOD",
        0x010A => "BPL2MOD",
        0x0180 => "COLOR00 (Background)",
        0x0182 => "COLOR01",
        0x0184 => "COLOR02",
        0x0186 => "COLOR03",
        0x0188 => "COLOR04",
        0x018A => "COLOR05",
        0x018C => "COLOR06",
        0x018E => "COLOR07",
        0x0190 => "COLOR08",
        0x0192 => "COLOR09",
        0x0194 => "COLOR10",
        0x0196 => "COLOR11",
        0x0198 => "COLOR12",
        0x019A => "COLOR13",
        0x019C => "COLOR14",
        0x019E => "COLOR15",
        _ => "CUSTOM_REG",
    }
}

pub fn parse_copperlist(bytes: Vec<u8>) -> ParseCopperResponse {
    let mut instructions = Vec::new();

    for chunk in bytes.chunks_exact(4) {
        let w1 = u16::from_be_bytes([chunk[0], chunk[1]]);
        let w2 = u16::from_be_bytes([chunk[2], chunk[3]]);

        if w1 == 0xFFFF && (w2 == 0xFFFE || w2 == 0xFFFF) {
            instructions.push(CopperInstructionItem {
                op_type: CopperOpType::End,
                word1: w1,
                word2: w2,
                description: "END OF COPPERLIST (WAIT $FF,$FE)".to_string(),
                vpos: None,
                hpos: None,
                reg_name: None,
                color_preview_hex: None,
            });
            break;
        }

        if (w1 & 1) == 0 {
            // MOVE instruction
            let reg_offset = w1 & 0x01FE;
            let reg_name = amiga_custom_reg_name(reg_offset);

            let color_preview_hex = if (0x0180..=0x01BE).contains(&reg_offset) {
                let r = (w2 >> 8) & 0x0F;
                let g = (w2 >> 4) & 0x0F;
                let b = w2 & 0x0F;
                Some(format!("#{:01X}{:01X}{:01X}", r, g, b))
            } else {
                None
            };

            instructions.push(CopperInstructionItem {
                op_type: CopperOpType::Move,
                word1: w1,
                word2: w2,
                description: format!("MOVE ${:04X} -> {} (${:03X})", w2, reg_name, reg_offset),
                vpos: None,
                hpos: None,
                reg_name: Some(reg_name.to_string()),
                color_preview_hex,
            });
        } else if (w2 & 1) == 0 {
            // WAIT instruction
            let vpos = w1 >> 8;
            let hpos = w1 & 0xFE;
            instructions.push(CopperInstructionItem {
                op_type: CopperOpType::Wait,
                word1: w1,
                word2: w2,
                description: format!("WAIT VPOS={}, HPOS={}", vpos, hpos),
                vpos: Some(vpos),
                hpos: Some(hpos),
                reg_name: None,
                color_preview_hex: None,
            });
        } else {
            // SKIP instruction
            let vpos = w1 >> 8;
            let hpos = w1 & 0xFE;
            instructions.push(CopperInstructionItem {
                op_type: CopperOpType::Skip,
                word1: w1,
                word2: w2,
                description: format!("SKIP IF BEAM > VPOS={}, HPOS={}", vpos, hpos),
                vpos: Some(vpos),
                hpos: Some(hpos),
                reg_name: None,
                color_preview_hex: None,
            });
        }
    }

    ParseCopperResponse { instructions }
}
