//! Motorola 68000 addressing mode decoder.
//!
//! Handles all 12 EA modes plus 68020+ full-format indexing.

use std::collections::HashMap;

/// Register names.
pub const REG_DATA: [&str; 8] = ["d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7"];
pub const REG_ADDR: [&str; 8] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "sp"];

/// CPU level mapping for feature gating.
pub fn cpu_level(cpu: &str) -> u8 {
    match cpu {
        "68000" => 0,
        "68010" => 1,
        "68020" => 2,
        "68030" => 3,
        "68040" => 4,
        "68060" => 5,
        _ => 5,
    }
}

/// Instruction stream for reading raw bytes.
pub struct InstructionStream<'a> {
    pub data: &'a [u8],
    pub start_pc: u32,
    pub offset: usize,
}

impl<'a> InstructionStream<'a> {
    pub fn new(data: &'a [u8], start_pc: u32) -> Self {
        Self {
            data,
            start_pc,
            offset: 0,
        }
    }

    pub fn read_word(&mut self) -> Result<u16, String> {
        if self.offset + 2 > self.data.len() {
            return Err("Unexpected end of instruction stream".to_string());
        }
        let val = u16::from_be_bytes([self.data[self.offset], self.data[self.offset + 1]]);
        self.offset += 2;
        Ok(val)
    }

    pub fn read_long(&mut self) -> Result<u32, String> {
        if self.offset + 4 > self.data.len() {
            return Err("Unexpected end of instruction stream".to_string());
        }
        let val = u32::from_be_bytes([
            self.data[self.offset],
            self.data[self.offset + 1],
            self.data[self.offset + 2],
            self.data[self.offset + 3],
        ]);
        self.offset += 4;
        Ok(val)
    }

    pub fn current_pc(&self) -> u32 {
        self.start_pc + self.offset as u32
    }

    pub fn seek(&mut self, offset: usize) {
        self.offset = offset;
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }
}

/// Sign-extend an 8-bit value to i32.
#[inline]
pub fn sign_extend_8(val: u8) -> i32 {
    val as i8 as i32
}

/// Sign-extend a 16-bit value to i32.
#[inline]
pub fn sign_extend_16(val: u16) -> i32 {
    val as i16 as i32
}

/// Base trait for disassembled operands.
pub trait OperandTrait {
    fn format(&self, labels: &HashMap<u32, String>) -> String;
    fn size_hint(&self) -> Option<&str> {
        None
    }
}

/// Effective address operand types.
#[derive(Debug, Clone)]
pub enum EAOperand {
    /// Data register direct: Dn
    DataReg(u8),
    /// Address register direct: An
    AddrReg(u8),
    /// Address register indirect: (An)
    AddrIndirect(u8),
    /// Postincrement: (An)+
    PostInc(u8),
    /// Predecrement: -(An)
    PreDec(u8),
    /// Displacement: (d16,An)
    AddrDisp(u8, i32),
    /// Index: (d8,An,Xn.size)
    AddrIndex(u8, i8, String, String),
    /// Absolute short: xxx.W
    AbsoluteShort(u16),
    /// Absolute long: xxx.L
    AbsoluteLong(u32),
    /// PC relative displacement: (xxx).W(PC)
    PcDisp(i32, u32),
    /// PC relative index: (d8,PC,Xn.size)
    PcIndex(i32, u32, String, String),
    /// Immediate: #value
    Immediate(u64, String),
    /// Float immediate: #$hex
    FloatImm(Vec<u8>, String),
    /// Special register
    SpecialReg(String),
    /// 68020+ memory indirect / full index
    MemoryIndirect(MemoryIndirectOperand),
}

/// 68020+ memory indirect and full indexing operand.
#[derive(Debug, Clone)]
pub struct MemoryIndirectOperand {
    pub base_reg: Option<String>,
    pub base_disp: i32,
    pub bd_present: bool,
    pub index_reg: Option<String>,
    pub index_size: Option<String>,
    pub index_scale: u8,
    pub outer_disp: i32,
    pub od_present: bool,
    pub is_postindexed: bool,
    pub is_indirect: bool,
    pub target_pc_rel: bool,
    pub pc_rel_target: u32,
}

impl OperandTrait for EAOperand {
    fn format(&self, labels: &HashMap<u32, String>) -> String {
        match self {
            EAOperand::DataReg(n) => REG_DATA[*n as usize].to_string(),
            EAOperand::AddrReg(n) => REG_ADDR[*n as usize].to_string(),
            EAOperand::AddrIndirect(n) => format!("({})", REG_ADDR[*n as usize]),
            EAOperand::PostInc(n) => format!("({})+", REG_ADDR[*n as usize]),
            EAOperand::PreDec(n) => format!("-({})", REG_ADDR[*n as usize]),
            EAOperand::AddrDisp(n, disp) => {
                let reg = &REG_ADDR[*n as usize];
                format_disp(*disp, Some(reg), None)
            }
            EAOperand::AddrIndex(n, disp, idx_reg, idx_size) => {
                let reg = &REG_ADDR[*n as usize];
                format_disp(*disp as i32, Some(reg), Some((idx_reg, idx_size, &1)))
            }
            EAOperand::AbsoluteShort(addr) => {
                let se = sign_extend_16(*addr) as u32;
                if let Some(label) = labels.get(&se) {
                    label.clone()
                } else {
                    format!("${:08x}", se)
                }
            }
            EAOperand::AbsoluteLong(addr) => {
                if let Some(label) = labels.get(addr) {
                    label.clone()
                } else {
                    format!("${:08x}", addr)
                }
            }
            EAOperand::PcDisp(target, _disp) => {
                let addr = *target as u32;
                if let Some(label) = labels.get(&addr) {
                    format!("{}(pc)", label)
                } else {
                    format!("${:08x}(pc)", addr)
                }
            }
            EAOperand::PcIndex(target, _disp, idx_reg, idx_size) => {
                let addr = *target as u32;
                if let Some(label) = labels.get(&addr) {
                    format!("{}(pc,{}.{})", label, idx_reg, idx_size)
                } else {
                    format!("${:08x}(pc,{}.{})", addr, idx_reg, idx_size)
                }
            }
            EAOperand::Immediate(val, size) => {
                if size.to_uppercase() == "L"
                    && let Some(label) = labels.get(&(*val as u32))
                {
                    return format!("#{}", label);
                }
                match size.to_uppercase().as_str() {
                    "B" => format!("#${:02x}", val & 0xFF),
                    "W" => format!("#${:04x}", val & 0xFFFF),
                    _ => format!("#${:08x}", val & 0xFFFFFFFF),
                }
            }
            EAOperand::FloatImm(raw, _fmt) => format!("#${}", hex_bytes(raw)),
            EAOperand::SpecialReg(name) => name.clone(),
            EAOperand::MemoryIndirect(mi) => mi.format(labels),
        }
    }

    fn size_hint(&self) -> Option<&str> {
        if let EAOperand::Immediate(_, size) = self {
            Some(size.as_str())
        } else {
            None
        }
    }
}

impl OperandTrait for MemoryIndirectOperand {
    fn format(&self, labels: &HashMap<u32, String>) -> String {
        let bd_str = if self.target_pc_rel {
            if let Some(label) = labels.get(&self.pc_rel_target) {
                label.clone()
            } else {
                format!("${:08x}", self.pc_rel_target)
            }
        } else if self.bd_present {
            format_signed(self.base_disp)
        } else {
            String::new()
        };

        let idx_str = if let (Some(idx_reg), Some(idx_size)) = (&self.index_reg, &self.index_size) {
            let mut s = format!("{}.{}", idx_reg, idx_size.to_lowercase());
            if self.index_scale > 1 {
                s.push_str(&format!("*{}", self.index_scale));
            }
            Some(s)
        } else {
            None
        };

        let od_str = if self.od_present {
            Some(format_signed(self.outer_disp))
        } else {
            None
        };

        if !self.is_indirect {
            let mut parts = Vec::new();
            if !bd_str.is_empty() {
                parts.push(bd_str);
            }
            if let Some(base) = &self.base_reg {
                parts.push(base.clone());
            }
            if let Some(idx) = &idx_str {
                parts.push(idx.clone());
            }
            format!("({})", parts.join(","))
        } else if self.is_postindexed {
            let mut base_parts = Vec::new();
            if !bd_str.is_empty() {
                base_parts.push(bd_str);
            }
            if let Some(base) = &self.base_reg {
                base_parts.push(base.clone());
            }
            let inner = if base_parts.is_empty() {
                "[0]".to_string()
            } else {
                format!("[{}]", base_parts.join(","))
            };
            let mut outer = vec![inner];
            if let Some(idx) = &idx_str {
                outer.push(idx.clone());
            }
            if let Some(od) = &od_str {
                outer.push(od.clone());
            }
            format!("({})", outer.join(","))
        } else {
            let mut inner_parts = Vec::new();
            if !bd_str.is_empty() {
                inner_parts.push(bd_str);
            }
            if let Some(base) = &self.base_reg {
                inner_parts.push(base.clone());
            }
            if let Some(idx) = &idx_str {
                inner_parts.push(idx.clone());
            }
            let inner = if inner_parts.is_empty() {
                "[0]".to_string()
            } else {
                format!("[{}]", inner_parts.join(","))
            };
            let mut outer = vec![inner];
            if let Some(od) = &od_str {
                outer.push(od.clone());
            }
            format!("({})", outer.join(","))
        }
    }
}

fn format_signed(val: i32) -> String {
    if val < 0 {
        format!("-${:x}", val.abs())
    } else {
        format!("${:x}", val)
    }
}

fn format_disp(disp: i32, reg: Option<&str>, idx: Option<(&String, &String, &u8)>) -> String {
    let disp_str = format_signed(disp);
    if let Some(r) = reg {
        if let Some((idx_reg, idx_size, _scale)) = idx {
            format!("{}({},{}.{})", disp_str, r, idx_reg, idx_size)
        } else {
            format!("{}({})", disp_str, r)
        }
    } else {
        disp_str
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Parse a 68000 brief-format index extension word.
pub fn parse_index_extension(ext_word: u16) -> (String, String, i8) {
    let idx_type = if ext_word & 0x8000 != 0 { "a" } else { "d" };
    let idx_num = (ext_word >> 12) & 0x7;
    let idx_reg = format!("{}{}", idx_type, idx_num);
    let idx_size = if ext_word & 0x0800 != 0 { "l" } else { "w" };
    let disp = ext_word as i8;
    (idx_reg, idx_size.to_string(), disp)
}

/// Decode full 68020+ extension word format.
pub fn decode_full_ea(
    reg: u8,
    ext_word: u16,
    stream: &mut InstructionStream,
    _inst_pc: u32,
    is_pc: bool,
) -> Result<MemoryIndirectOperand, String> {
    let idx_type = if ext_word & 0x8000 != 0 { "a" } else { "d" };
    let idx_num = (ext_word >> 12) & 0x7;
    let idx_size = if ext_word & 0x0800 != 0 { "l" } else { "w" };
    let scale = [1, 2, 4, 8][((ext_word >> 9) & 0x3) as usize];

    let bs = ext_word & 0x0080 != 0;
    let is_suppress = ext_word & 0x0040 != 0;
    let bd_code = (ext_word >> 4) & 0x3;
    let i_i_s = ext_word & 0x07;

    let base_reg = if !bs {
        if is_pc {
            Some("pc".to_string())
        } else {
            Some(REG_ADDR[reg as usize].to_string())
        }
    } else {
        None
    };

    let index_reg = if !is_suppress {
        Some(format!("{}{}", idx_type, idx_num))
    } else {
        None
    };

    let mut bd: i32 = 0;
    let mut bd_present = false;
    if bd_code == 2 {
        bd = sign_extend_16(stream.read_word()?);
        bd_present = true;
    } else if bd_code == 3 {
        bd = stream.read_long()? as i32;
        bd_present = true;
    }

    let mut target_pc_rel = false;
    let mut pc_rel_target: u32 = 0;
    if is_pc && !bs {
        target_pc_rel = true;
        let bd_len = if bd_code == 2 {
            2
        } else if bd_code == 3 {
            4
        } else {
            0
        };
        // PRM: "The value of the PC is the address of the extension word" -
        // the base displacement's byte length must be subtracted, plus the
        // extension word itself (2 bytes), to land on the extension word's
        // own address rather than the position right after it. Verified
        // against real `vasm -m68020` output for `([target,pc],d1.w*2)`.
        pc_rel_target = stream
            .current_pc()
            .wrapping_sub(bd_len)
            .wrapping_sub(2)
            .wrapping_add(bd as u32);
    }

    let is_indirect = i_i_s > 0;
    // PRM Table 2-2: I/IS 1-3 (with IS=0) is Indirect Preindexed, I/IS 5-7 is
    // Indirect Postindexed - verified against real `vasm -m68020` output for
    // `([$10,a0,d1.w*2],$20)` (preindexed syntax, ext word 0x1322).
    let is_postindexed = matches!(i_i_s, 4..=7);

    let mut od: i32 = 0;
    let mut od_present = false;
    if is_indirect {
        if matches!(i_i_s, 2 | 6) {
            od = sign_extend_16(stream.read_word()?);
            od_present = true;
        } else if matches!(i_i_s, 3 | 7) {
            od = stream.read_long()? as i32;
            od_present = true;
        }
    }

    Ok(MemoryIndirectOperand {
        base_reg,
        base_disp: bd,
        bd_present,
        index_reg,
        index_size: if !is_suppress {
            Some(idx_size.to_string())
        } else {
            None
        },
        index_scale: scale,
        outer_disp: od,
        od_present,
        is_postindexed,
        is_indirect,
        target_pc_rel,
        pc_rel_target,
    })
}

/// Decode an effective address from mode/reg fields.
pub fn decode_ea(
    mode: u8,
    reg: u8,
    size: &str,
    stream: &mut InstructionStream,
    inst_pc: u32,
    cpu: &str,
) -> Result<EAOperand, String> {
    let level = cpu_level(cpu);

    match mode {
        0 => Ok(EAOperand::DataReg(reg)),
        1 => Ok(EAOperand::AddrReg(reg)),
        2 => Ok(EAOperand::AddrIndirect(reg)),
        3 => Ok(EAOperand::PostInc(reg)),
        4 => Ok(EAOperand::PreDec(reg)),
        5 => {
            let disp = sign_extend_16(stream.read_word()?);
            Ok(EAOperand::AddrDisp(reg, disp))
        }
        6 => {
            let ext_word = stream.read_word()?;
            if ext_word & 0x0100 == 0 || level < 2 {
                let (idx_reg, idx_size, disp) = parse_index_extension(ext_word);
                Ok(EAOperand::AddrIndex(reg, disp, idx_reg, idx_size))
            } else {
                let mi = decode_full_ea(reg, ext_word, stream, inst_pc, false)?;
                Ok(EAOperand::MemoryIndirect(mi))
            }
        }
        7 => match reg {
            0 => {
                let addr = stream.read_word()?;
                Ok(EAOperand::AbsoluteShort(addr))
            }
            1 => {
                let addr = stream.read_long()?;
                Ok(EAOperand::AbsoluteLong(addr))
            }
            2 => {
                let ext_pc = stream.current_pc();
                let disp = sign_extend_16(stream.read_word()?);
                let target = (ext_pc as i32 + disp) as u32;
                Ok(EAOperand::PcDisp(target as i32, target))
            }
            3 => {
                let ext_pc = stream.current_pc();
                let ext_word = stream.read_word()?;
                if ext_word & 0x0100 == 0 || level < 2 {
                    let (idx_reg, idx_size, disp) = parse_index_extension(ext_word);
                    let target = (ext_pc as i32 + disp as i32) as u32;
                    Ok(EAOperand::PcIndex(disp as i32, target, idx_reg, idx_size))
                } else {
                    let mi = decode_full_ea(reg, ext_word, stream, inst_pc, true)?;
                    Ok(EAOperand::MemoryIndirect(mi))
                }
            }
            4 => {
                let sz = size.to_uppercase();
                match sz.as_str() {
                    "B" => {
                        let val = stream.read_word()? as u64 & 0xFF;
                        Ok(EAOperand::Immediate(val, size.to_string()))
                    }
                    "W" => {
                        let val = stream.read_word()? as u64;
                        Ok(EAOperand::Immediate(val, size.to_string()))
                    }
                    "L" => {
                        let val = stream.read_long()? as u64;
                        Ok(EAOperand::Immediate(val, size.to_string()))
                    }
                    "S" | "D" | "X" | "P" => {
                        let n = match sz.as_str() {
                            "S" => 2,
                            "D" => 4,
                            "X" => 6,
                            "P" => 6,
                            _ => 1,
                        };
                        let mut raw = Vec::new();
                        for _ in 0..n {
                            let w = stream.read_word()?;
                            raw.push((w >> 8) as u8);
                            raw.push((w & 0xFF) as u8);
                        }
                        Ok(EAOperand::FloatImm(raw, size.to_string()))
                    }
                    _ => {
                        let val = stream.read_word()? as u64;
                        Ok(EAOperand::Immediate(val, size.to_string()))
                    }
                }
            }
            _ => Err(format!("Invalid register for mode 7: {}", reg)),
        },
        _ => Err(format!("Invalid addressing mode: {}", mode)),
    }
}

/// Format a register list bitmask into a string like "d0-d3/a0-a1".
pub fn format_reg_list(mask: u16, is_addr: bool) -> String {
    let regs = if is_addr { &REG_ADDR } else { &REG_DATA };
    let mut parts = Vec::new();
    let mut range_start: Option<usize> = None;

    for i in 0..8 {
        if mask & (1 << i) != 0 {
            if range_start.is_none() {
                range_start = Some(i);
            }
        } else if let Some(start) = range_start.take() {
            if i - 1 == start {
                parts.push(regs[start].to_string());
            } else {
                parts.push(format!("{}-{}", regs[start], regs[i - 1]));
            }
        }
    }
    if let Some(start) = range_start {
        if 7 == start {
            parts.push(regs[7].to_string());
        } else {
            parts.push(format!("{}-{}", regs[start], regs[7]));
        }
    }

    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_extend() {
        assert_eq!(sign_extend_8(0xFF), -1);
        assert_eq!(sign_extend_8(0x80), -128);
        assert_eq!(sign_extend_8(0x7F), 127);
        assert_eq!(sign_extend_16(0xFFFF), -1);
        assert_eq!(sign_extend_16(0x8000), -32768);
    }

    #[test]
    fn test_instruction_stream() {
        let data = [0x12, 0x34, 0x56, 0x78];
        let mut stream = InstructionStream::new(&data, 0x1000);
        assert_eq!(stream.current_pc(), 0x1000);
        assert_eq!(stream.read_word().unwrap(), 0x1234);
        assert_eq!(stream.current_pc(), 0x1002);
        assert_eq!(stream.read_word().unwrap(), 0x5678);
        assert!(stream.read_word().is_err());
    }

    #[test]
    fn test_decode_ea_dreg() {
        let data = [0x00, 0x00];
        let mut stream = InstructionStream::new(&data, 0);
        let ea = decode_ea(0, 3, "w", &mut stream, 0, "68000").unwrap();
        assert!(matches!(ea, EAOperand::DataReg(3)));
    }

    #[test]
    fn test_decode_ea_abshort() {
        let data = [0xFF, 0x80];
        let mut stream = InstructionStream::new(&data, 0);
        let ea = decode_ea(7, 0, "w", &mut stream, 0, "68000").unwrap();
        if let EAOperand::AbsoluteShort(addr) = ea {
            assert_eq!(addr, 0xFF80);
        } else {
            panic!("Expected AbsoluteShort");
        }
    }

    #[test]
    fn test_decode_ea_abslong() {
        let data = [0x00, 0x12, 0x34, 0x56];
        let mut stream = InstructionStream::new(&data, 0);
        let ea = decode_ea(7, 1, "l", &mut stream, 0, "68000").unwrap();
        if let EAOperand::AbsoluteLong(addr) = ea {
            assert_eq!(addr, 0x00123456);
        } else {
            panic!("Expected AbsoluteLong");
        }
    }

    #[test]
    fn test_format_reg_list() {
        assert_eq!(format_reg_list(0x000F, false), "d0-d3");
        assert_eq!(format_reg_list(0x0001, false), "d0");
        assert_eq!(format_reg_list(0x0005, false), "d0/d2");
        assert_eq!(format_reg_list(0x0003, true), "a0-a1");
    }

    #[test]
    fn test_cpu_level() {
        assert_eq!(cpu_level("68000"), 0);
        assert_eq!(cpu_level("68010"), 1);
        assert_eq!(cpu_level("68020"), 2);
        assert_eq!(cpu_level("68060"), 5);
    }
}
