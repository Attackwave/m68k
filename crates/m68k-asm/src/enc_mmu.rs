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

/// MMU register code for PMOVE, by control register name: the 6-bit
/// combination of the extension word's 3-bit group prefix (bits 15-13) and
/// 3-bit P-Register field (bits 12-10), as a single `bits15-10` value.
///
/// Verified against the M68000 Family Programmer's Reference Manual
/// (section 6, PMOVE instruction formats): SRP/CRP/TC share prefix `010`;
/// TT0/TT1 share prefix `000`; MMUSR has no P-Register field and uses the
/// fixed prefix `011` (P-Register bits left at 0).
fn pmove_reg_code(name: &str) -> Option<u16> {
    Some(match name.to_uppercase().as_str() {
        "TC" => 0b010_000,
        "TT0" => 0b000_010,
        "TT1" => 0b000_011,
        "SRP" => 0b010_010,
        "CRP" => 0b010_011,
        "MMUSR" => 0b011_000,
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
        Operand::Immediate(0x003) => pmove_reg_code("TC"),
        Operand::Immediate(0x807) => pmove_reg_code("SRP"),
        Operand::Immediate(0x805) => pmove_reg_code("MMUSR"),
        _ => None,
    }
}

/// Encode `PMOVE <ea>,MMUreg` or `PMOVE MMUreg,<ea>` (68030+).
///
/// Extension word layout (verified against the M68000 PRM): bits 15-10 =
/// group prefix + P-Register (see [`pmove_reg_code`]), bit 9 = R/W
/// (0 = memory-to-register, 1 = register-to-memory), bit 8 = FD, bits 7-0
/// unused/zero.
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
    let rw: u16 = if direction_ea_to_reg { 0 } else { 1 };
    let ext = (mmu_code << 10) | (rw << 9);
    let mut words = vec![0xF000 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `PTESTR FC,<ea>,#level[,An]` / `PTESTW FC,<ea>,#level[,An]` (68030+).
///
/// Extension word layout (verified against the M68000 PRM and real
/// `vasm -m68030` output for `ptestr #2,(a0),#3`, `ptestw #2,(a0),#3`, and
/// `ptestr #2,(a0),#3,a1`): bits 15-13 = `100` (fixed group prefix), bits
/// 12-10 = level, bit 9 = R/W (0 = write/PTESTW, 1 = read/PTESTR), bit 8 =
/// A (1 if an address register is given), bits 7-5 = that register (0 if
/// none), bits 4-0 = FC field (`10XXX` for an immediate function code).
///
/// The previous version of this function implemented a different,
/// nonstandard single-operand-adjacent shape (`PTEST #level,<ea>`, opword
/// base `0xF040`) that matched neither this instruction's real syntax nor
/// its real encoding — replaced outright rather than patched.
pub fn enc_ptest(
    fc: i64,
    ea_op: &Operand,
    level: i64,
    an: Option<u8>,
    read: bool,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "l", ext_pc, CONTROL, cpu)?;
    let fc_field = 0b10_000 | ((fc as u16) & 0x7);
    let rw: u16 = if read { 1 } else { 0 };
    let (a_bit, an_field) = match an {
        Some(n) => (1u16, n as u16),
        None => (0u16, 0u16),
    };
    let ext = (0b100 << 13)
        | (((level as u16) & 0x7) << 10)
        | (rw << 9)
        | (a_bit << 8)
        | (an_field << 5)
        | fc_field;
    let mut words = vec![0xF000 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `PFLUSH #fc,#mask,<ea>` (68030): flush by function code and
/// effective address (mode `110`).
///
/// Extension word layout (verified against the M68000 PRM): bits 15-13 =
/// `001` (fixed group prefix), bits 12-10 = mode (`110` here, since an EA is
/// always supplied), bits 9-8 = 0, bits 7-5 = mask, bits 4-0 = FC field. The
/// FC field itself is 5 bits: `10XXX` for an immediate function code (`fc`'s
/// low 3 bits), matching the immediate-FC form of the assembler syntax.
pub fn enc_pflush(
    fc: i64,
    mask: i64,
    ea_op: &Operand,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    let (mode, reg, ext_words) = encode_ea(ea_op, "l", ext_pc, CONTROL, cpu)?;
    let fc_field = 0b10_000 | ((fc as u16) & 0x7);
    let ext = (0b001 << 13) | (0b110 << 10) | (((mask as u16) & 0x7) << 5) | fc_field;
    let mut words = vec![0xF010 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode PFLUSHA (68030): flush all ATC entries (mode `001`, no EA/FC/mask).
///
/// Verified against real `vasm -m68030` output: `F000 2400`. The previous
/// single-word `0xF010` was wrong on two counts — PFLUSHA needs the
/// extension word (mode `001` at bits 12-10, group prefix `001` at bits
/// 15-13), and `0xF010` isn't even a valid opword for it (`0x10` is the EA
/// mode/register field's contribution for a `(An)`-style operand, which
/// PFLUSHA doesn't take).
pub fn enc_pflusha(cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68030")?;
    Ok(vec![0xF000, 0b001_001 << 10])
}

/// Encode PFLUSHAN (68040): flush all except global entries.
///
/// Verified against real `vasm -m68040` output: `F510`. The 68040 PFLUSH
/// family shares base `0xF500` with a 2-bit opmode at bits 4-3 (`00`
/// PFLUSHN, `01` PFLUSH, `10` PFLUSHAN, `11` PFLUSHA) and register at bits
/// 2-0 — the previous `0xF018` didn't match any of these opmodes.
pub fn enc_pflushan(cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_cpu(cpu, "68040")?;
    Ok(vec![0xF500 | (0b10 << 3)])
}

/// Encode the single-An/(An)-operand MMU instructions: PFLUSHN, PLPAW, PLPAR.
///
/// PFLUSHN's base (0xF500, verified against real `vasm -m68040` output) is
/// part of the 68040 PFLUSH opmode family — see `enc_pflushan`'s docs.
/// PLPAW/PLPAR's bases (0xF588/0xF5C8) are unverified against any real
/// assembler or the PRM; treat them with caution.
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

/// Encode a 68040 cache-invalidate/push instruction: `CINVL/P/A` or
/// `CPUSHL/P/A`.
///
/// Layout (verified against real `vasm -m68040` output for all eight
/// scope×unit combinations, e.g. `cinvl dc,(a0)`=F448, `cinvp dc,(a0)`
/// =F450, `cinva dc`=F458, `cpushl dc,(a0)`=F468): opword =
/// `0xF400 | (scope<<6) | (push<<5) | (unit<<3) | reg`, where scope is 2
/// bits (`01`=DC, `10`=IC, `11`=BC, matching the conventional `#1`/`#2`/
/// `#3` cache-select immediate), `push` is 0 for invalidate / 1 for push,
/// `unit` is 2 bits (`01`=Line, `10`=Page, `11`=All), and `reg` (bits 2-0)
/// only applies to the Line/Page forms (0 for the All forms, which take no
/// address register).
///
/// The previous version of this function ignored the cache-select
/// immediate and the address register entirely (CINVL/CINVP/CPUSHL/CPUSHP),
/// and CINVA/CPUSHA used an unrelated fixed opcode (`0xF420|reg`/
/// `0xF4A0|reg`) that didn't match any field in the real instruction —
/// replaced outright rather than patched.
fn enc_cache_op(push: bool, unit: u16, cache: i64, reg: u16, cpu: &str) -> Result<u16, AsmError> {
    check_cpu(cpu, "68040")?;
    let scope = (cache as u16) & 0x3;
    let push_bit: u16 = if push { 1 } else { 0 };
    Ok(0xF400 | (scope << 6) | (push_bit << 5) | (unit << 3) | reg)
}

/// Encode `CINVL/CPUSHL <cache>,(An)`: invalidate/push a single cache line.
pub fn enc_cache_line_op(
    push: bool,
    cache: i64,
    an: &Operand,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let reg = match an {
        Operand::AddrReg(n) => *n as u16,
        Operand::AddrRegIndirect(n) => *n as u16,
        _ => return Err(AsmError::new("cache op requires <cache>,(An)")),
    };
    Ok(vec![enc_cache_op(push, 0b01, cache, reg, cpu)?])
}

/// Encode `CINVP/CPUSHP <cache>,(An)`: invalidate/push a cache page.
pub fn enc_cache_page_op(
    push: bool,
    cache: i64,
    an: &Operand,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let reg = match an {
        Operand::AddrReg(n) => *n as u16,
        Operand::AddrRegIndirect(n) => *n as u16,
        _ => return Err(AsmError::new("cache op requires <cache>,(An)")),
    };
    Ok(vec![enc_cache_op(push, 0b10, cache, reg, cpu)?])
}

/// Encode CINVA (68040): invalidate all cache lines in the given cache.
pub fn enc_cinva(cache: i64, cpu: &str) -> Result<Vec<u16>, AsmError> {
    Ok(vec![enc_cache_op(false, 0b11, cache, 0, cpu)?])
}

/// Encode CPUSHA (68040): push all cache lines in the given cache.
pub fn enc_cpusha(cache: i64, cpu: &str) -> Result<Vec<u16>, AsmError> {
    Ok(vec![enc_cache_op(true, 0b11, cache, 0, cpu)?])
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
    fn test_pmove_reg_to_ea() {
        // PMOVE TC,(A0) -- TC is the source (register-to-memory, R/W=1).
        // Verified against M68000 PRM: TC=0b010000<<10 | R/W(1)<<9 = 0x4200.
        let dst = Operand::AddrRegIndirect(0);
        let tc = Operand::Immediate(0x003);
        let words = enc_pmove(&dst, &tc, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x4200]);
    }

    #[test]
    fn test_pmove_ea_to_reg() {
        // PMOVE (A0),TC -- TC is the destination (memory-to-register, R/W=0).
        // Verified against M68000 PRM: TC=0b010000<<10 | R/W(0)<<9 = 0x4000.
        let src = Operand::AddrRegIndirect(0);
        let tc = Operand::Immediate(0x003);
        let words = enc_pmove(&src, &tc, true, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x4000]);
    }

    #[test]
    fn test_pmove_srp() {
        // PMOVE SRP,(A0) -- verified against M68000 PRM: SRP=0b010010<<10 |
        // R/W(1, register-to-memory)<<9 = 0x4A00.
        let dst = Operand::AddrRegIndirect(0);
        let srp = Operand::Immediate(0x807);
        let words = enc_pmove(&dst, &srp, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x4A00]);
    }

    #[test]
    fn test_pmove_tt0_via_special_operand() {
        // TT0 doesn't exist in the MOVEC namespace, so it arrives as
        // Operand::Special. Verified against M68000 PRM: TT0's group
        // prefix+P-register is 0b000010, R/W=1 (register-to-memory) ->
        // ext = (0b000010<<10)|(1<<9) = 0x0A00.
        let dst = Operand::AddrRegIndirect(0);
        let tt0 = Operand::Special("TT0".to_string());
        let words = enc_pmove(&dst, &tt0, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x0A00]);
    }

    #[test]
    fn test_ptestr_matches_vasm() {
        // PTESTR #2,(A0),#3 -- verified against real `vasm -m68030` output:
        // F010 8E12.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_ptest(2, &dst, 3, None, true, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x8E12]);
    }

    #[test]
    fn test_ptestw_matches_vasm() {
        // PTESTW #2,(A0),#3 -- verified against real `vasm -m68030` output:
        // F010 8C12.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_ptest(2, &dst, 3, None, false, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x8C12]);
    }

    #[test]
    fn test_ptestr_with_an_matches_vasm() {
        // PTESTR #2,(A0),#3,A1 -- verified against real `vasm -m68030`
        // output: F010 8F32.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_ptest(2, &dst, 3, Some(1), true, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x8F32]);
    }

    #[test]
    fn test_pflush_matches_vasm() {
        // PFLUSH #2,#3,(A0) -- verified against real `vasm -m68030`
        // output: F010 3872.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_pflush(2, 3, &dst, 0, "68030").unwrap();
        assert_eq!(words, vec![0xF010, 0x3872]);
    }

    #[test]
    fn test_pflusha_matches_vasm() {
        // PFLUSHA -- verified against real `vasm -m68030` output: F000 2400.
        assert_eq!(enc_pflusha("68030").unwrap(), vec![0xF000, 0x2400]);
    }

    #[test]
    fn test_pflushan_matches_vasm() {
        // PFLUSHAN -- verified against real `vasm -m68040` output: F510.
        assert_eq!(enc_pflushan("68040").unwrap(), vec![0xF510]);
    }

    #[test]
    fn test_pflushn_matches_vasm() {
        // PFLUSHN (A0) -- verified against real `vasm -m68040` output: F500.
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_mmu_single_reg(0xF500, &dst, "68030").unwrap();
        assert_eq!(words, vec![0xF500]);
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
    fn test_lpstop_matches_python() {
        let words = enc_lpstop(0x2700, "68060").unwrap();
        assert_eq!(words, vec![0xF800, 0x01C0, 0x2700]);
    }

    // Cache-line op tests below are all verified against real
    // `vasm -m68040` output for `<cinvl|cinvp|cpushl|cpushp> <bc|ic|dc>,(a0)`
    // and `<cinva|cpusha> <bc|ic|dc>`. Cache scope: #1=DC, #2=IC, #3=BC.

    #[test]
    fn test_cinvl_dc_matches_vasm() {
        let a0 = Operand::AddrRegIndirect(0);
        let words = enc_cache_line_op(false, 1, &a0, "68040").unwrap();
        assert_eq!(words, vec![0xF448]);
    }

    #[test]
    fn test_cinvp_dc_matches_vasm() {
        let a0 = Operand::AddrRegIndirect(0);
        let words = enc_cache_page_op(false, 1, &a0, "68040").unwrap();
        assert_eq!(words, vec![0xF450]);
    }

    #[test]
    fn test_cpushl_dc_matches_vasm() {
        let a0 = Operand::AddrRegIndirect(0);
        let words = enc_cache_line_op(true, 1, &a0, "68040").unwrap();
        assert_eq!(words, vec![0xF468]);
    }

    #[test]
    fn test_cpushp_dc_matches_vasm() {
        let a0 = Operand::AddrRegIndirect(0);
        let words = enc_cache_page_op(true, 1, &a0, "68040").unwrap();
        assert_eq!(words, vec![0xF470]);
    }

    #[test]
    fn test_cinva_bc_matches_vasm() {
        assert_eq!(enc_cinva(3, "68040").unwrap(), vec![0xF4D8]);
    }

    #[test]
    fn test_cpusha_bc_matches_vasm() {
        assert_eq!(enc_cpusha(3, "68040").unwrap(), vec![0xF4F8]);
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
        assert!(enc_ptest(0, &a0, 0, None, true, 0, "68020").is_err());
    }

    #[test]
    fn test_lpstop_requires_68060() {
        assert!(enc_lpstop(0, "68040").is_err());
    }

    #[test]
    fn test_cache_ops_require_68040() {
        let a0 = Operand::AddrRegIndirect(0);
        assert!(enc_cinva(3, "68030").is_err());
        assert!(enc_cache_line_op(false, 1, &a0, "68030").is_err());
    }
}
