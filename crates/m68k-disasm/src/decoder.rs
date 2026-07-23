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
        } else if self.mnemonic.len() < 8 {
            format!("{:<8}{}", self.mnemonic, ops_str)
        } else {
            format!("{} {}", self.mnemonic, ops_str)
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
            operands.push(DecodedOperand::from_ea(EAOperand::AbsoluteLong(target)));
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
            operands.push(DecodedOperand::from_ea(EAOperand::AbsoluteLong(target)));
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
        ParserType::LinkLong => {
            let reg = (op & 0x7) as u8;
            let hi = stream.read_word()? as u32;
            let lo = stream.read_word()? as u32;
            let disp = (hi << 16) | lo;
            operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(reg)));
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                disp as u64,
                "l".into(),
            )));
            Ok((format!("{}.l", name), operands, target_addr))
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
            let vec = (op & 0x7) as u8;
            operands.push(DecodedOperand::from_ea(EAOperand::Immediate(
                vec as u64,
                "w".into(),
            )));
            Ok((name, operands, target_addr))
        }
        ParserType::Movep => {
            // Opmode field (bits 7-6): 00=word mem->reg, 01=long mem->reg,
            // 10=word reg->mem, 11=long reg->mem (bit 7 = direction, bit 6 =
            // size) - matches enc_movep's op_mode 4/5/6/7 encoding.
            let opmode = (op >> 6) & 0x3;
            let to_mem = opmode & 0x2 != 0;
            let size = if opmode & 0x1 != 0 { "l" } else { "w" };
            let reg = ((op >> 9) & 0x7) as u8;
            let addr_reg = (op & 0x7) as u8;
            let disp = sign_extend_8(stream.read_word()? as u8);
            let dreg = DecodedOperand::from_ea(EAOperand::DataReg(reg));
            let mem = DecodedOperand::from_ea(EAOperand::AddrDisp(addr_reg, disp));
            if to_mem {
                operands.push(dreg);
                operands.push(mem);
            } else {
                operands.push(mem);
                operands.push(dreg);
            }
            Ok((format!("{}.{}", name, size), operands, target_addr))
        }
        ParserType::Bitfield => {
            // The bitfield extension word precedes any EA extension words
            // (e.g. an absolute address or displacement) — verified against
            // real `vasm -m68020` output, and matches how the assembler's
            // encoder orders them (opword, ext, then EA extension words).
            let ext = stream.read_word()?;
            let ea_mode = ((op >> 3) & 0x7) as u8;
            let ea_reg = (op & 0x7) as u8;
            let ea = decode_ea(ea_mode, ea_reg, "b", stream, inst_pc, cpu)?;

            let sub = (op >> 8) & 0x7;
            let (mnemonic, has_dst_reg, dst_reg_is_source) = match sub {
                0 => ("bftst", false, false),
                1 => ("bfextu", true, false),
                2 => ("bfchg", false, false),
                3 => ("bfexts", true, false),
                4 => ("bfclr", false, false),
                5 => ("bfffo", true, false),
                6 => ("bfset", false, false),
                7 => ("bfins", true, true),
                _ => unreachable!("3-bit field"),
            };

            let offset_str = if (ext & 0x0800) != 0 {
                format!("d{}", (ext >> 6) & 0x7)
            } else {
                format!("{}", (ext >> 6) & 0x1F)
            };
            let width_str = if (ext & 0x0020) != 0 {
                format!("d{}", ext & 0x7)
            } else {
                let w = ext & 0x1F;
                format!("{}", if w == 0 { 32 } else { w })
            };
            let empty_labels = HashMap::new();
            let bitfield_operand = format!(
                "{}{{{}:{}}}",
                ea.format(&empty_labels),
                offset_str,
                width_str
            );

            if has_dst_reg {
                let reg = ((ext >> 12) & 0x7) as u8;
                if dst_reg_is_source {
                    operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
                    operands.push(DecodedOperand::special(bitfield_operand));
                } else {
                    operands.push(DecodedOperand::special(bitfield_operand));
                    operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
                }
            } else {
                operands.push(DecodedOperand::special(bitfield_operand));
            }

            Ok((mnemonic.into(), operands, target_addr))
        }
        ParserType::PflushFamily040 => {
            let opmode = (op >> 3) & 0x3;
            let reg = (op & 0x7) as u8;
            let mnemonic = match opmode {
                0b00 => "pflushn",
                0b01 => "pflush",
                0b10 => "pflushan",
                0b11 => "pflusha",
                _ => unreachable!("2-bit field"),
            };
            if opmode <= 0b01 {
                operands.push(DecodedOperand::from_ea(EAOperand::AddrIndirect(reg)));
            }
            Ok((mnemonic.into(), operands, target_addr))
        }
        ParserType::Pmove => {
            let ext = stream.read_word()?;
            let group_prefix = (ext >> 13) & 0x7;

            match group_prefix {
                0b001 => {
                    // PFLUSHA (mode 001, no EA/FC/mask) or PFLUSH FC,MASK,<ea>
                    // (mode 110). Verified against real `vasm -m68030`
                    // output for `pflusha` (F000 2400) and
                    // `pflush #2,#3,(a0)` (F010 3872).
                    let mode = (ext >> 10) & 0x7;
                    if mode == 0b001 {
                        Ok(("pflusha".into(), operands, target_addr))
                    } else {
                        let ea_mode = ((op >> 3) & 0x7) as u8;
                        let ea_reg = (op & 0x7) as u8;
                        let ea = decode_ea(ea_mode, ea_reg, "l", stream, inst_pc, cpu)?;
                        let mask = (ext >> 5) & 0x7;
                        let fc = ext & 0x1F;
                        operands.push(DecodedOperand::special(format!("#{}", fc & 0x7)));
                        operands.push(DecodedOperand::special(format!("#{}", mask)));
                        operands.push(DecodedOperand::from_ea(ea));
                        Ok(("pflush".into(), operands, target_addr))
                    }
                }
                0b100 => {
                    // PTESTR/PTESTW FC,<ea>,#level[,An]. Verified against
                    // real `vasm -m68030` output for `ptestr #2,(a0),#3`
                    // (F010 8E12), `ptestw #2,(a0),#3` (F010 8C12), and
                    // `ptestr #2,(a0),#3,a1` (F010 8F32).
                    let level = (ext >> 10) & 0x7;
                    let rw = (ext >> 9) & 1;
                    let a_bit = (ext >> 8) & 1;
                    let an = ((ext >> 5) & 0x7) as u8;
                    let fc = ext & 0x1F;
                    let mnemonic = if rw == 1 { "ptestr" } else { "ptestw" };

                    let ea_mode = ((op >> 3) & 0x7) as u8;
                    let ea_reg = (op & 0x7) as u8;
                    let ea = decode_ea(ea_mode, ea_reg, "l", stream, inst_pc, cpu)?;

                    operands.push(DecodedOperand::special(format!("#{}", fc & 0x7)));
                    operands.push(DecodedOperand::from_ea(ea));
                    operands.push(DecodedOperand::special(format!("#{}", level)));
                    if a_bit == 1 {
                        operands.push(DecodedOperand::from_ea(EAOperand::AddrReg(an)));
                    }
                    Ok((mnemonic.into(), operands, target_addr))
                }
                0b000 | 0b010 | 0b011 => {
                    // PMOVE <ea>,MRn / PMOVE MRn,<ea>. Verified against
                    // real `vasm -m68030` output for `pmove tc,(a0)`
                    // (F010 4200) and `pmove (a0),tc` (F010 4000).
                    let mmu_field = (ext >> 10) & 0x3F;
                    let reg_name = match (group_prefix, (mmu_field & 0x7)) {
                        (0b010, 0b000) => "tc",
                        (0b010, 0b010) => "srp",
                        (0b010, 0b011) => "crp",
                        (0b000, 0b010) => "tt0",
                        (0b000, 0b011) => "tt1",
                        (0b011, _) => "mmusr",
                        _ => return Err("unknown PMOVE MMU register group".into()),
                    };
                    let rw = (ext >> 9) & 1;

                    let ea_mode = ((op >> 3) & 0x7) as u8;
                    let ea_reg = (op & 0x7) as u8;
                    let ea = decode_ea(ea_mode, ea_reg, "l", stream, inst_pc, cpu)?;

                    if rw == 0 {
                        // memory-to-register
                        operands.push(DecodedOperand::from_ea(ea));
                        operands.push(DecodedOperand::special(reg_name));
                    } else {
                        // register-to-memory
                        operands.push(DecodedOperand::special(reg_name));
                        operands.push(DecodedOperand::from_ea(ea));
                    }
                    Ok(("pmove".into(), operands, target_addr))
                }
                _ => Err("unknown MMU extension word group prefix".into()),
            }
        }
        ParserType::CacheOp040 => {
            let scope = (op >> 6) & 0x3;
            let push = (op >> 5) & 1;
            let unit = (op >> 3) & 0x3;
            let reg = (op & 0x7) as u8;

            let mnemonic = match (push, unit) {
                (0, 0b01) => "cinvl",
                (0, 0b10) => "cinvp",
                (0, 0b11) => "cinva",
                (1, 0b01) => "cpushl",
                (1, 0b10) => "cpushp",
                (1, 0b11) => "cpusha",
                _ => return Err("unknown cache operation unit field".into()),
            };

            operands.push(DecodedOperand::special(format!("#{}", scope)));
            if unit != 0b11 {
                operands.push(DecodedOperand::from_ea(EAOperand::AddrIndirect(reg)));
            }
            Ok((mnemonic.into(), operands, target_addr))
        }
        ParserType::Fpu => decode_fpu_cpgen(op, stream, inst_pc, cpu, target_addr),
        ParserType::FpuScc => {
            let cc = stream.read_word()? & 0x3F;
            let mnemonic = format!("fs{}", fpu_cc_name(cc)?);
            let ea_mode = ((op >> 3) & 0x7) as u8;
            let ea_reg = (op & 0x7) as u8;
            let ea = decode_ea(ea_mode, ea_reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok((mnemonic, operands, target_addr))
        }
        ParserType::FpuDbcc => {
            let reg = (op & 0x7) as u8;
            let cc = stream.read_word()? & 0x3F;
            // Displacement is relative to the PC after the condition word (opword + cc
            // word), not after the opword alone - verified against real vasm output for
            // `fdbeq d0,$1010` at pc=$1000: pc_after_ext=$1004, disp=$0c, target=$1010.
            let ext_pc = stream.current_pc();
            let disp = sign_extend_16(stream.read_word()?);
            let target = (ext_pc as i32 + disp) as u32;
            target_addr = Some(target);
            let mnemonic = format!("fdb{}", fpu_cc_name(cc)?);
            operands.push(DecodedOperand::from_ea(EAOperand::DataReg(reg)));
            operands.push(DecodedOperand::from_ea(EAOperand::AbsoluteLong(target)));
            Ok((mnemonic, operands, target_addr))
        }
        ParserType::FpuTrapcc => {
            let mode = op & 0x7;
            let cc = stream.read_word()? & 0x3F;
            let base_mnemonic = format!("ftrap{}", fpu_cc_name(cc)?);
            let mnemonic = match mode {
                2 => {
                    let val = stream.read_word()?;
                    operands.push(DecodedOperand::special(format!("#${:04x}", val)));
                    format!("{}.w", base_mnemonic)
                }
                3 => {
                    let val = stream.read_long()?;
                    operands.push(DecodedOperand::special(format!("#${:08x}", val)));
                    format!("{}.l", base_mnemonic)
                }
                4 => base_mnemonic,
                _ => return Err("unknown FTRAPcc mode".into()),
            };
            Ok((mnemonic, operands, target_addr))
        }
        ParserType::FpuBcc => {
            let size_bit = (op >> 6) & 1;
            let cc = op & 0x1F;
            let mnemonic = format!("fb{}", fpu_cc_name(cc)?);
            let ext_pc = stream.current_pc();
            let target = if size_bit == 1 {
                let d32 = stream.read_long()? as i32;
                (ext_pc as i32 + d32) as u32
            } else {
                let d16 = sign_extend_16(stream.read_word()?);
                (ext_pc as i32 + d16) as u32
            };
            target_addr = Some(target);
            operands.push(DecodedOperand::from_ea(EAOperand::AbsoluteLong(target)));
            Ok((mnemonic, operands, target_addr))
        }
        ParserType::FpuSave => {
            let ea_mode = ((op >> 3) & 0x7) as u8;
            let ea_reg = (op & 0x7) as u8;
            let ea = decode_ea(ea_mode, ea_reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok(("fsave".into(), operands, target_addr))
        }
        ParserType::FpuRestore => {
            let ea_mode = ((op >> 3) & 0x7) as u8;
            let ea_reg = (op & 0x7) as u8;
            let ea = decode_ea(ea_mode, ea_reg, "b", stream, inst_pc, cpu)?;
            operands.push(DecodedOperand::from_ea(ea));
            Ok(("frestore".into(), operands, target_addr))
        }
    }
}

/// FPU condition-code mnemonic suffix (lowercase) for a 6-bit (0-31) condition value.
fn fpu_cc_name(cc: u16) -> Result<&'static str, String> {
    Ok(match cc & 0x1F {
        0x00 => "f",
        0x01 => "eq",
        0x02 => "ogt",
        0x03 => "oge",
        0x04 => "olt",
        0x05 => "ole",
        0x06 => "ogl",
        0x07 => "or",
        0x08 => "un",
        0x09 => "ueq",
        0x0A => "ugt",
        0x0B => "uge",
        0x0C => "ult",
        0x0D => "ule",
        0x0E => "ne",
        0x0F => "t",
        0x10 => "sf",
        0x11 => "seq",
        0x12 => "gt",
        0x13 => "ge",
        0x14 => "lt",
        0x15 => "le",
        0x16 => "gl",
        0x17 => "gle",
        0x18 => "ngle",
        0x19 => "ngl",
        0x1A => "nle",
        0x1B => "nlt",
        0x1C => "nge",
        0x1D => "ngt",
        0x1E => "sne",
        0x1F => "st",
        _ => unreachable!("5-bit field"),
    })
}

/// FPU source-format specifier (extension word bits 12-10) to size-suffix character.
fn fpu_fmt_suffix(fmt: u16) -> &'static str {
    match fmt & 0x7 {
        0 => "l",
        1 => "s",
        2 => "x",
        3 => "p",
        4 => "w",
        5 => "d",
        6 => "b",
        _ => "x",
    }
}

/// FPU monadic/dyadic arithmetic opclass/opmode (extension word bits 6-0) to mnemonic.
fn fpu_arith_name(cmd: u16) -> Option<&'static str> {
    Some(match cmd {
        0x00 => "fmove",
        0x01 => "fint",
        0x02 => "fsinh",
        0x03 => "fintrz",
        0x04 => "fsqrt",
        0x06 => "flognp1",
        0x08 => "fetoxm1",
        0x09 => "ftanh",
        0x0A => "fatan",
        0x0B => "ftan",
        0x0C => "fasin",
        0x0D => "fatanh",
        0x0E => "fsin",
        0x0F => "ftentox",
        0x10 => "ftwotox",
        0x11 => "fetox",
        0x12 => "flog10",
        0x14 => "flog2",
        0x15 => "flogn",
        0x18 => "fabs",
        0x19 => "fcosh",
        0x1A => "fneg",
        0x1C => "facos",
        0x1D => "fcos",
        0x1E => "fgetexp",
        0x1F => "fgetman",
        0x20 => "fdiv",
        0x21 => "fmod",
        0x22 => "fadd",
        0x23 => "fmul",
        0x24 => "fsgldiv",
        0x25 => "frem",
        0x26 => "fscale",
        0x27 => "fsglmul",
        0x28 => "fsub",
        0x38 => "fcmp",
        0x3A => "ftst",
        _ => return None,
    })
}

/// "Short" (rounding-precision-forcing) FPU opclass/opmode to mnemonic.
fn fpu_short_name(cmd: u16) -> Option<&'static str> {
    Some(match cmd {
        0x40 => "fsmove",
        0x41 => "fssqrt",
        0x44 => "fdmove",
        0x45 => "fdsqrt",
        0x60 => "fsdiv",
        0x62 => "fsadd",
        0x63 => "fsmul",
        0x64 => "fddiv",
        0x66 => "fdadd",
        0x67 => "fdmul",
        0x68 => "fssub",
        0x6C => "fdsub",
        _ => return None,
    })
}

/// FPU control-register mask (extension word bits 12-10) to register-list text,
/// e.g. `fpcr/fpsr/fpiar` for mask 0b111.
fn fpu_ctrl_list_name(mask: u16) -> String {
    let mut parts = Vec::new();
    if mask & 0b100 != 0 {
        parts.push("fpcr");
    }
    if mask & 0b010 != 0 {
        parts.push("fpsr");
    }
    if mask & 0b001 != 0 {
        parts.push("fpiar");
    }
    parts.join("/")
}

/// Reverse an 8-bit FPU register mask (see `enc_fpu::reverse_fp_mask` for the encoder side:
/// the postincrement/control static-list format has FP0 as the MSB, opposite of predecrement).
fn reverse_fp_mask(mask: u8) -> u8 {
    let mut out = 0u8;
    for i in 0..8 {
        if (mask >> i) & 1 != 0 {
            out |= 1 << (7 - i);
        }
    }
    out
}

/// Format an FPU data-register mask (bit N = FPn) as `fp0/fp2-fp4` register-list text.
fn fpu_reg_list_text(mask: u8) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0u8;
    while i < 8 {
        if (mask >> i) & 1 != 0 {
            let start = i;
            let mut end = i;
            while end + 1 < 8 && (mask >> (end + 1)) & 1 != 0 {
                end += 1;
            }
            if end > start {
                parts.push(format!("fp{}-fp{}", start, end));
            } else {
                parts.push(format!("fp{}", start));
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    parts.join("/")
}

/// Decode the FPU cpGEN group (opword base 0xF200): arithmetic, FMOVE, FMOVECR, FSINCOS,
/// and FMOVEM, all dispatched on the extension word's top bits (15-13) and, for the
/// arithmetic/FMOVE cases, the R/M and opclass/opmode fields.
fn decode_fpu_cpgen(
    op: u16,
    stream: &mut InstructionStream,
    inst_pc: u32,
    cpu: &str,
    target_addr: Option<u32>,
) -> Result<(String, Vec<DecodedOperand>, Option<u32>), String> {
    let ext = stream.read_word()?;
    let mut operands: Vec<DecodedOperand> = Vec::new();
    let group = (ext >> 13) & 0x7;
    let ea_mode = ((op >> 3) & 0x7) as u8;
    let ea_reg = (op & 0x7) as u8;

    match group {
        0b110 | 0b111 => {
            // FMOVEM data-register list <-> memory. bit13 (part of `group`) is dr,
            // bits12-11 are list-mode (00/01=predecrement static/dynamic,
            // 10/11=postincrement-or-control static/dynamic).
            let dr = (ext >> 13) & 1;
            let list_mode = (ext >> 11) & 0x3;
            let is_predec = list_mode & 0b10 == 0;
            let raw_mask = (ext & 0xFF) as u8;
            let mask = if is_predec {
                raw_mask
            } else {
                reverse_fp_mask(raw_mask)
            };
            let reg_list = fpu_reg_list_text(mask);
            let ea = decode_ea(ea_mode, ea_reg, "x", stream, inst_pc, cpu)?;
            if dr == 1 {
                operands.push(DecodedOperand::special(reg_list));
                operands.push(DecodedOperand::from_ea(ea));
            } else {
                operands.push(DecodedOperand::from_ea(ea));
                operands.push(DecodedOperand::special(reg_list));
            }
            Ok(("fmovem".into(), operands, target_addr))
        }
        0b100 | 0b101 => {
            // FMOVE <-> FPU control register(s) (FPCR/FPSR/FPIAR). bit13 is dr
            // (1 = ctrl -> EA, 0 = EA -> ctrl); bits12-10 are the control-register mask.
            let dr = (ext >> 13) & 1;
            let mask = (ext >> 10) & 0x7;
            let reg_list = fpu_ctrl_list_name(mask);
            let ea = decode_ea(ea_mode, ea_reg, "l", stream, inst_pc, cpu)?;
            if dr == 1 {
                operands.push(DecodedOperand::special(reg_list));
                operands.push(DecodedOperand::from_ea(ea));
            } else {
                operands.push(DecodedOperand::from_ea(ea));
                operands.push(DecodedOperand::special(reg_list));
            }
            Ok(("fmove".into(), operands, target_addr))
        }
        0b010 | 0b011 => {
            // <ea>,FPn (group 010) or FPn,<ea> (group 011): arithmetic, FMOVE,
            // FMOVECR (src format specifier 0b111), or FSINCOS (cmd bits 6-5 = 01).
            let rm_dst = ((ext >> 7) & 0x7) as u8;
            let cmd = ext & 0x7F;
            let fmt = (ext >> 10) & 0x7;

            if group == 0b010 && fmt == 0b111 {
                // FMOVECR #rom_offset,FPn: no EA is consumed (bits 5-0 of op are ignored
                // by real hardware here, but stay in the same opword as a normal cpGEN op).
                let rom_offset = ext & 0x7F;
                operands.push(DecodedOperand::special(format!("#${:x}", rom_offset)));
                operands.push(DecodedOperand::special(format!("fp{}", rm_dst)));
                return Ok(("fmovecr".into(), operands, target_addr));
            }
            if group == 0b010 && (cmd & 0x78) == 0x30 {
                // FSINCOS <ea>,FPc:FPd - cmd bits 6-3 = 0110, cos_dst in cmd bits 2-0,
                // sin_dst (the "destination register" field) in ext bits 9-7.
                let cos_dst = cmd & 0x7;
                let ea = decode_ea(ea_mode, ea_reg, fpu_fmt_suffix(fmt), stream, inst_pc, cpu)?;
                operands.push(DecodedOperand::from_ea(ea));
                operands.push(DecodedOperand::special(format!(
                    "fp{}:fp{}",
                    cos_dst, rm_dst
                )));
                return Ok(("fsincos".into(), operands, target_addr));
            }

            let base_mnemonic = fpu_arith_name(cmd)
                .or_else(|| fpu_short_name(cmd))
                .ok_or("unknown FPU arithmetic opclass/opmode")?;
            let suffix = fpu_fmt_suffix(fmt);
            let ea = decode_ea(ea_mode, ea_reg, suffix, stream, inst_pc, cpu)?;
            // Only append the size suffix when it isn't the assembler's default (word,
            // verified against real vasm output for unsuffixed <ea>,FPn arithmetic/FMOVE),
            // so a plain round-trip doesn't spuriously widen to e.g. `fadd.w`.
            let mnemonic = if suffix == "w" {
                base_mnemonic.to_string()
            } else {
                format!("{}.{}", base_mnemonic, suffix)
            };
            if group == 0b010 {
                operands.push(DecodedOperand::from_ea(ea));
                operands.push(DecodedOperand::special(format!("fp{}", rm_dst)));
            } else {
                operands.push(DecodedOperand::special(format!("fp{}", rm_dst)));
                operands.push(DecodedOperand::from_ea(ea));
            }
            Ok((mnemonic, operands, target_addr))
        }
        0b000 => {
            // FPn,FPm (reg-reg) or FPn (monadic, src==dst): R/M=0, source register in
            // bits 12-10, destination register in bits 9-7, opclass/opmode in bits 6-0.
            let src_reg = (ext >> 10) & 0x7;
            let dst_reg = (ext >> 7) & 0x7;
            let cmd = ext & 0x7F;

            if (cmd & 0x78) == 0x30 {
                // FSINCOS FPn,FPc:FPd (reg-reg form).
                let cos_dst = cmd & 0x7;
                operands.push(DecodedOperand::special(format!("fp{}", src_reg)));
                operands.push(DecodedOperand::special(format!(
                    "fp{}:fp{}",
                    cos_dst, dst_reg
                )));
                return Ok(("fsincos".into(), operands, target_addr));
            }

            let mnemonic = fpu_arith_name(cmd)
                .or_else(|| fpu_short_name(cmd))
                .ok_or("unknown FPU arithmetic opclass/opmode")?;
            operands.push(DecodedOperand::special(format!("fp{}", src_reg)));
            operands.push(DecodedOperand::special(format!("fp{}", dst_reg)));
            Ok((mnemonic.into(), operands, target_addr))
        }
        _ => Err("unknown FPU cpGEN extension-word group".into()),
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

    // FPU decoder tests. Reference bytes verified against real `vasm -m68040` output
    // (see crates/m68k-asm/src/enc_fpu.rs for the matching encoder-side verification).
    fn decode_one(bytes: &[u8], cpu: &str) -> DecodedInstruction {
        let mut stream = InstructionStream::new(bytes, 0x1000);
        match decode_next(&mut stream, cpu).unwrap().1 {
            DecodeResult::Instruction(inst) => inst,
            DecodeResult::DataWord(_) => panic!("expected instruction, got data word"),
        }
    }

    #[test]
    fn test_decode_fadd_dn_default_word() {
        // vasm: fadd d0,fp2 -> f2005122 (no suffix -> word specifier, not extended)
        let inst = decode_one(&[0xF2, 0x00, 0x51, 0x22], "68040");
        assert_eq!(inst.mnemonic, "fadd");
    }

    #[test]
    fn test_decode_fadd_dn_long_suffix_shown() {
        // vasm: fadd.l d0,fp2 -> f2004122
        let inst = decode_one(&[0xF2, 0x00, 0x41, 0x22], "68040");
        assert_eq!(inst.mnemonic, "fadd.l");
    }

    #[test]
    fn test_decode_fadd_reg_reg() {
        // vasm: fadd fp1,fp2 -> f2000522
        let inst = decode_one(&[0xF2, 0x00, 0x05, 0x22], "68040");
        assert_eq!(inst.mnemonic, "fadd");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "fp1");
        assert_eq!(inst.operands[1].format(&labels), "fp2");
    }

    #[test]
    fn test_decode_fmove_ctrl_reg_direction() {
        // vasm: fmove fpiar,d0 -> f200a400 (ctrl -> EA) vs fmove d0,fpiar -> f2008400 (EA -> ctrl)
        let labels = HashMap::new();
        let to_dn = decode_one(&[0xF2, 0x00, 0xA4, 0x00], "68040");
        assert_eq!(to_dn.operands[0].format(&labels), "fpiar");
        assert_eq!(to_dn.operands[1].format(&labels), "d0");

        let from_dn = decode_one(&[0xF2, 0x00, 0x84, 0x00], "68040");
        assert_eq!(from_dn.operands[0].format(&labels), "d0");
        assert_eq!(from_dn.operands[1].format(&labels), "fpiar");
    }

    #[test]
    fn test_decode_fmovem_predec_mask_not_reversed() {
        // vasm: fmovem fp0/fp1,-(a7) -> f227e003
        let inst = decode_one(&[0xF2, 0x27, 0xE0, 0x03], "68040");
        assert_eq!(inst.mnemonic, "fmovem");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "fp0-fp1");
    }

    #[test]
    fn test_decode_fmovem_nonpredec_mask_reversed() {
        // vasm: fmovem fp0/fp1,(a0) -> f210f0c0 (postincrement/control: mask stored reversed)
        let inst = decode_one(&[0xF2, 0x10, 0xF0, 0xC0], "68040");
        assert_eq!(inst.mnemonic, "fmovem");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "fp0-fp1");
    }

    #[test]
    fn test_decode_fmovem_mem_to_regs_mask_reversed() {
        // vasm: fmovem (a0)+,fp0/fp1 -> f218d0c0
        let inst = decode_one(&[0xF2, 0x18, 0xD0, 0xC0], "68040");
        assert_eq!(inst.mnemonic, "fmovem");
        let labels = HashMap::new();
        assert_eq!(inst.operands[1].format(&labels), "fp0-fp1");
    }

    #[test]
    fn test_decode_fmovecr() {
        // vasm: fmovecr #0,fp0 -> f2005c00
        let inst = decode_one(&[0xF2, 0x00, 0x5C, 0x00], "68040");
        assert_eq!(inst.mnemonic, "fmovecr");
    }

    #[test]
    fn test_decode_fsincos_reg_reg() {
        // vasm: fsincos fp1,fp2:fp3 -> f20005b2
        let inst = decode_one(&[0xF2, 0x00, 0x05, 0xB2], "68040");
        assert_eq!(inst.mnemonic, "fsincos");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "fp1");
        assert_eq!(inst.operands[1].format(&labels), "fp2:fp3");
    }

    #[test]
    fn test_decode_fbeq_word() {
        // vasm: fbeq $1010 (at pc=$1000) -> f281000e
        let inst = decode_one(&[0xF2, 0x81, 0x00, 0x0E], "68040");
        assert_eq!(inst.mnemonic, "fbeq");
        assert_eq!(inst.target_address, Some(0x1010));
    }

    #[test]
    fn test_decode_fbeq_long() {
        // vasm: fbeq.l $1010 (at pc=$1000) -> f2c10000000e
        let inst = decode_one(&[0xF2, 0xC1, 0x00, 0x00, 0x00, 0x0E], "68040");
        assert_eq!(inst.mnemonic, "fbeq");
        assert_eq!(inst.target_address, Some(0x1010));
    }

    #[test]
    fn test_decode_fdbeq() {
        // vasm: fdbeq d0,$1010 (at pc=$1000) -> f2480001000c
        let inst = decode_one(&[0xF2, 0x48, 0x00, 0x01, 0x00, 0x0C], "68040");
        assert_eq!(inst.mnemonic, "fdbeq");
        assert_eq!(inst.target_address, Some(0x1010));
    }

    #[test]
    fn test_decode_fseq() {
        // vasm: fseq d0 -> f2400001
        let inst = decode_one(&[0xF2, 0x40, 0x00, 0x01], "68040");
        assert_eq!(inst.mnemonic, "fseq");
    }

    #[test]
    fn test_decode_ftrapeq_no_operand() {
        // vasm: ftrapeq -> f27c0001
        let inst = decode_one(&[0xF2, 0x7C, 0x00, 0x01], "68040");
        assert_eq!(inst.mnemonic, "ftrapeq");
        assert!(inst.operands.is_empty());
    }

    #[test]
    fn test_decode_ftrapeq_word() {
        // vasm: ftrapeq.w #1234 -> f27a000104d2
        let inst = decode_one(&[0xF2, 0x7A, 0x00, 0x01, 0x04, 0xD2], "68040");
        assert_eq!(inst.mnemonic, "ftrapeq.w");
    }

    #[test]
    fn test_decode_ftrapeq_long() {
        // vasm: ftrapeq.l #12345678 -> f27b000100bc614e
        let inst = decode_one(&[0xF2, 0x7B, 0x00, 0x01, 0x00, 0xBC, 0x61, 0x4E], "68040");
        assert_eq!(inst.mnemonic, "ftrapeq.l");
    }

    #[test]
    fn test_decode_fnop() {
        // vasm: fnop -> f2800000 (encoded as fbf with zero displacement)
        let inst = decode_one(&[0xF2, 0x80, 0x00, 0x00], "68040");
        assert_eq!(inst.mnemonic, "fbf");
    }

    #[test]
    fn test_decode_fsave() {
        // vasm: fsave -(a0) -> f320
        let inst = decode_one(&[0xF3, 0x20], "68040");
        assert_eq!(inst.mnemonic, "fsave");
    }

    #[test]
    fn test_decode_frestore() {
        // vasm: frestore (a0)+ -> f358
        let inst = decode_one(&[0xF3, 0x58], "68040");
        assert_eq!(inst.mnemonic, "frestore");
    }

    #[test]
    fn test_decode_fpu_requires_68020_or_later() {
        let bytes = [0xF2, 0x00, 0x05, 0x22];
        let mut stream = InstructionStream::new(&bytes, 0x1000);
        // On 68000, the F-line opcode has no matching pattern and falls back to a data word.
        let (_addr, result) = decode_next(&mut stream, "68000").unwrap();
        assert!(matches!(result, DecodeResult::DataWord(_)));
    }

    // MOVEP/BKPT decoder tests. Reference bytes verified against real `vasm -m68020`
    // output. Regression tests for a pattern-ordering/masking bug: MOVEP's original
    // mask only matched the mem->reg/.w opmode and was itself shadowed by BTST's
    // wider mask; BKPT was shadowed by PEA.

    #[test]
    fn test_decode_movep_all_opmodes() {
        // vasm: movep.w 4(a0),d0 / movep.l 4(a0),d0 / movep.w d0,4(a0) / movep.l d0,4(a0)
        // -> 0108 0004 / 0148 0004 / 0188 0004 / 01c8 0004
        let cases: [(u16, &str); 4] = [
            (0x0108, "movep.w"),
            (0x0148, "movep.l"),
            (0x0188, "movep.w"),
            (0x01C8, "movep.l"),
        ];
        for (op, expected_mnemonic) in cases {
            let bytes = [(op >> 8) as u8, op as u8, 0x00, 0x04];
            let inst = decode_one(&bytes, "68020");
            assert_eq!(inst.mnemonic, expected_mnemonic);
        }
    }

    #[test]
    fn test_decode_movep_direction() {
        // vasm: movep.w 4(a0),d0 -> mem-to-reg (Dn is the destination, i.e. second operand)
        let inst = decode_one(&[0x01, 0x08, 0x00, 0x04], "68020");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "$4(a0)");
        assert_eq!(inst.operands[1].format(&labels), "d0");

        // vasm: movep.w d0,4(a0) -> reg-to-mem (Dn is the source, i.e. first operand)
        let inst = decode_one(&[0x01, 0x88, 0x00, 0x04], "68020");
        assert_eq!(inst.operands[0].format(&labels), "d0");
        assert_eq!(inst.operands[1].format(&labels), "$4(a0)");
    }

    #[test]
    fn test_decode_movep_does_not_shadow_btst_bchg_bclr_bset() {
        // vasm: btst d0,d1 / bchg d0,d1 / bclr d0,d1 / bset d0,d1 -> 0101/0141/0181/01c1
        let cases: [(u16, &str); 4] = [
            (0x0101, "btst"),
            (0x0141, "bchg"),
            (0x0181, "bclr"),
            (0x01C1, "bset"),
        ];
        for (op, expected_mnemonic) in cases {
            let inst = decode_one(&[(op >> 8) as u8, op as u8], "68020");
            assert_eq!(inst.mnemonic, expected_mnemonic);
        }
    }

    #[test]
    fn test_decode_bkpt() {
        // vasm: bkpt #5 -> 484d
        let inst = decode_one(&[0x48, 0x4D], "68020");
        assert_eq!(inst.mnemonic, "bkpt");
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "#$0005");
    }

    #[test]
    fn test_decode_bkpt_does_not_shadow_pea() {
        // vasm: pea (a0) -> 4850
        let inst = decode_one(&[0x48, 0x50], "68020");
        assert_eq!(inst.mnemonic, "pea");
    }

    #[test]
    fn test_decode_link_l() {
        // vasm: LINK.L A5,#$12345678 -> 480D 1234 5678
        let inst = decode_one(&[0x48, 0x0D, 0x12, 0x34, 0x56, 0x78], "68020");
        assert_eq!(inst.mnemonic, "link.l");
    }

    #[test]
    fn test_decode_link_l_does_not_shadow_nbcd() {
        // vasm: nbcd (a0) -> 4810
        let inst = decode_one(&[0x48, 0x10], "68020");
        assert_eq!(inst.mnemonic, "nbcd");
    }

    #[test]
    fn test_decode_memory_indirect_preindexed_vs_postindexed() {
        // vasm: move.l ([$10,a0,d1.w*2],$20),d2 -> 2430 1322 0010 0020 (preindexed)
        //       move.l ([$10,a0],d1.w*2,$20),d2 -> 2430 1326 0010 0020 (postindexed)
        // Regression test for a bug where the two forms were swapped.
        let labels = HashMap::new();

        let pre = decode_one(&[0x24, 0x30, 0x13, 0x22, 0x00, 0x10, 0x00, 0x20], "68020");
        assert_eq!(pre.operands[0].format(&labels), "([$10,a0,d1.w*2],$20)");

        let post = decode_one(&[0x24, 0x30, 0x13, 0x26, 0x00, 0x10, 0x00, 0x20], "68020");
        assert_eq!(post.operands[0].format(&labels), "([$10,a0],d1.w*2,$20)");
    }

    #[test]
    fn test_decode_memory_indirect_pc_relative_target_address() {
        // vasm: move.l ([$1010,pc],d1.w*2),d2 (at pc=$1000) -> 243b 1325 000e
        // Regression test for the PC-relative target being off by 2 (the base
        // displacement length was subtracted but not the extension word's own
        // 2 bytes, landing on the position after the ext word instead of at it).
        let mut stream = InstructionStream::new(&[0x24, 0x3B, 0x13, 0x25, 0x00, 0x0E], 0x1000);
        let (_addr, result) = decode_next(&mut stream, "68020").unwrap();
        let inst = match result {
            DecodeResult::Instruction(inst) => inst,
            DecodeResult::DataWord(_) => panic!("expected instruction, got data word"),
        };
        let labels = HashMap::new();
        assert_eq!(inst.operands[0].format(&labels), "([$00001010,pc],d1.w*2)");
    }

    #[test]
    fn test_decode_exg_all_three_forms() {
        // vasm: exg d0,d1 -> 0xC141, exg a0,a1 -> 0xC149, exg d2,a3 -> 0xC58B.
        // Regression test for the EXG patterns' mask being too narrow (0xF1C0,
        // missing bit 3 of the 5-bit opmode field), which let the three EXG
        // forms collide with each other and let AND's wider mask swallow them.
        let dd = decode_one(&[0xC1, 0x41], "68000");
        assert_eq!(dd.mnemonic, "exg");

        let aa = decode_one(&[0xC1, 0x49], "68000");
        assert_eq!(aa.mnemonic, "exg");

        let da = decode_one(&[0xC5, 0x8B], "68000");
        assert_eq!(da.mnemonic, "exg");
    }

    #[test]
    fn test_decode_exg_does_not_shadow_and() {
        // vasm: and.l d0,d1 -> c280, and.l d0,(a1) -> c191
        let and1 = decode_one(&[0xC2, 0x80], "68000");
        assert_eq!(and1.mnemonic, "and.l");

        let and2 = decode_one(&[0xC1, 0x91], "68000");
        assert_eq!(and2.mnemonic, "and.l");
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
