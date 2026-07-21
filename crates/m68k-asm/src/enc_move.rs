//! Instruction encoders for MOVE family: MOVE, MOVEA, MOVEM, MOVEQ.

use m68k_core::ea_categories::ea::*;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

fn size_code(size: &str) -> Result<u8, AsmError> {
    match size.to_lowercase().as_str() {
        "b" => Ok(0),
        "w" => Ok(1),
        "l" => Ok(2),
        _ => Err(AsmError::new("invalid size")),
    }
}

/// Encode MOVE instruction.
pub fn enc_move(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let move_size = [1, 3, 2][sz as usize];
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, ALL, cpu)?;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;

    let op = ((move_size as u16) << 12)
        | ((dst_reg as u16) << 9)
        | ((dst_mode as u16) << 6)
        | ((src_mode as u16) << 3)
        | (src_reg as u16);

    let mut words = vec![op];
    words.extend(dst_ext);
    words.extend(src_ext);
    Ok(words)
}

/// Encode MOVEA instruction.
pub fn enc_movea(
    src: &Operand,
    dst_reg: u8,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;

    let op = 0x3000
        | ((sz as u16) << 12)
        | ((dst_reg as u16) << 9)
        | ((src_mode as u16) << 3)
        | (src_reg as u16);
    let mut words = vec![op];
    words.extend(src_ext);
    Ok(words)
}

/// Encode MOVEQ instruction.
pub fn enc_moveq(data: i8, dst_reg: u8) -> Result<Vec<u16>, AsmError> {
    let op = 0x7000 | ((dst_reg as u16) << 9) | ((data as u8) as u16);
    Ok(vec![op])
}

/// Encode MOVEM instruction (register to memory).
pub fn enc_movem_rm(
    reg_mask: u16,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let is_predec = matches!(dst, Operand::AddrRegPreDec(_));
    let display_mask = if is_predec {
        let mut m = 0u16;
        for i in 0..16 {
            if reg_mask & (1 << i) != 0 {
                m |= 1 << (15 - i);
            }
        }
        m
    } else {
        reg_mask
    };

    let allowed = AREG_IND | APOSTINC | APREDEC | ABSW | ABSL | PCDISP | PCINDEXED | AINDEXED;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;

    if dst_mode < 2 {
        return Err(AsmError::new("MOVEM does not support Dn/An direct"));
    }

    let size_bit = if size.to_lowercase() == "l" { 1 } else { 0 };
    let op =
        0x4800 | 0x0080 | ((size_bit as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op, display_mask];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode MOVEM instruction (memory to register).
pub fn enc_movem_mr(
    src: &Operand,
    reg_mask: u16,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let allowed = AREG_IND | APOSTINC | APREDEC | ABSW | ABSL | PCDISP | PCINDEXED | AINDEXED;
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, allowed, cpu)?;

    if src_mode < 2 {
        return Err(AsmError::new("MOVEM does not support Dn/An direct"));
    }

    let size_bit = if size.to_lowercase() == "l" { 1 } else { 0 };
    let op =
        0x4C00 | 0x0080 | ((size_bit as u16) << 6) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op, reg_mask];
    words.extend(src_ext);
    Ok(words)
}

/// Encode MOVEP instruction: Dn ↔ (An, disp) for peripheral data transfer.
pub fn enc_movep(src: &Operand, dst: &Operand, size: &str) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "w" => 0,
        "l" => 1,
        _ => return Err(AsmError::new("invalid size for MOVEP")),
    };
    let (data_reg, addr_reg, disp, to_mem) = match (src, dst) {
        (Operand::DataReg(dr), Operand::AddrRegIndirectDisp(ar, d, _)) => (*dr, *ar, *d, true),
        (Operand::AddrRegIndirectDisp(ar, d, _), Operand::DataReg(dr)) => (*dr, *ar, *d, false),
        _ => return Err(AsmError::new("MOVEP requires Dn,(An,disp) or (An,disp),Dn")),
    };
    let op_mode = if to_mem {
        if sz == 1 { 7 } else { 6 }
    } else if sz == 1 {
        5
    } else {
        4
    };
    let op = 0x0008 | ((data_reg as u16) << 9) | ((op_mode as u16) << 6) | (addr_reg as u16);
    Ok(vec![op, (disp as u16)])
}

/// Encode MOVES instruction (68010+): register to/from alternate address space.
pub fn enc_moves(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" {
        return Err(AsmError::new("MOVES requires 68010 or later"));
    }
    let size_code = match size {
        "b" => 0u16,
        "w" => 1,
        "l" => 3,
        _ => return Err(AsmError::new("invalid size for MOVES")),
    };
    let (reg_word, ea_ast) = if let Operand::DataReg(r) = src {
        (((*r as u16) << 12), dst)
    } else if let Operand::AddrReg(r) = src {
        ((1u16 << 15) | ((*r as u16) << 12), dst)
    } else if let Operand::DataReg(r) = dst {
        (((*r as u16) << 12), src)
    } else if let Operand::AddrReg(r) = dst {
        ((1u16 << 15) | ((*r as u16) << 12), src)
    } else {
        return Err(AsmError::new(
            "MOVES requires one register and one EA operand",
        ));
    };
    let ea_allowed = DREG | AREG_IND | APOSTINC | APREDEC | AINDEXED | ABSW | ABSL;
    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea_ast, size, pc, ea_allowed, cpu)?;
    let base = 0x0E00 | (size_code << 9);
    let op = base | ((ea_mode as u16) << 3) | (ea_reg as u16);
    let mut words = vec![op, reg_word];
    words.extend(ea_ext);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_b_d0_d1() {
        let words = enc_move(&Operand::DataReg(0), &Operand::DataReg(1), "b", 0, "68000").unwrap();
        assert_eq!(words, vec![0x1200]);
    }

    #[test]
    fn test_move_w_d0_d1() {
        let words = enc_move(&Operand::DataReg(0), &Operand::DataReg(1), "w", 0, "68000").unwrap();
        assert_eq!(words, vec![0x3200]);
    }

    #[test]
    fn test_move_l_d0_d1() {
        let words = enc_move(&Operand::DataReg(0), &Operand::DataReg(1), "l", 0, "68000").unwrap();
        assert_eq!(words, vec![0x2200]);
    }

    #[test]
    fn test_move_l_imm_d0() {
        let words = enc_move(
            &Operand::Immediate(0x12345678),
            &Operand::DataReg(0),
            "l",
            0,
            "68000",
        )
        .unwrap();
        assert_eq!(words, vec![0x203C, 0x1234, 0x5678]);
    }

    #[test]
    fn test_moveq_5_d0() {
        let words = enc_moveq(5, 0).unwrap();
        assert_eq!(words, vec![0x7005]);
    }

    #[test]
    fn test_moveq_neg1_d0() {
        let words = enc_moveq(-1, 0).unwrap();
        assert_eq!(words, vec![0x70FF]);
    }

    #[test]
    fn test_movep_dn_to_mem_word() {
        let words = enc_movep(
            &Operand::DataReg(0),
            &Operand::AddrRegIndirectDisp(1, 0x1000, false),
            "w",
        )
        .unwrap();
        assert_eq!(words, vec![0x0108 | (6 << 6) | 1, 0x1000]); // Dn = D0 (bit 9 = 0)
    }

    #[test]
    fn test_movep_mem_to_dn_long() {
        let words = enc_movep(
            &Operand::AddrRegIndirectDisp(2, 0x2000, false),
            &Operand::DataReg(3),
            "l",
        )
        .unwrap();
        assert_eq!(words, vec![0x0108 | (3 << 9) | (5 << 6) | 2, 0x2000]);
    }

    #[test]
    fn test_moves_dn_to_mem() {
        let words = enc_moves(
            &Operand::DataReg(0),
            &Operand::AddrRegIndirect(1),
            "l",
            0,
            "68010",
        )
        .unwrap();
        // base 0x0E00 | (3<<9) = 0x0E00 | 0x600 = 0x0E00
        // Wait: 3 << 9 = 0x600. 0x0E00 | 0x600 = 0x0E00 | 0x0600 = 0x0C00? Let me compute:
        // 0x0E00 = 0b0000_1110_0000_0000
        // 0x0600 = 0b0000_0110_0000_0000
        // OR = 0x0E00 | 0x0600 = 0x0E00 (bit 9 is already set in 0x0E00... no, let me check)
        // 0x0E00 bit 9 = 0, bit 10 = 1, bit 11 = 1
        // 0x0600 bit 9 = 1, bit 10 = 1
        // OR: bit 9 = 1, bit 10 = 1 = 0x0C00... hmm
        // Actually, 3 << 9 = 0x1800... NO. 3 = 0b11. 3 << 9 = 0b11000000000 = 0x600. Yes, 0x600.
        // 0x0E00 = 0b0000_1110_0000_0000
        // 0x0600 = 0b0000_0110_0000_0000
        // OR     = 0b0000_1110_0000_0000 = 0x0E00 (unchanged, since bit 10 already set)
        // | (2<<3) | 1 = 0x0E00 | 0x10 | 1 = 0x0E11
        // ext: (0 << 12) = 0
        assert_eq!(words, vec![0x0E11, 0x0000]);
    }

    #[test]
    fn test_moves_mem_to_areg() {
        let words = enc_moves(
            &Operand::AddrRegIndirect(1),
            &Operand::AddrReg(2),
            "l",
            0,
            "68010",
        )
        .unwrap();
        // Dn→mem: reg_word = (1<<15) | (2<<12) = 0x8000 | 0x2000 = 0xA000
        // Actually: (1 << 15) = 0x8000, (2 << 12) = 0x2000, OR = 0xA000
        // op = 0x0E00 | (3<<9) | (2<<3) | 1 = 0x0E00 | 0x600 | 0x10 | 1 = 0x0E11
        // Wait: AREN_IND mode=2, reg=1. So (ea_mode<<3) | ea_reg = (2<<3) | 1 = 0x10 | 1 = 0x11
        assert_eq!(words, vec![0x0E11, 0xA000]);
    }

    #[test]
    fn test_moves_68000_fails() {
        assert!(
            enc_moves(
                &Operand::DataReg(0),
                &Operand::AddrRegIndirect(1),
                "l",
                0,
                "68000",
            )
            .is_err()
        );
    }
}
