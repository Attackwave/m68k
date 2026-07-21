//! EA (effective address) encoder for m68k assembler.

use m68k_core::ea_categories::ea_matches;
use m68k_core::errors::AsmError;
use m68k_core::operands::{MemoryIndirectOperand, Operand};

pub type EAEncoded = (u8, u8, Vec<u16>);

pub fn encode_ea(
    op: &Operand,
    size: &str,
    ext_pc: u32,
    allowed: u16,
    cpu: &str,
) -> Result<EAEncoded, AsmError> {
    let cpu_level = cpu_level(cpu);
    match op {
        Operand::DataReg(n) => {
            let (mode, reg) = (0, *n);
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![]))
        }
        Operand::AddrReg(n) => {
            let (mode, reg) = (1, *n);
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![]))
        }
        Operand::AddrRegIndirect(n) => {
            let (mode, reg) = (2, *n);
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![]))
        }
        Operand::AddrRegPostInc(n) => {
            let (mode, reg) = (3, *n);
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![]))
        }
        Operand::AddrRegPreDec(n) => {
            let (mode, reg) = (4, *n);
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![]))
        }
        Operand::AddrRegIndirectDisp(n, disp, _) => {
            let (mode, reg) = (5, *n);
            if *disp < -32768 || *disp > 32767 {
                return Err(AsmError::new("displacement out of 16-bit range"));
            }
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![to_word(*disp)]))
        }
        Operand::AddrRegIndirectIndex(n, disp, xreg, scale, is_long) => {
            let (mode, reg) = (6, *n);
            let xsize = if *is_long { "l" } else { "w" };
            let ext = encode_brief_index(*disp as i32, *xreg as u8, xsize, *scale, cpu_level)?;
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![ext]))
        }
        Operand::AbsoluteShort(addr) => {
            let (mode, reg) = (7, 0);
            let a = *addr;
            if !(-32768..=32767).contains(&a) {
                return Err(AsmError::new("absolute short address out of range"));
            }
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![to_word(a)]))
        }
        Operand::AbsoluteLong(addr) => {
            let (mode, reg) = (7, 1);
            let a = *addr;
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![to_word(a >> 16), to_word(a & 0xFFFF)]))
        }
        Operand::PcRelativeDisp(target, _) => {
            let (mode, reg) = (7, 2);
            let disp = (*target).wrapping_sub(ext_pc as i32 + 4);
            if !(-32768..=32767).contains(&disp) {
                return Err(AsmError::new("PC-relative displacement out of range"));
            }
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![to_word(disp)]))
        }
        Operand::PcRelativeIndex(disp, xreg, scale, is_long) => {
            let (mode, reg) = (7, 3);
            let xsize = if *is_long { "l" } else { "w" };
            let ext = encode_brief_index(*disp as i32, *xreg as u8, xsize, *scale, cpu_level)?;
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, vec![ext]))
        }
        Operand::Immediate(val) => {
            let (mode, reg) = (7, 4);
            let words = encode_immediate(*val, size)?;
            check_ea(mode, reg, allowed)?;
            Ok((mode, reg, words))
        }
        Operand::Bitfield(..) => Err(AsmError::new(
            "bitfield operand cannot be used as a plain EA",
        )),
        Operand::FpReg(_) | Operand::FpCtrlList(_) => Err(AsmError::new(
            "FPU register operand cannot be used as a plain EA",
        )),
        Operand::Special(_) => Err(AsmError::new(
            "special register operand cannot be used as a plain EA",
        )),
        Operand::RegPair(..) => Err(AsmError::new(
            "Dh:Dl/Dr:Dq register pair cannot be used as a plain EA",
        )),
        Operand::MemoryIndirect(mi) => encode_full_ea(mi, allowed, cpu_level),
        Operand::Memory(val) | Operand::Address(val) => {
            let addr = *val as u32;
            let addr16 = (addr & 0xFFFF) as i16;
            let se_addr = addr16 as i32;
            if se_addr as u32 == addr {
                let (mode, reg) = (7, 0);
                check_ea(mode, reg, allowed)?;
                Ok((mode, reg, vec![to_word(se_addr)]))
            } else {
                let (mode, reg) = (7, 1);
                check_ea(mode, reg, allowed)?;
                Ok((
                    mode,
                    reg,
                    vec![
                        to_word((addr >> 16) as i32),
                        to_word((addr & 0xFFFF) as i32),
                    ],
                ))
            }
        }
    }
}

fn cpu_level(cpu: &str) -> u8 {
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

fn to_word(val: i32) -> u16 {
    (val & 0xFFFF) as u16
}

fn check_ea(mode: u8, reg: u8, allowed: u16) -> Result<(), AsmError> {
    if !ea_matches(mode, reg, allowed) && allowed != 0xFFFF {
        Err(AsmError::new("addressing mode not allowed"))
    } else {
        Ok(())
    }
}

fn encode_immediate(value: i64, size: &str) -> Result<Vec<u16>, AsmError> {
    let sz = if size.is_empty() {
        "w"
    } else {
        &size.to_lowercase()
    };
    match sz {
        "b" => {
            if value > 255 {
                return Err(AsmError::new("byte immediate out of range"));
            }
            Ok(vec![to_word(value as i32)])
        }
        "w" => {
            if value < 0 {
                Ok(vec![to_word((value & 0xFFFF) as i32)])
            } else if value > 65535 {
                Err(AsmError::new("word immediate out of range"))
            } else {
                Ok(vec![to_word(value as i32)])
            }
        }
        "l" => Ok(vec![
            to_word((value >> 16) as i32),
            to_word((value & 0xFFFF) as i32),
        ]),
        _ => Ok(vec![to_word(value as i32)]),
    }
}

fn encode_brief_index(
    disp: i32,
    xreg: u8,
    xsize: &str,
    scale: u8,
    cpu_level: u8,
) -> Result<u16, AsmError> {
    if scale > 1 && cpu_level < 2 {
        return Err(AsmError::new("index scale > 1 requires CPU >= 68020"));
    }
    let is_areg = xreg >= 8;
    let xn = if is_areg { xreg - 8 } else { xreg };
    let long = if xsize == "l" { 1 } else { 0 };
    if ![1, 2, 4, 8].contains(&scale) {
        return Err(AsmError::new("invalid index scale"));
    }
    let scale_log2 = scale.trailing_zeros() as u8;
    if !(-128..=127).contains(&disp) {
        return Err(AsmError::new("brief index displacement out of range"));
    }
    let ext = ((if is_areg { 1 } else { 0 }) << 15)
        | ((xn as u16) << 12)
        | ((long as u16) << 11)
        | ((scale_log2 as u16) << 9)
        | (disp as u8 as u16);
    Ok(ext)
}

/// Encode a 68020+ full-format (memory indirect) EA. Mirrors `decode_full_ea`
/// in reverse; port of the Python reference's `_encode_full_ea`.
///
/// Extension word layout:
/// - bit 15: index is `An` (vs `Dn`)
/// - bit 14: reserved (0)
/// - bits 13-12: index register number
/// - bit 11: index size (1=`.L`, 0=`.W`)
/// - bits 10-9: scale (log2)
/// - bit 8: 1 (full format flag)
/// - bit 7: base register suppress
/// - bit 6: index suppress
/// - bits 5-4: base displacement size (0=null, 2=word, 3=long)
/// - bits 3-0: indirect/index/outer-displacement selector (i_i_s)
///
/// PC-relative full-format EAs (`base_is_pc`) are not supported — mirroring
/// the Python reference, which also leaves this case unimplemented.
fn encode_full_ea(
    mi: &MemoryIndirectOperand,
    allowed: u16,
    cpu_level: u8,
) -> Result<EAEncoded, AsmError> {
    if cpu_level < 2 {
        return Err(AsmError::new("full format EA requires CPU >= 68020"));
    }
    if mi.base_is_pc {
        return Err(AsmError::new("PC-relative full format EA is not supported"));
    }

    let base_suppress = mi.base_reg.is_none();
    let index_suppress = mi.index_reg.is_none();

    let bd_code: u16 = match mi.base_disp {
        None => 0,
        Some(bd) if (-32768..=32767).contains(&bd) => 2,
        Some(_) => 3,
    };

    // `MemoryIndirect` is only ever constructed for indirect forms (a plain
    // pre-indexed EA without indirection is `AddrRegIndirectIndex` instead),
    // so i_i_s is always in the indirect range (1-3 postindexed, 5-7 preindexed).
    let od_size: u16 = match mi.outer_disp {
        None => 0,
        Some(od) if (-32768..=32767).contains(&od) => 2,
        Some(_) => 3,
    };

    let iis_offset = match od_size {
        0 => 0,
        2 => 1,
        _ => 2,
    };
    let i_i_s: u16 = if mi.is_postindexed {
        1 + iis_offset
    } else {
        5 + iis_offset
    };

    let (idx_areg, idx_num, idx_long) = match mi.index_reg {
        Some(xreg) => {
            let is_areg = xreg >= 8;
            let xn = if is_areg { xreg - 8 } else { xreg };
            (is_areg, xn, mi.index_long)
        }
        None => (false, 0, false),
    };

    let scale_log2 = if index_suppress {
        0
    } else {
        if ![1, 2, 4, 8].contains(&mi.index_scale) {
            return Err(AsmError::new("invalid index scale"));
        }
        mi.index_scale.trailing_zeros() as u16
    };

    let ext_word = ((if idx_areg { 1 } else { 0 }) << 15)
        | ((idx_num as u16) << 12)
        | ((if idx_long { 1 } else { 0 }) << 11)
        | (scale_log2 << 9)
        | 0x0100 // full format flag
        | ((if base_suppress { 1 } else { 0 }) << 7)
        | ((if index_suppress { 1 } else { 0 }) << 6)
        | (bd_code << 4)
        | i_i_s;

    let (mode, reg) = (6, mi.base_reg.unwrap_or(0));
    check_ea(mode, reg, allowed)?;

    let mut ext_words = vec![ext_word];

    if let Some(bd) = mi.base_disp {
        if bd_code == 2 {
            if !(-32768..=32767).contains(&bd) {
                return Err(AsmError::new("full EA base displacement out of word range"));
            }
            ext_words.push(to_word(bd));
        } else {
            ext_words.push(to_word(bd >> 16));
            ext_words.push(to_word(bd & 0xFFFF));
        }
    }

    if let Some(od) = mi.outer_disp {
        if od_size == 2 {
            if !(-32768..=32767).contains(&od) {
                return Err(AsmError::new(
                    "full EA outer displacement out of word range",
                ));
            }
            ext_words.push(to_word(od));
        } else {
            ext_words.push(to_word(od >> 16));
            ext_words.push(to_word(od & 0xFFFF));
        }
    }

    Ok((mode, reg, ext_words))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_encode_imm_byte() {
        let (mode, reg, ext) =
            encode_ea(&Operand::Immediate(0xFF), "b", 0, 0xFFFF, "68000").unwrap();
        assert_eq!((mode, reg), (7, 4));
        assert_eq!(ext, vec![0x00FF]);
    }
    #[test]
    fn test_encode_indirect() {
        let (mode, reg, ext) =
            encode_ea(&Operand::AddrRegIndirect(2), "w", 0, 0xFFFF, "68000").unwrap();
        assert_eq!((mode, reg), (2, 2));
        assert!(ext.is_empty());
    }
    #[test]
    fn test_encode_imm_long() {
        let (mode, reg, ext) =
            encode_ea(&Operand::Immediate(0x12345678), "l", 0, 0xFFFF, "68000").unwrap();
        assert_eq!((mode, reg), (7, 4));
        assert_eq!(ext, vec![0x1234, 0x5678]);
    }
    #[test]
    fn test_encode_absolute_short() {
        let (mode, reg, ext) =
            encode_ea(&Operand::AbsoluteShort(0x1234), "w", 0, 0xFFFF, "68000").unwrap();
        assert_eq!((mode, reg), (7, 0));
        assert_eq!(ext, vec![0x1234]);
    }

    fn base_mi() -> MemoryIndirectOperand {
        MemoryIndirectOperand {
            base_reg: Some(0),
            base_is_pc: false,
            base_disp: None,
            index_reg: None,
            index_long: false,
            index_scale: 1,
            outer_disp: None,
            is_postindexed: false,
        }
    }

    #[test]
    fn test_encode_memory_indirect_requires_68020() {
        let op = Operand::MemoryIndirect(Box::new(base_mi()));
        let err = encode_ea(&op, "l", 0, 0xFFFF, "68000").unwrap_err();
        assert!(err.message.contains("68020"));
    }

    #[test]
    fn test_encode_memory_indirect_simple() {
        // ([A0]) -> mode=6 reg=0, ext = full-format flag only, bs=0, is=1(suppress index), i_i_s=5
        let op = Operand::MemoryIndirect(Box::new(base_mi()));
        let (mode, reg, ext) = encode_ea(&op, "l", 0, 0xFFFF, "68020").unwrap();
        assert_eq!((mode, reg), (6, 0));
        assert_eq!(ext, vec![0x0145]);
    }

    #[test]
    fn test_encode_memory_indirect_with_bd_and_index_preindexed() {
        // ([$10,A0,D1.W*2],$20)
        let mi = MemoryIndirectOperand {
            base_reg: Some(0),
            base_is_pc: false,
            base_disp: Some(0x10),
            index_reg: Some(1), // D1
            index_long: false,
            index_scale: 2,
            outer_disp: Some(0x20),
            is_postindexed: false,
        };
        let op = Operand::MemoryIndirect(Box::new(mi));
        let (mode, reg, ext) = encode_ea(&op, "l", 0, 0xFFFF, "68020").unwrap();
        assert_eq!((mode, reg), (6, 0));
        assert_eq!(ext, vec![0x1326, 0x0010, 0x0020]);
    }

    #[test]
    fn test_encode_memory_indirect_postindexed() {
        // ([$10,A0],D1.W*2,$20)
        let mi = MemoryIndirectOperand {
            base_reg: Some(0),
            base_is_pc: false,
            base_disp: Some(0x10),
            index_reg: Some(1),
            index_long: false,
            index_scale: 2,
            outer_disp: Some(0x20),
            is_postindexed: true,
        };
        let op = Operand::MemoryIndirect(Box::new(mi));
        let (mode, reg, ext) = encode_ea(&op, "l", 0, 0xFFFF, "68020").unwrap();
        assert_eq!((mode, reg), (6, 0));
        assert_eq!(ext, vec![0x1322, 0x0010, 0x0020]);
    }

    #[test]
    fn test_encode_memory_indirect_pc_relative_rejected() {
        let mi = MemoryIndirectOperand {
            base_reg: None,
            base_is_pc: true,
            base_disp: Some(0x10),
            index_reg: None,
            index_long: false,
            index_scale: 1,
            outer_disp: None,
            is_postindexed: false,
        };
        let op = Operand::MemoryIndirect(Box::new(mi));
        let err = encode_ea(&op, "l", 0, 0xFFFF, "68020").unwrap_err();
        assert!(err.message.contains("PC-relative"));
    }
}
