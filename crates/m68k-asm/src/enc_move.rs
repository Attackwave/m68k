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
    // The destination is data-alterable, plus An for the MOVEA form
    // (`MOVE.W D0,A1`, which the dispatcher routes here rather than to
    // `enc_movea`). `ALL` let two categories of invalid instruction
    // through silently: PC-relative destinations (`MOVE.W D0,LAB(PC)`)
    // and immediate ones. PC-relative modes are not alterable on any
    // 68k — not just pre-68020 — because there is nothing to write back
    // to; the reference assembler rejects them on every architecture.
    // `CLR`/`ADD` already refused these; only MOVE was permissive.
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, DATA_ALT | AREG, cpu)?;

    let op = ((move_size as u16) << 12)
        | ((dst_reg as u16) << 9)
        | ((dst_mode as u16) << 6)
        | ((src_mode as u16) << 3)
        | (src_reg as u16);

    // Extension words follow the opword in operand order: source first,
    // then destination. Emitting them the other way round produced
    // subtly corrupt code whenever *both* operands carry extensions —
    // e.g. `move.b #$ff,$8(a2)` came out as 157C 0008 00FF instead of
    // 157C 00FF 0008, swapping the immediate with the displacement.
    // Found by reassembling real Kickstart 1.3 ROM code and comparing
    // against the original bytes; confirmed with reference encodings for
    // immediate->displacement, absolute->absolute and displacement->
    // displacement combinations.
    let mut words = vec![op];
    words.extend(src_ext);
    words.extend(dst_ext);
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
    // MOVE-family size bits (13-12) are w=3, l=2 — not 1/3. MOVEA has no
    // byte form. The previous `if l {3} else {1}` emitted .l as 3 (i.e.
    // MOVE.W) and .w as 1 (MOVE.B), so every MOVEA came out the wrong
    // size. verified against reference encodings: `movea.l a0,a1` -> 0x2248.
    let sz: u16 = match size {
        "l" => 2,
        "w" => 3,
        _ => return Err(AsmError::new("MOVEA requires .w or .l size")),
    };
    // MOVEA accepts *all* addressing modes as its source, including An
    // (`MOVEA.L A0,A1` is valid and common). The previous `DATA`
    // category excludes address registers, so that form was rejected
    // with "addressing mode not allowed".
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, ALL, cpu)?;

    // Destination mode (bits 8-6) must be 001 = address-register direct;
    // it was previously left at 000 (data register), so the encoding
    // named a Dn destination and decoded back as a plain MOVE.
    let op = (sz << 12)
        | ((dst_reg as u16) << 9)
        | (0b001 << 6)
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

/// Encode MOVEQ from a full-width immediate, rejecting values the 8-bit
/// field cannot hold.
///
/// The caller used to cast with `as i8`, which wraps silently: `MOVEQ
/// #256,D0` assembled as `MOVEQ #0` and `MOVEQ #-129,D0` as `MOVEQ #127`
/// — the loaded value simply became a different one, with no diagnostic.
/// The reference rejects both.
pub fn enc_moveq_checked(data: i64, dst_reg: u8) -> Result<Vec<u16>, AsmError> {
    // Accepted either as a signed byte (`#-1`) or as the 32-bit pattern it
    // loads (`#$FFFFFFFF`) — the reference takes both, and the
    // disassembler prints the latter, so rejecting it would break the
    // roundtrip. What must still fail is a value that simply does not fit,
    // such as `#256` or `#-129`.
    let fits_signed_byte = (-128..=127).contains(&data);
    // An unsigned byte (`#$FF`) or the sign-extended 32-bit pattern it
    // loads (`#$FFFFFFFF`) are both spellings of the same encoding.
    let fits_unsigned_byte = (128..=255).contains(&data);
    let fits_as_u32_pattern = (0..=0xFFFF_FFFF).contains(&data) && {
        let sign_extended = (data as u32) as i32;
        (-128..=127).contains(&sign_extended)
    };
    if !fits_signed_byte && !fits_unsigned_byte && !fits_as_u32_pattern {
        return Err(AsmError::new(format!(
            "MOVEQ operand {} is outside the range -128..127 \
             (use MOVE.L for a full 32-bit immediate)",
            data
        )));
    }
    enc_moveq(data as u32 as i32 as i8, dst_reg)
}

#[cfg(test)]
mod moveq_range_tests {
    use super::*;

    #[test]
    fn moveq_rejects_values_that_do_not_fit() {
        // These used to wrap silently via `as i8`: #256 loaded 0 and #-129
        // loaded 127. The value the program got was simply a different one,
        // with no diagnostic. The reference rejects both.
        assert!(enc_moveq_checked(256, 0).is_err());
        assert!(enc_moveq_checked(-129, 0).is_err());
        assert!(enc_moveq_checked(0x100, 0).is_err());
    }

    #[test]
    fn moveq_accepts_every_spelling_of_a_byte() {
        // Signed, unsigned, and the sign-extended 32-bit pattern are all
        // the same encoding, and the reference takes all three. The
        // disassembler prints the last form, so rejecting it would break
        // the roundtrip.
        for (input, want) in [
            (-1i64, 0x70FFu16),
            (0xFFFF_FFFF, 0x70FF),
            (0xFF, 0x70FF),
            (127, 0x707F),
            (-128, 0x7080),
            (0, 0x7000),
        ] {
            assert_eq!(
                enc_moveq_checked(input, 0).unwrap(),
                vec![want],
                "for operand {}",
                input
            );
        }
    }
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

    let allowed =
        AREG_IND | APOSTINC | APREDEC | AREG_DISP | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
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
    let allowed =
        AREG_IND | APOSTINC | APREDEC | AREG_DISP | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
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
        // `(An)` without a displacement means displacement 0. MOVEP has no
        // plain-indirect encoding, so the operand shape differs from the
        // instruction's addressing mode — the reference accepts
        // `movep.w (a2),d6` and emits the same bytes as `0(a2)`.
        (Operand::DataReg(dr), Operand::AddrRegIndirect(ar)) => (*dr, *ar, 0, true),
        (Operand::AddrRegIndirect(ar), Operand::DataReg(dr)) => (*dr, *ar, 0, false),
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
    // Size occupies opword bits 7-6 (b=0, w=1, l=2), like most
    // single-EA instructions — *not* bits 10-9. The previous code both
    // used the wrong field width (0/1/3, a bits-10-9 style encoding) and
    // OR'd it at bit 9, where the 0x0E00 base already has bits 11-9 set,
    // so the shift had no effect at all and every MOVES came out
    // byte-sized. verified against reference encodings: `moves.w a1,(a0)` is
    // 0x0E50, `moves.l d0,(a0)` is 0x0E90.
    let size_code = match size {
        "b" => 0u16,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size for MOVES")),
    };
    // Extension word: bit 15 selects An over Dn, bits 14-12 the register
    // number, and bit 11 the direction (1 = register -> memory). That
    // direction bit was previously never set, so `MOVES Rn,<ea>` encoded
    // identically to `MOVES <ea>,Rn` and silently assembled to a load.
    let (reg_word, ea_ast) = if let Operand::DataReg(r) = src {
        ((1u16 << 11) | ((*r as u16) << 12), dst)
    } else if let Operand::AddrReg(r) = src {
        ((1u16 << 15) | (1u16 << 11) | ((*r as u16) << 12), dst)
    } else if let Operand::DataReg(r) = dst {
        (((*r as u16) << 12), src)
    } else if let Operand::AddrReg(r) = dst {
        ((1u16 << 15) | ((*r as u16) << 12), src)
    } else {
        return Err(AsmError::new(
            "MOVES requires one register and one EA operand",
        ));
    };
    let ea_allowed = DREG | AREG_IND | APOSTINC | APREDEC | AREG_DISP | AINDEXED | ABSW | ABSL;
    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea_ast, size, pc, ea_allowed, cpu)?;
    let op = 0x0E00 | (size_code << 6) | ((ea_mode as u16) << 3) | (ea_reg as u16);
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
        // Reference encoding: `moves.l d0,(a1)` -> 0e91 0800.
        // opword: 0x0E00 | (size 2 << 6) | (mode 2 << 3) | reg 1 = 0x0E91
        // ext:    Dn (bit15=0), reg 0 (bits14-12), direction
        //         register->memory (bit11=1) = 0x0800
        // The previous expectation (0x0E11, 0x0000) encoded this as a
        // *byte*-sized *load* — the size shift landed on already-set bits
        // of the 0x0E00 base and the direction bit was never emitted.
        assert_eq!(words, vec![0x0E91, 0x0800]);
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
        // Reference encoding: `moves.l (a1),a2` -> 0e91 a000.
        // opword: same 0x0E91 as the store above (the EA and size are
        // identical); ext: An (bit15=1) | reg 2 (bits14-12) | direction
        // memory->register (bit11=0) = 0xA000.
        assert_eq!(words, vec![0x0E91, 0xA000]);
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
