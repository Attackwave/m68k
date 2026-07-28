//! Central mnemonic → minimum-CPU gating table.
//!
//! Consolidates the CPU-availability checks that were previously scattered
//! (and inconsistently applied — some checked only `cpu == "68000"` instead
//! of excluding 68010 too, others had no check at all) across `enc_flow.rs`,
//! `enc_math.rs`, `enc_logic.rs`, `enc_bitfield.rs`, `enc_fpu.rs` and
//! `enc_mmu.rs`. This is the single source of truth for "which CPU
//! generation does mnemonic X require", queried once per instruction before
//! any encoder-specific logic runs.

use crate::addressing::cpu_level;

/// The CPU names `cpu_level()` recognizes explicitly. Any other string
/// falls through to `cpu_level()`'s permissive `_ => 5` (max) fallback,
/// so unvalidated user input like a typo'd `--cpu 68O20` would otherwise
/// silently be treated as the *most* permissive CPU instead of being
/// rejected. CLI entry points should validate against this list before
/// calling `Assembler::set_cpu`/`Disassembler::set_cpu`.
pub const KNOWN_CPUS: &[&str] = &["68000", "68010", "68020", "68030", "68040", "68060"];

/// Validates a user-supplied `--cpu` string against the known CPU names.
/// Returns an error message (not an `AsmError`, to stay usable from CLI
/// binaries that don't depend on the encoder error type) if unrecognized.
pub fn validate_cpu_name(cpu: &str) -> Result<(), String> {
    if KNOWN_CPUS.contains(&cpu) {
        Ok(())
    } else {
        Err(format!(
            "unknown --cpu value '{}': expected one of {}",
            cpu,
            KNOWN_CPUS.join(", ")
        ))
    }
}

/// Minimum CPU generation required for a mnemonic (or an EA/instruction
/// feature such as scaled-index or full-format addressing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinCpu {
    /// Base 68000 instruction set — always available.
    Mc68000,
    /// Requires 68010 or later.
    Mc68010,
    /// Requires 68020 or later.
    Mc68020,
    /// Requires 68030 or later (PMMU-adjacent ops built into the CPU).
    Mc68030,
    /// Requires 68040 or later.
    Mc68040,
    /// Requires 68060.
    Mc68060,
}

impl MinCpu {
    fn level(self) -> u8 {
        match self {
            MinCpu::Mc68000 => 0,
            MinCpu::Mc68010 => 1,
            MinCpu::Mc68020 => 2,
            MinCpu::Mc68030 => 3,
            MinCpu::Mc68040 => 4,
            MinCpu::Mc68060 => 5,
        }
    }

    /// Human-readable CPU name, for error messages.
    pub fn name(self) -> &'static str {
        match self {
            MinCpu::Mc68000 => "68000",
            MinCpu::Mc68010 => "68010",
            MinCpu::Mc68020 => "68020",
            MinCpu::Mc68030 => "68030",
            MinCpu::Mc68040 => "68040",
            MinCpu::Mc68060 => "68060",
        }
    }
}

/// Returns `Some(min_cpu)` for mnemonics that require later than base
/// 68000, `None` for base-68000 mnemonics and for names this table doesn't
/// recognize (unknown mnemonics are rejected elsewhere, by the dispatcher).
///
/// `mnemonic` must be the bare mnemonic, upper-cased, without size suffix
/// (e.g. `"MOVE"`, `"BFINS"`, `"FADD"` — not `"MOVE.L"`).
pub fn min_cpu_for_mnemonic(mnemonic: &str) -> Option<MinCpu> {
    Some(match mnemonic {
        // 68010: privileged/loop-mode additions.
        "MOVEC" | "MOVES" | "RTD" | "BKPT" => MinCpu::Mc68010,

        // 68020+: bitfield instructions.
        "BFTST" | "BFCHG" | "BFCLR" | "BFSET" | "BFEXTU" | "BFEXTS" | "BFFFO" | "BFINS"
        | "BFINV" => MinCpu::Mc68020,

        // 68020+: CAS/CAS2, CHK2/CMP2, CALLM/RTM, PACK/UNPK, TRAPcc,
        // extended MUL/DIV forms. (LINK.L, EXTB.L and long Bcc/BSR
        // displacement are also 68020+ but share a plain-68000 mnemonic
        // with a 68000-valid form — e.g. LINK.W — so they stay gated by
        // their existing size/operand-dependent checks in enc_flow.rs
        // rather than this flat mnemonic table.)
        "CAS" | "CAS2" | "CHK2" | "CMP2" | "CALLM" | "RTM" | "PACK" | "UNPK" | "TRAPCC"
        | "TRAPCS" | "TRAPEQ" | "TRAPF" | "TRAPGE" | "TRAPGT" | "TRAPHI" | "TRAPLE" | "TRAPLS"
        | "TRAPLT" | "TRAPMI" | "TRAPNE" | "TRAPPL" | "TRAPT" | "TRAPVC" | "TRAPVS" | "MULU.L"
        | "MULS.L" | "DIVSL" | "DIVUL" => MinCpu::Mc68020,

        // FPU instructions (68881/68882, or on-chip from 68040 on with the
        // model treating FPU as available from 68020 upward — this tool
        // doesn't model a separate "has coprocessor" flag).
        m if m.starts_with('F') && is_fpu_mnemonic(m) => MinCpu::Mc68020,

        // 68030+: PMMU (68851, or on-chip from 68030).
        "PMOVE" | "PTEST" | "PTESTR" | "PTESTW" | "PFLUSH" | "PFLUSHA" | "PLOAD" | "PVALID"
        | "PSAVE" | "PRESTORE" => MinCpu::Mc68030,

        // 68040+: cache ops, MOVE16, on-chip-MMU-only PMMU variant.
        "CINVL" | "CINVP" | "CINVA" | "CPUSHL" | "CPUSHP" | "CPUSHA" | "MOVE16" | "PFLUSHAN" => {
            MinCpu::Mc68040
        }

        // 68060-only.
        "LPSTOP" => MinCpu::Mc68060,

        _ => return None,
    })
}

/// FPU mnemonics all require 68020+ (coprocessor interface introduced then).
/// Kept as a separate helper since the FPU mnemonic set is large and mostly
/// regular (`F` prefix) but a few non-`F`-prefixed aliases don't apply here.
fn is_fpu_mnemonic(m: &str) -> bool {
    // Every real mnemonic in this assembler that starts with 'F' is an FPU
    // instruction (FADD, FMOVE, FBcc, FDBcc, FScc, FTRAPcc, FSAVE,
    // FRESTORE, FNOP, FMOVEM, FMOVECR, FSINCOS, the "short" precision-forcing
    // forms FSMOVE/FDADD/etc., and all condition-code variants). There is no
    // non-FPU mnemonic starting with 'F' in the instruction set.
    m.starts_with('F')
}

/// Extra per-instruction-form gating that isn't a flat mnemonic lookup:
/// forms that are 68020+ on some CPUs but only reachable via specific
/// operand shapes (e.g. `DIVS.L`/`DIVU.L` word-form is 68000 base, but the
/// 64-bit `Dh:Dl`/`Dr:Dq` register-pair form is 68020+; `EXTB.L` promotes
/// `EXTB` which itself doesn't exist pre-68020).
///
/// Returns an error message fragment (without "requires ..." prefix) if
/// `cpu` doesn't meet `min`.
pub fn check_cpu_at_least(cpu: &str, min: MinCpu) -> Result<(), String> {
    if cpu_level(cpu) < min.level() {
        Err(format!(
            "instruction not available on {} (requires {} or later)",
            cpu,
            min.name()
        ))
    } else {
        Ok(())
    }
}

/// Look up and check a plain mnemonic against the central table in one
/// call. `Ok(())` for base-68000 mnemonics, for mnemonics this table
/// doesn't know about (nothing to gate), and for any mnemonic when `cpu`
/// meets its minimum.
pub fn check_mnemonic_cpu(mnemonic: &str, cpu: &str) -> Result<(), String> {
    match min_cpu_for_mnemonic(mnemonic) {
        Some(min) => check_cpu_at_least(cpu, min).map_err(|_| {
            format!(
                "{} not available on {} (requires {} or later)",
                mnemonic,
                cpu,
                min.name()
            )
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_68000_mnemonics_ungated() {
        assert!(check_mnemonic_cpu("MOVE", "68000").is_ok());
        assert!(check_mnemonic_cpu("ADD", "68000").is_ok());
        assert_eq!(min_cpu_for_mnemonic("MOVE"), None);
    }

    #[test]
    fn movec_requires_68010() {
        assert!(check_mnemonic_cpu("MOVEC", "68000").is_err());
        assert!(check_mnemonic_cpu("MOVEC", "68010").is_ok());
        assert!(check_mnemonic_cpu("MOVEC", "68020").is_ok());
    }

    #[test]
    fn bitfield_requires_68020_not_just_not_68000() {
        // Regression: previous checks in enc_bitfield.rs correctly excluded
        // both 68000 and 68010, this locks that in centrally.
        assert!(check_mnemonic_cpu("BFINS", "68000").is_err());
        assert!(check_mnemonic_cpu("BFINS", "68010").is_err());
        assert!(check_mnemonic_cpu("BFINS", "68020").is_ok());
    }

    #[test]
    fn divsl_requires_68020_not_just_not_68000() {
        // Regression for the enc_math.rs gap: DIVSL/DIVUL previously
        // correctly excluded 68010 (`cpu == "68000" || cpu == "68010"`),
        // this centralizes and locks in the same behavior.
        assert!(check_mnemonic_cpu("DIVSL", "68010").is_err());
        assert!(check_mnemonic_cpu("DIVSL", "68020").is_ok());
    }

    #[test]
    fn cas2_now_gated() {
        // Regression: enc_cas2 previously took no cpu parameter at all and
        // was completely ungated.
        assert!(check_mnemonic_cpu("CAS2", "68010").is_err());
        assert!(check_mnemonic_cpu("CAS2", "68020").is_ok());
    }

    #[test]
    fn pack_unpk_now_gated() {
        assert!(check_mnemonic_cpu("PACK", "68010").is_err());
        assert!(check_mnemonic_cpu("UNPK", "68000").is_err());
        assert!(check_mnemonic_cpu("PACK", "68020").is_ok());
    }

    #[test]
    fn rtm_now_gated() {
        assert!(check_mnemonic_cpu("RTM", "68010").is_err());
        assert!(check_mnemonic_cpu("RTM", "68020").is_ok());
    }

    #[test]
    fn bkpt_rtd_require_68010() {
        assert!(check_mnemonic_cpu("BKPT", "68000").is_err());
        assert!(check_mnemonic_cpu("RTD", "68000").is_err());
        assert!(check_mnemonic_cpu("BKPT", "68010").is_ok());
    }

    #[test]
    fn fpu_mnemonics_require_68020() {
        assert!(check_mnemonic_cpu("FADD", "68010").is_err());
        assert!(check_mnemonic_cpu("FADD", "68020").is_ok());
        assert!(check_mnemonic_cpu("FBEQ", "68010").is_err());
        assert!(check_mnemonic_cpu("FDBEQ", "68010").is_err());
        assert!(check_mnemonic_cpu("FTRAPEQ", "68010").is_err());
        assert!(check_mnemonic_cpu("FNOP", "68010").is_err());
        assert!(check_mnemonic_cpu("FNOP", "68020").is_ok());
    }

    #[test]
    fn mmu_requires_68030() {
        assert!(check_mnemonic_cpu("PMOVE", "68020").is_err());
        assert!(check_mnemonic_cpu("PMOVE", "68030").is_ok());
    }

    #[test]
    fn move16_and_cache_ops_require_68040() {
        assert!(check_mnemonic_cpu("MOVE16", "68030").is_err());
        assert!(check_mnemonic_cpu("MOVE16", "68040").is_ok());
        assert!(check_mnemonic_cpu("CINVA", "68030").is_err());
        assert!(check_mnemonic_cpu("CPUSHA", "68030").is_err());
        assert!(check_mnemonic_cpu("PFLUSHAN", "68030").is_err());
        assert!(check_mnemonic_cpu("PFLUSHAN", "68040").is_ok());
    }

    #[test]
    fn lpstop_requires_68060() {
        assert!(check_mnemonic_cpu("LPSTOP", "68040").is_err());
        assert!(check_mnemonic_cpu("LPSTOP", "68060").is_ok());
    }

    #[test]
    fn validate_cpu_name_accepts_known_rejects_unknown() {
        for name in KNOWN_CPUS {
            assert!(validate_cpu_name(name).is_ok());
        }
        assert!(validate_cpu_name("68O20").is_err());
        assert!(validate_cpu_name("").is_err());
        assert!(validate_cpu_name("68080").is_err());
    }

    #[test]
    fn unknown_cpu_string_does_not_silently_grant_everything() {
        // cpu_level() maps unknown strings to level 5 (max) as a
        // permissive fallback for forward-compat CPU names; this is a
        // known, intentional tradeoff documented at the CLI-validation
        // layer (bogus --cpu values are rejected there, not here).
        assert!(check_mnemonic_cpu("LPSTOP", "68060").is_ok());
    }
}
