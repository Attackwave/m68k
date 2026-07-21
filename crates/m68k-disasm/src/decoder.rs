//! Instruction decoder for the m68k disassembler.

use std::collections::HashMap;

use m68k_core::addressing::{
    EAOperand, InstructionStream, OperandTrait, cpu_level, decode_ea, sign_extend_8, sign_extend_16,
};
use m68k_core::opcodes::{ParserType, opcode_patterns};

pub const CONDITION_CODES: [&str; 16] = [
    "t", "f", "hi", "ls", "cc", "cs", "ne", "eq", "vc", "vs", "pl", "mi", "ge", "lt", "gt", "le",
];

#[derive(Debug, Clone)]
pub struct DecodedInstruction {
    pub address: u32,
    pub raw_bytes: Vec<u8>,
    pub mnemonic: String,
    pub operands: Vec<DecodedOperand>,
    pub target_address: Option<u32>,
}

impl DecodedInstruction {
    pub fn format(&self, labels: &HashMap<u32, String>) -> String {
        let ops: Vec<String> = self.operands.iter().map(|op| op.format(labels)).collect();
        let ops_str = ops.join(", ");
        if ops_str.is_empty() {
            self.mnemonic.clone()
        } else {
            format!("{:<8}{}", self.mnemonic, ops_str)
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedOperand {
    pub ea: Option<EAOperand>,
    pub special: Option<String>,
}

impl DecodedOperand {
    pub fn from_ea(ea: EAOperand) -> Self {
        Self {
            ea: Some(ea),
            special: None,
        }
    }

    pub fn special(name: impl Into<String>) -> Self {
        Self {
            ea: None,
            special: Some(name.into()),
        }
    }

    pub fn format(&self, labels: &HashMap<u32, String>) -> String {
        if let Some(ea) = &self.ea {
            ea.format(labels)
        } else if let Some(s) = &self.special {
            s.clone()
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone)]
pub struct DataWordInstruction {
    pub address: u32,
    pub raw_bytes: Vec<u8>,
    pub val: u16,
}

impl DataWordInstruction {
    pub fn format(&self, _labels: &HashMap<u32, String>) -> String {
        format!("dc.w     ${:04x}", self.val)
    }
}

#[derive(Debug, Clone)]
pub enum DecodeResult {
    Instruction(DecodedInstruction),
    DataWord(DataWordInstruction),
}

pub fn decode_next(
    stream: &mut InstructionStream,
    cpu_limit: &str,
) -> Result<(u32, DecodeResult), String> {
    let inst_pc = stream.current_pc();
    let start_offset = stream.offset;
    let op = stream.read_word()?;

    let patterns = opcode_patterns();
    let limit_level = cpu_level(cpu_limit);
    for pat in patterns {
        if (op & pat.mask) != pat.value {
            continue;
        }
        if cpu_level(pat.cpu) > limit_level {
            continue;
        }
        stream.seek(start_offset + 2);
        if let Ok((mnemonic, operands, target_addr)) =
            parse_operands(op, pat, stream, inst_pc, cpu_limit)
        {
            let raw = stream.data[start_offset..stream.offset].to_vec();
            return Ok((
                inst_pc,
                DecodeResult::Instruction(DecodedInstruction {
                    address: inst_pc,
                    raw_bytes: raw,
                    mnemonic,
                    operands,
                    target_address: target_addr,
                }),
            ));
        }
    }

    stream.seek(start_offset + 2);
    let raw = stream.data[start_offset..stream.offset].to_vec();
    Ok((
        inst_pc,
        DecodeResult::DataWord(DataWordInstruction {
            address: inst_pc,
            raw_bytes: raw,
            val: op,
        }),
    ))
}

fn parse_operands(
    op: u16,
    pat: &m68k_core::opcodes::OpcodePattern,
    stream: &mut InstructionStream,
    inst_pc: u32,
    cpu: &str,
) -> Result<(String, Vec<DecodedOperand>, Option<u32>), String> {
    let parser = pat.parser;
    let name = pat.mnemonic.to_lowercase();
    let mut operands: Vec<DecodedOperand> = Vec::new();
    let mut target_addr: Option<u32> = None;

    match parser {
        ParserType::None => Ok((name, operands, target_addr)),
        ParserType::Move => {
            let size_code = ((op >> 12) & 0x3) as u8;
            let size = match size_code {
                1 => "b",
                3 => "w",
                2 => "l",
                _ => return Err("invalid size code".into()),
            };
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_mode = ((op >> 6) & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((format!("move.{}", size), operands, target_addr))
        }
        ParserType::Movea => {
            let size_code = ((op >> 12) & 0x3) as u8;
            let size = if size_code == 3 { "w" } else { "l" };
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = EAOperand::AddrReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((format!("movea.{}", size), operands, target_addr))
        }
        ParserType::Moveq => {
            let data = (op & 0xFF) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let val = sign_extend_8(data);
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                val as u64 & 0xFFFFFFFF,
                "l".into(),
            )));
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(dst_reg)));
            Ok(("moveq".into(), operands, target_addr))
        }
        ParserType::MovemRm => {
            let size = if ((op >> 6) & 1) == 1 { "l" } else { "w" };
            let mode = ((op >> 3) & 0x7) as u8;
            let _reg = (op & 0x7) as u8;
            let mask = stream.read_word()?;
            let is_predec = mode == 4;
            let display_mask = if is_predec {
                let mut m = 0u16;
                for i in 0..16 {
                    if mask & (1 << i) != 0 {
                        m |= 1 << (15 - i);
                    }
                }
                m
            } else {
                mask
            };
            let regs = format_movem_list(display_mask);
            let ea = decode_ea(mode, _reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::special(regs));
            operands.push(DecodedOperand::from_ea(ea));
            Ok((format!("movem.{}", size), operands, target_addr))
        }
        ParserType::MovemMr => {
            let size = if ((op >> 6) & 1) == 1 { "l" } else { "w" };
            let mode = ((op >> 3) & 0x7) as u8;
            let _reg = (op & 0x7) as u8;
            let mask = stream.read_word()?;
            let regs = format_movem_list(mask);
            let ea = decode_ea(mode, _reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            operands.push(DecodedOperand::special(regs));
            Ok((format!("movem.{}", size), operands, target_addr))
        }
        ParserType::EaReg => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((format!("{}.{}", name, size), operands, target_addr))
        }
        ParserType::RegEa => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let src_reg = ((op >> 9) & 0x7) as u8;
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let src = EAOperand::DataReg(src_reg);
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((format!("{}.{}", name, size), operands, target_addr))
        }
        ParserType::Adda => {
            let size_code = ((op >> 8) & 0x1) as u8;
            let size = if size_code == 0 { "w" } else { "l" };
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = EAOperand::AddrReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::ImmEa => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let imm = read_immediate(size, stream)?;
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(imm));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Quick => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let data = ((op >> 9) & 0x7) as u8;
            let val = if data == 0 { 8 } else { data as u64 };
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                val,
                size.into(),
            )));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::SingleEa => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let size = pat.fixed_size.unwrap_or("w");
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Branch => {
            let cc = ((op >> 8) & 0xF) as usize;
            let cc_name = if cc == 0 {
                "bra"
            } else if cc == 1 {
                "bsr"
            } else {
                &format!("b{}", CONDITION_CODES[cc])
            };
            let disp = (op & 0xFF) as i8;
            let target = if disp == 0 {
                let ext_pc = stream.current_pc();
                let d16 = sign_extend_16(stream.read_word()?);
                (ext_pc as i32 + d16) as u32
            } else if disp == -1 {
                let ext_pc = stream.current_pc();
                let d32 = stream.read_long()? as i32;
                (ext_pc as i32 + d32) as u32
            } else {
                let ext_pc = stream.current_pc();
                (ext_pc as i32 + disp as i32) as u32
            };
            target_addr = Some(target);
            operands.push(DecodedOperand::special(format!("${:08x}", target)));
            Ok((cc_name.into(), operands, target_addr))
        }
        ParserType::Dbcc => {
            let cc = ((op >> 8) & 0xF) as usize;
            let cc_name = if cc == 1 {
                "dbra"
            } else {
                &format!("db{}", CONDITION_CODES[cc])
            };
            let reg = (op & 0x7) as u8;
            let ext_pc = stream.current_pc();
            let disp = sign_extend_16(stream.read_word()?);
            let target = (ext_pc as i32 + disp) as u32;
            target_addr = Some(target);
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
            operands.push(DecodedOperand::special(format!("${:08x}", target)));
            Ok((cc_name.into(), operands, target_addr))
        }
        ParserType::Scc => {
            let cc = ((op >> 8) & 0xF) as usize;
            let cc_name = if cc == 1 {
                "sf"
            } else {
                &format!("s{}", CONDITION_CODES[cc])
            };
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((cc_name.into(), operands, target_addr))
        }
        ParserType::BitReg => {
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let src_reg = ((op >> 9) & 0x7) as u8;
            let src = EAOperand::DataReg(src_reg);
            let size = if dst_mode == 0 { "l" } else { "b" };
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::BitImm => {
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let bit = stream.read_word()?;
            let size = if dst_mode == 0 { "l" } else { "b" };
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                bit as u64,
                "b".into(),
            )));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::ShiftReg => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let count_type = ((op >> 5) & 0x1) as u8;
            if count_type == 1 {
                let reg = ((op >> 9) & 0x7) as u8;
                let dst_reg = (op & 0x7) as u8;
                return Ok((
                    format!("{}.{}", name, size),
                    vec![
                        DecodedOperand::from_ea(EAOperand::DataReg(reg)),
                        DecodedOperand::from_ea(EAOperand::DataReg(dst_reg)),
                    ],
                    target_addr,
                ));
            }
            let count = ((op >> 9) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            if count == 0 {
                return Ok((
                    format!("{}.{}", name, size),
                    vec![
                        DecodedOperand::from_ea(EAOperand::Immediate(8, "b".into())),
                        DecodedOperand::from_ea(EAOperand::DataReg(dst_reg)),
                    ],
                    target_addr,
                ));
            }
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                count as u64,
                "b".into(),
            )));
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(dst_reg)));
            Ok((format!("{}.{}", name, size), operands, target_addr))
        }
        ParserType::ShiftMem => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let size = pat.fixed_size.unwrap_or("w");
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Exg => {
            let mode = ((op >> 3) & 0x1F) as u8;
            let regx = ((op >> 9) & 0x7) as u8;
            let regy = (op & 0x7) as u8;
            match mode {
                0x8 => {
                    operands.push(DecodedOperand::from_ea(EAOperand::DataReg(regx)));
                    operands.push(DecodedOperand::from_ea(EAOperand::DataReg(regy)));
                }
                0x9 => {
                    operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(regx)));
                    operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(regy)));
                }
                0x11 => {
                    operands.push(DecodedOperand::from_ea(EAOperand::DataReg(regx)));
                    operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(regy)));
                }
                _ => return Err(format!("invalid exg mode: {}", mode)),
            }
            Ok((name, operands, target_addr))
        }
        ParserType::Ext => {
            let size = if (op & 0x40) != 0 { "l" } else { "w" };
            let dst_reg = (op & 0x7) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(dst_reg)));
            Ok((format!("ext.{}", size), operands, target_addr))
        }
        ParserType::Link => {
            let reg = (op & 0x7) as u8;
            let disp = sign_extend_16(stream.read_word()?);
            operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(reg)));
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                disp as u64 & 0xFFFFFFFF,
                "w".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::Unlk => {
            let reg = (op & 0x7) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(reg)));
            Ok((name, operands, target_addr))
        }
        ParserType::Trap => {
            let vec = (op & 0xF) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                vec as u64,
                "b".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::Swap => {
            let reg = (op & 0x7) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
            Ok((name, operands, target_addr))
        }
        ParserType::Chk => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Lea => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "l", stream, inst_pc, cpu)?;
            let dst = EAOperand::AddrReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::AbcdSbcd => {
            let mode = ((op >> 3) & 0x1) as u8;
            let regx = ((op >> 9) & 0x7) as u8;
            let regy = (op & 0x7) as u8;
            if mode == 0 {
                operands.push(DecodedOperand::from_ea(EAOperand::DataReg(regx)));
                operands.push(DecodedOperand::from_ea(EAOperand::DataReg(regy)));
            } else {
                operands.push(DecodedOperand::from_ea(EAOperand::PostInc(regx)));
                operands.push(DecodedOperand::from_ea(EAOperand::PostInc(regy)));
            }
            Ok((name, operands, target_addr))
        }
        ParserType::MoveToCcr => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::special("ccr"));
            Ok(("move.w".into(), operands, target_addr))
        }
        ParserType::MoveFromSr => {
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let dst = decode_ea(dst_mode, dst_reg, "w", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::special("sr"));
            operands.push(DecodedOperand::from_ea(dst));
            Ok(("move.w".into(), operands, target_addr))
        }
        ParserType::MoveToSr => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::special("sr"));
            Ok(("move.w".into(), operands, target_addr))
        }
        ParserType::ImmCcr => {
            let imm = stream.read_word()?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                imm as u64,
                "w".into(),
            )));
            operands.push(DecodedOperand::special("ccr"));
            Ok((name, operands, target_addr))
        }
        ParserType::ImmSr => {
            let imm = stream.read_word()?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                imm as u64,
                "w".into(),
            )));
            operands.push(DecodedOperand::special("sr"));
            Ok((name, operands, target_addr))
        }
        ParserType::Tst => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Jsr => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "w", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Jmp => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "w", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Pea => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "l", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Clr => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Neg => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Negx => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Not => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Nbcd => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Tas => {
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let ea = decode_ea(mode, reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((name, operands, target_addr))
        }
        ParserType::Nop
        | ParserType::Rts
        | ParserType::Rte
        | ParserType::Rtr
        | ParserType::Trapv
        | ParserType::Illegal
        | ParserType::Reset => Ok((name, operands, target_addr)),
        ParserType::Stop => {
            let imm = stream.read_word()?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                imm as u64,
                "w".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::MoveUsp => {
            let dir = ((op >> 3) & 0x1) as u8;
            let reg = (op & 0x7) as u8;
            if dir == 0 {
                operands.push(DecodedOperand::special("usp"));
                operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(reg)));
            } else {
                operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(reg)));
                operands.push(DecodedOperand::special("usp"));
            }
            Ok((name, operands, target_addr))
        }
        ParserType::Cmpa => {
            let size_code = ((op >> 8) & 0x1) as u8;
            let size = if size_code == 0 { "w" } else { "l" };
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = EAOperand::AddrReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Cmpi => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let imm = read_immediate(size, stream)?;
            let mode = ((op >> 3) & 0x7) as u8;
            let reg = (op & 0x7) as u8;
            let dst = decode_ea(mode, reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(imm));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Cmpm => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::PostInc(src_reg)));
            operands.push(DecodedOperand::from_ea(EAOperand::PostInc(dst_reg)));
            Ok((format!("cmpm.{}", size), operands, target_addr))
        }
        ParserType::Cmp => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, size, stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Muls => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Mulu => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Divs => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Divu => {
            let src_mode = ((op >> 3) & 0x7) as u8;
            let src_reg = (op & 0x7) as u8;
            let dst_reg = ((op >> 9) & 0x7) as u8;
            let src = decode_ea(src_mode, src_reg, "w", stream, inst_pc, cpu)?;
            let dst = EAOperand::DataReg(dst_reg);
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Eor => {
            let size_code = ((op >> 6) & 0x3) as u8;
            if size_code >= 3 {
                return Err("invalid size code".into());
            }
            let size = ["b", "w", "l"][size_code as usize];
            let src_reg = ((op >> 9) & 0x7) as u8;
            let dst_mode = ((op >> 3) & 0x7) as u8;
            let dst_reg = (op & 0x7) as u8;
            let src = EAOperand::DataReg(src_reg);
            let dst = decode_ea(dst_mode, dst_reg, size, stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(src));
            operands.push(DecodedOperand::from_ea(dst));
            Ok((name, operands, target_addr))
        }
        ParserType::Rtd => {
            let imm = stream.read_word()?;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                imm as u64,
                "w".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::Bkpt => {
            let vec = (op & 0xF) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                vec as u64,
                "w".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::Movep => {
            let size_code = ((op >> 6) & 0x3) as u8;
            let _size = if size_code == 1 { "w" } else { "l" };
            let reg = ((op >> 9) & 0x7) as u8;
            let addr_reg = (op & 0x7) as u8;
            let disp = sign_extend_8(stream.read_word()? as u8);
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
            operands.push(DecodedOperand::from_ea(EAOperand::AddrDisp(addr_reg, disp)));
            Ok((name, operands, target_addr))
        }
    }
}

fn read_immediate(size: &str, stream: &mut InstructionStream) -> Result<EAOperand, String> {
    match size {
        "b" => {
            let val = stream.read_word()? as u64 & 0xFF;
            Ok(EAOperand::Immediate(val, "b".into()))
        }
        "w" => {
            let val = stream.read_word()? as u64;
            Ok(EAOperand::Immediate(val, "w".into()))
        }
        "l" => {
            let val = stream.read_long()? as u64;
            Ok(EAOperand::Immediate(val, "l".into()))
        }
        _ => Err(format!("unknown immediate size: {}", size)),
    }
}

fn format_movem_list(mask: u16) -> String {
    let mut regs = Vec::new();
    for i in 0..8 {
        if mask & (1 << i) != 0 {
            regs.push(format!("d{}", i));
        }
    }
    for i in 0..8 {
        if mask & (1 << (i + 8)) != 0 {
            regs.push(format!("a{}", i));
        }
    }
    if regs.is_empty() {
        return String::new();
    }

    fn group_regs(prefix: &str, regs: &[String]) -> Vec<String> {
        let mut nums: Vec<usize> = regs
            .iter()
            .filter(|r| r.starts_with(prefix))
            .map(|r| r[prefix.len()..].parse().unwrap())
            .collect();
        nums.sort();
        if nums.is_empty() {
            return vec![];
        }
        let mut groups = Vec::new();
        let mut start = nums[0];
        let mut prev = nums[0];
        for &n in &nums[1..] {
            if n == prev + 1 {
                prev = n;
            } else {
                if prev == start {
                    groups.push(format!("{}{}", prefix, start));
                } else {
                    groups.push(format!("{}{}-{}{}", prefix, start, prefix, prev));
                }
                start = n;
                prev = n;
            }
        }
        if prev == start {
            groups.push(format!("{}{}", prefix, start));
        } else {
            groups.push(format!("{}{}-{}{}", prefix, start, prefix, prev));
        }
        groups
    }

    let d_groups = group_regs("d", &regs);
    let a_groups = group_regs("a", &regs);
    [d_groups, a_groups].concat().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_nop() {
        let bytes = [0x4E, 0x71];
        let mut stream = InstructionStream::new(&bytes, 0);
        let (_addr, result) = decode_next(&mut stream, "68000").unwrap();
        if let DecodeResult::Instruction(inst) = result {
            assert_eq!(inst.mnemonic, "nop");
            assert!(inst.operands.is_empty());
        } else {
            panic!("Expected instruction");
        }
    }

    #[test]
    fn test_decode_rts() {
        let bytes = [0x4E, 0x75];
        let mut stream = InstructionStream::new(&bytes, 0);
        let (_addr, result) = decode_next(&mut stream, "68000").unwrap();
        if let DecodeResult::Instruction(inst) = result {
            assert_eq!(inst.mnemonic, "rts");
        } else {
            panic!("Expected instruction");
        }
    }

    #[test]
    fn test_decode_moveq() {
        let bytes = [0x70, 0x05];
        let mut stream = InstructionStream::new(&bytes, 0);
        let (_addr, result) = decode_next(&mut stream, "68000").unwrap();
        if let DecodeResult::Instruction(inst) = result {
            assert_eq!(inst.mnemonic, "moveq");
        } else {
            panic!("Expected instruction");
        }
    }

    #[test]
    fn test_decode_rtd() {
        let bytes = [0x4E, 0x74, 0x00, 0x0C];
        let mut stream = InstructionStream::new(&bytes, 0);
        let (_addr, result) = decode_next(&mut stream, "68010").unwrap();
        if let DecodeResult::Instruction(inst) = result {
            assert_eq!(inst.mnemonic, "rtd");
            assert_eq!(inst.operands.len(), 1);
        } else {
            panic!("Expected RTD instruction, got data word");
        }
    }

    #[test]
    fn test_golden_decoder() {
        let data = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/golden/vectors.json"
        ))
        .expect("Failed to read golden vectors");
        let golden: serde_json::Value = serde_json::from_str(&data).expect("Failed to parse");
        let tests = golden["decoder_tests"]
            .as_array()
            .expect("decoder_tests not found");

        for t in tests {
            let name = t["name"].as_str().unwrap();
            let input_hex = t["input_hex"].as_str().unwrap();
            let expected = t["expected"].as_str().unwrap();
            let cpu = t["cpu"].as_str().unwrap();

            let bytes =
                hex::decode(input_hex).unwrap_or_else(|_| panic!("Invalid hex in {}", name));
            let mut stream = InstructionStream::new(&bytes, 0);
            let (_addr, result) = decode_next(&mut stream, cpu)
                .unwrap_or_else(|_| panic!("Failed to decode {}", name));

            let labels = HashMap::new();
            let formatted = match result {
                DecodeResult::Instruction(inst) => inst.format(&labels),
                DecodeResult::DataWord(dw) => dw.format(&labels),
            };

            let expected_n = expected.replace(" ", "");
            let actual_n = formatted.replace(" ", "");
            assert_eq!(
                actual_n, expected_n,
                "Golden test '{}' failed: expected '{}', got '{}'",
                name, expected, formatted
            );
        }
    }
}
