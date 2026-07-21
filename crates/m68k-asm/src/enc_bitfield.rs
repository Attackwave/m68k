//! Bitfield instruction encoders (68020+): BFTST, BFEXTU, BFEXTS, BFFFO,
//! BFCHG, BFCLR, BFSET, BFINS, and the non-standard BFINV.

use m68k_core::ea_categories::ea;
use m68k_core::errors::AsmError;
use m68k_core::operands::{BitfieldSpec, Operand};

use crate::ea_encode::encode_ea;

/// Subtype field (bits 10-8 of the extension word) per mnemonic.
fn subtype(mnemonic: &str) -> Option<u16> {
    match mnemonic {
        "BFTST" => Some(0),
        "BFEXTU" => Some(1),
        "BFCHG" => Some(2),
        "BFEXTS" => Some(3),
        "BFCLR" => Some(4),
        "BFFFO" => Some(5),
        "BFSET" => Some(6),
        "BFINS" => Some(7),
        _ => None,
    }
}

fn is_read_only(mnemonic: &str) -> bool {
    matches!(mnemonic, "BFTST" | "BFEXTU" | "BFEXTS" | "BFFFO")
}

/// Build the bitfield extension word's offset/width fields.
///
/// Layout (verified against real `vasm -m68020` output for
/// `BFEXTU D1{D2:D3},D4` -> ext word `0x48A3`): bit 11 = offset-is-register
/// flag, bits 10-6 = offset (register number or 5-bit immediate), bit 5 =
/// width-is-register flag, bits 4-0 = width (register number or immediate,
/// 0 meaning 32). Bits 14-12 (the destination register, when present) are
/// set separately by the caller — they don't overlap with this function's
/// bits, unlike the previous (incorrect) bit-15/bit-11 layout.
fn bitfield_ext(offset: &BitfieldSpec, width: &BitfieldSpec) -> u16 {
    let mut ext = 0u16;
    match offset {
        BitfieldSpec::DataReg(n) => {
            ext |= 0x0800;
            ext |= ((*n & 0x7) as u16) << 6;
        }
        BitfieldSpec::Immediate(v) => {
            ext |= (((*v as i32) & 0x1F) as u16) << 6;
        }
    }
    match width {
        BitfieldSpec::DataReg(n) => {
            ext |= 0x0020;
            ext |= (*n & 0x7) as u16;
        }
        BitfieldSpec::Immediate(v) => {
            let w = ((*v as i32) % 32) & 0x1F;
            ext |= w as u16;
        }
    }
    ext
}

/// Encode BFTST/BFEXTU/BFEXTS/BFFFO/BFCHG/BFCLR/BFSET/BFINS.
///
/// `reg` is the Dn operand: absent for BFTST/BFCHG/BFCLR/BFSET/BFFFO's implicit
/// destination-less forms, the *source* Dn for BFEXTU/BFEXTS/BFFFO, and the
/// *destination* Dn for BFINS.
pub fn enc_bitfield(
    mnemonic: &str,
    bitfield: &Operand,
    reg: Option<u8>,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new(format!(
            "{} requires 68020 or later",
            mnemonic
        )));
    }

    let Operand::Bitfield(ea_op, offset, width) = bitfield else {
        return Err(AsmError::new(format!(
            "{} requires bitfield operand: ea{{offset:width}}",
            mnemonic
        )));
    };

    let sub = subtype(mnemonic)
        .ok_or_else(|| AsmError::new(format!("unknown bitfield mnemonic: {}", mnemonic)))?;

    let allowed = if is_read_only(mnemonic) {
        ea::DREG | ea::CONTROL
    } else {
        ea::DREG | ea::CONTROL_ALT
    };

    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea_op, "b", ext_pc, allowed, cpu)?;

    let mut ext = bitfield_ext(offset, width);
    if let Some(rn) = reg {
        ext |= ((rn & 0x7) as u16) << 12;
    }

    let opword = 0xE000
        | (1 << 11)
        | (1 << 7)
        | (1 << 6)
        | (sub << 8)
        | ((ea_mode as u16) << 3)
        | (ea_reg as u16);

    let mut words = vec![opword, ext];
    words.extend(ea_ext);
    Ok(words)
}

/// Encode BFINV (non-standard bitfield invert): same shape as BFSET/BFCLR
/// but sets the invert flag (extension word bit 5) and uses alterable-memory-only EA.
pub fn enc_bfinv(bitfield: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new("BFINV requires 68020 or later"));
    }

    let Operand::Bitfield(ea_op, offset, width) = bitfield else {
        return Err(AsmError::new(
            "BFINV requires bitfield operand: ea{offset:width}",
        ));
    };

    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea_op, "b", ext_pc, ea::CONTROL_ALT, cpu)?;

    let mut ext = bitfield_ext(offset, width);
    ext |= 1 << 5;

    let opword = 0xE000
        | (1 << 11)
        | (1 << 7)
        | (1 << 6)
        | (7 << 8)
        | ((ea_mode as u16) << 3)
        | (ea_reg as u16);

    let mut words = vec![opword, ext];
    words.extend(ea_ext);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bftst_imm_offset_width() {
        // BFTST D0{4:8}
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(0)),
            Box::new(BitfieldSpec::Immediate(4)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = enc_bitfield("BFTST", &bf, None, 0, "68020").unwrap();
        // opword = 0xE000|0x800|0x80|0x40|(0<<8)|(0<<3)|0 = 0xE8C0
        assert_eq!(words[0], 0xE8C0);
        // ext = offset(4)<<6 | width(8)
        assert_eq!(words[1], (4 << 6) | 8);
    }

    #[test]
    fn test_bfextu_dn_offset_width_with_dest_reg() {
        // BFEXTU D1{D2:D3},D4 -- verified against real `vasm -m68020`
        // output: opword 0xE9C1, ext word 0x48A3.
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(1)),
            Box::new(BitfieldSpec::DataReg(2)),
            Box::new(BitfieldSpec::DataReg(3)),
        );
        let words = enc_bitfield("BFEXTU", &bf, Some(4), 0, "68020").unwrap();
        assert_eq!(words[0], 0xE9C1);
        assert_eq!(words[1], 0x48A3);
    }

    #[test]
    fn test_bfins_dest_reg_uses_bits_14_12() {
        // BFINS D0,D1{4:8} -- verified against real `vasm -m68020` output:
        // opword 0xEFC1, ext word 0x0108. Destination register always
        // encodes at bits 14-12 of the extension word, regardless of
        // mnemonic (D0 here happens to be 0, so this also checks offset(4)
        // and width(8) land correctly without a dest-register bit set).
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(1)),
            Box::new(BitfieldSpec::Immediate(4)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = enc_bitfield("BFINS", &bf, Some(0), 0, "68020").unwrap();
        assert_eq!(words[0], 0xEFC1);
        assert_eq!(words[1], 0x0108);
    }

    #[test]
    fn test_bitfield_width_32_encodes_as_zero() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(32)),
        );
        let words = enc_bitfield("BFTST", &bf, None, 0, "68020").unwrap();
        assert_eq!(words[1] & 0x1F, 0);
    }

    #[test]
    fn test_bfinv_sets_invert_bit() {
        // BFINV requires an alterable-memory EA (Dn is not allowed).
        let bf = Operand::Bitfield(
            Box::new(Operand::AddrRegIndirect(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = enc_bfinv(&bf, 0, "68020").unwrap();
        assert_eq!(words[0], 0xE000 | 0x800 | 0x80 | 0x40 | (7 << 8) | (2 << 3));
        assert_eq!(words[1] & (1 << 5), 1 << 5);
    }

    #[test]
    fn test_bfinv_rejects_dn_ea() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        assert!(enc_bfinv(&bf, 0, "68020").is_err());
    }

    #[test]
    fn test_bftst_rejects_areg_ea() {
        let bf = Operand::Bitfield(
            Box::new(Operand::AddrReg(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        assert!(enc_bitfield("BFTST", &bf, None, 0, "68020").is_err());
    }

    #[test]
    fn test_non_bitfield_operand_errors() {
        let not_bf = Operand::DataReg(0);
        assert!(enc_bitfield("BFTST", &not_bf, None, 0, "68020").is_err());
    }
}
