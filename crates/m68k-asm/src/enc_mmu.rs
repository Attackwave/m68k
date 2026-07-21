//! MMU, cache, and misc 68030/68040/68060 privileged-instruction encoders.

use m68k_core::ea_categories::ea::*;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

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

fn check_cpu(cpu: &str, min: &str) -> Result<(), AsmError> {
    if cpu_level(cpu) < cpu_level(min) {
        Err(AsmError::new(format!("requires {} or later", min)))
    } else {
        Ok(())
    }
}

/// MMU register code for PMOVE, by control register name.
fn pmove_reg_code(name: &str) -> Option<u16> {
    Some(match name.to_uppercase().as_str() {
        "TC" => 0,
        "TT0" => 2,
        "TT1" => 3,
        "SRP" => 4,
        "CRP" => 5,
        "MMUSR" => 6,
        _ => return None,
    })
}

/// Resolve an operand parsed by `parse_operand_text` into a PMOVE MMU register code.
/// Accepts `Operand::Special(name)` (TT0/TT1/CRP, which don't exist in the MOVEC namespace)
/// and `Operand::Immediate(movec_code)` (TC/SRP/MMUSR, ambiguous with MOVEC control registers
/// of the same name, so they arrive pre-resolved to a MOVEC code that must be mapped back).
fn pmove_reg_code_from_operand(op: &Operand) -> Option<u16> {
    match op {
        Operand::Special(name) => pmove_reg_code(name),
        Operand::Immediate(0x003) => Some(0), // TC
        Operand::Immediate(0x807) => Some(4), // SRP
        Operand::Immediate(0x805) => Some(6), // MMUSR
        _ => None,
    }
}

/// Encode `PMOVE <ea>,MMUreg` or `PMOVE MMUreg,<ea>` (68030+).
pub fn enc_pmove(
    ea_op: &Operand,
    reg_op: &Operand,
    direction_ea_to_reg: bool,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let mmu_code = pmove_reg_code_from_operand(reg_op)
        .ok_or_else(|| AsmError::new("unknown MMU register for PMOVE"))?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "l", ext_pc, CONTROL, cpu)?;
    let direction: u16 = if direction_ea_to_reg { 0 } else { 1 };
    let ext = (direction << 14) | (mmu_code << 10);
    let mut words = vec![0xF000 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `PTEST #level,<ea>` (68030+).
pub fn enc_ptest(
    level: i64,
    ea_op: &Operand,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "l", ext_pc, CONTROL, cpu)?;
    let ext = ((level as u16) & 0x7) << 10;
    let mut words = vec![0xF040 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `PFLUSH #fc,#mask,<ea>` (68030).
pub fn enc_pflush(
    fc: i64,
    mask: i64,
    ea_op: &Operand,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "l", ext_pc, CONTROL, cpu)?;
    let ext = (((mask as u16) & 0x7) << 5) | ((fc as u16) & 0x7);
    let mut words = vec![0xF010 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode PFLUSHA (68030).
pub fn enc_pflusha(cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    Ok(vec![0xF010])
}

/// Encode PFLUSHAN (68040).
pub fn enc_pflushan(cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    Ok(vec![0xF018])
}

/// Encode the single-An/(An)-operand MMU instructions: PTESTW, PTESTR, PFLUSHN, PLPAW, PLPAR.
///
/// Bugfix vs. the Python reference: PTESTW/PTESTR are single-operand instructions there too
/// (base opcodes 0xF548/0xF568), but a copy-paste bug in the base_map routes them through the
/// two-operand `_encode_ptest` encoder, making `ptestw (a0)` fail with a wrong-arity error.
pub fn enc_mmu_single_reg(base: u16, reg_op: &Operand, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let reg = match reg_op {
        Operand::AddrReg(n) => *n,
        Operand::AddrRegIndirect(n) => *n,
        _ => return Err(AsmError::new("operand must be An or (An)")),
    };
    Ok(vec![base | (reg as u16)])
}

/// Encode LPSTOP #data (68060).
pub fn enc_lpstop(val: i64, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68060")?;
    Ok(vec![0xF800, 0x01C0, (val as u16)])
}

/// Encode a cache line operation (CINVL/CINVP/CPUSHL/CPUSHP): 68040 only, opcode is fully
/// static (no register/level fields survive in the reference encoding for these forms).
pub fn enc_cache_line_op(family: u16, op_type: u16, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    let opword = 0xF400 | (family << 5) | (op_type << 4);
    Ok(vec![opword])
}

/// Encode CINVA (68040): invalidate all cache lines in the given cache.
pub fn enc_cinva(an: &Operand, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    match an {
        Operand::AddrReg(n) => Ok(vec![0xF420 | (*n as u16)]),
        _ => Err(AsmError::new("cinva operand must be An")),
    }
}

/// Encode CPUSHA (68040): push all cache lines in the given cache.
pub fn enc_cpusha(an: &Operand, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    match an {
        Operand::AddrReg(n) => Ok(vec![0xF4A0 | (*n as u16)]),
        _ => Err(AsmError::new("cpusha operand must be An")),
    }
}

/// Encode CPUSH #level,An (68040).
pub fn enc_cpush(level: i64, an: &Operand, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    match an {
        Operand::AddrReg(n) => Ok(vec![0xF4A8 | (((level as u16) & 0x3) << 3) | (*n as u16)]),
        _ => Err(AsmError::new("cpush second operand must be An")),
    }
}

/// Encode CINV #level,An (68040).
pub fn enc_cinv(level: i64, an: &Operand, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    match an {
        Operand::AddrReg(n) => Ok(vec![0xF428 | (((level as u16) & 0x3) << 3) | (*n as u16)]),
        _ => Err(AsmError::new("cinv second operand must be An")),
    }
}

/// Encode `PSAVE <ea>` (68030): save MMU internal state.
pub fn enc_psave(ea_op: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "b", ext_pc, ALL, cpu)?;
    let mut words = vec![0xF080 | ((mode as u16) << 3) | (reg as u16)];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `PRESTORE <ea>` (68030): restore MMU internal state.
pub fn enc_prestore(ea_op: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "b", ext_pc, ALL, cpu)?;
    let mut words = vec![0xF0C0 | ((mode as u16) << 3) | (reg as u16)];
    words.extend(ext_words);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference bytes (source: Python assembler unless noted otherwise):
    //   pmove tc,(a0)     -> f0104000
    //   pmove (a0),tc     -> f0100000
    //   pmove srp,(a0)    -> f0105000
    //   ptest #3,(a0)     -> f0500c00
    //   pflush #2,#3,(a0) -> f0100062
    //   pflusha           -> f010
    //   pflushan          -> f018
    //   pflushn (a0)      -> f518
    //   plpaw (a0)        -> f588
    //   plpar (a0)        -> f5c8
    //   lpstop #$2700     -> f80001c02700
    //   cinvl #1,a0       -> f420
    //   cinvp #1,a0       -> f430
    //   cpushl #3,a0      -> f460
    //   cpushp #3,a0      -> f470
    //   cinva a0          -> f420
    //   cpusha a0         -> f4a0
    //   cpush #2,a0       -> f4b8
    //   cinv #2,a0        -> f438
    //   psave -(a0)       -> f0a0
    //   prestore (a0)+    -> f0d8

    #[test]
    fn test_pmove_reg_to_ea_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let tc = Operand::Immediate(0x003);
        let words = enc_pmove(&dst, &tc, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x4000]);
    }

    #[test]
    fn test_pmove_ea_to_reg_matches_python() {
        let src = Operand::AddrRegIndirect(0);
        let tc = Operand::Immediate(0x003);
        let words = enc_pmove(&src, &tc, true, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x0000]);
    }

    #[test]
    fn test_pmove_srp_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let srp = Operand::Immediate(0x807);
        let words = enc_pmove(&dst, &srp, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x5000]);
    }

    #[test]
    fn test_pmove_tt0_via_special_operand() {
        // TT0 doesn't exist in the MOVEC namespace, so it arrives as Operand::Special.
        // mmu_code=2, direction=1 (reg->ea) -> ext = (1<<14)|(2<<10) = 0x4800
        let dst = Operand::AddrRegIndirect(0);
        let tt0 = Operand::Special("TT0".to_string());
        let words = enc_pmove(&dst, &tt0, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x4800]);
    }

    #[test]
    fn test_ptest_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_ptest(3, &dst, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF050, 0x0C00]);
    }

    #[test]
    fn test_pflush_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_pflush(2, 3, &dst, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x0062]);
    }

    #[test]
    fn test_pflusha_matches_python() {
        assert_eq!(enc_pflusha("68030").unwrap(), vec![0xF010]);
    }

    #[test]
    fn test_pflushan_matches_python() {
        assert_eq!(enc_pflushan("68040").unwrap(), vec![0xF018]);
    }

    #[test]
    fn test_pflushn_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF518, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF518]);
    }

    #[test]
    fn test_plpaw_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF588, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF588]);
    }

    #[test]
    fn test_plpar_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF5C8, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF5C8]);
    }

    #[test]
    fn test_ptestw_bugfix_single_operand() {
        // Bugfix vs. Python: `ptestw (a0)` fails there because of a base_map copy-paste bug
        // that routes it through the two-operand _encode_ptest. It's a plain single-reg op.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF548, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF548]);
    }

    #[test]
    fn test_ptestr_bugfix_single_operand() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF568, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF568]);
    }

    #[test]
    fn test_lpstop_matches_python() {
        let words = enc_lpstop(0x2700, "68060").unwrap();
        assert_eq!(words, vec![0xF800, 0x01C0, 0x2700]);
    }

    #[test]
    fn test_cinvl_matches_python() {
        let words = enc_cache_line_op(1, 0, "68040").unwrap();
        assert_eq!(words, vec![0xF420]);
    }

    #[test]
    fn test_cinvp_matches_python() {
        let words = enc_cache_line_op(1, 1, "68040").unwrap();
        assert_eq!(words, vec![0xF430]);
    }

    #[test]
    fn test_cpushl_matches_python() {
        let words = enc_cache_line_op(3, 0, "68040").unwrap();
        assert_eq!(words, vec![0xF460]);
    }

    #[test]
    fn test_cpushp_matches_python() {
        let words = enc_cache_line_op(3, 1, "68040").unwrap();
        assert_eq!(words, vec![0xF470]);
    }

    #[test]
    fn test_cinva_matches_python() {
        let a0 = Operand::AddrReg(0);
        assert_eq!(enc_cinva(&a0, "68040").unwrap(), vec![0xF420]);
    }

    #[test]
    fn test_cpusha_matches_python() {
        let a0 = Operand::AddrReg(0);
        assert_eq!(enc_cpusha(&a0, "68040").unwrap(), vec![0xF4A0]);
    }

    #[test]
    fn test_cpush_matches_python() {
        let a0 = Operand::AddrReg(0);
        assert_eq!(enc_cpush(2, &a0, "68040").unwrap(), vec![0xF4B8]);
    }

    #[test]
    fn test_cinv_matches_python() {
        let a0 = Operand::AddrReg(0);
        assert_eq!(enc_cinv(2, &a0, "68040").unwrap(), vec![0xF438]);
    }

    #[test]
    fn test_psave_matches_python() {
        let dst = Operand::AddrRegPreDec(0);
        assert_eq!(enc_psave(&dst, 0, "68030").unwrap(), vec![0xF0A0]);
    }

    #[test]
    fn test_prestore_matches_python() {
        let src = Operand::AddrRegPostInc(0);
        assert_eq!(enc_prestore(&src, 0, "68030").unwrap(), vec![0xF0D8]);
    }

    #[test]
    fn test_mmu_requires_68030() {
        let a0 = Operand::AddrRegIndirect(0);
        let tc = Operand::Immediate(0x003);
        assert!(enc_pmove(&a0, &tc, false, 0, "68020").is_err());
        assert!(enc_ptest(0, &a0, 0, "68020").is_err());
    }

    #[test]
    fn test_lpstop_requires_68060() {
        assert!(enc_lpstop(0, "68040").is_err());
    }

    #[test]
    fn test_cache_ops_require_68040() {
        let a0 = Operand::AddrReg(0);
        assert!(enc_cinva(&a0, "68030").is_err());
        assert!(enc_cache_line_op(1, 0, "68030").is_err());
    }
}
