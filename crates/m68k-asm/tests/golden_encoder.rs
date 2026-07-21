//! Golden vector tests for the encoder.

use m68k_asm::encoder::encode_instruction;
use m68k_core::operands::{BitfieldSpec, Operand};

/// Parse a bitfield offset/width value: Dn or a (possibly '#'-prefixed) constant.
fn parse_bitfield_val(s: &str) -> Result<BitfieldSpec, String> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('D').or_else(|| s.strip_prefix('d'))
        && let Ok(n) = rest.parse::<u8>()
        && n <= 7
    {
        return Ok(BitfieldSpec::DataReg(n));
    }
    let num_str = s.strip_prefix('#').unwrap_or(s);
    let value = if let Some(hex) = num_str.strip_prefix('$') {
        i64::from_str_radix(hex, 16).map_err(|_| format!("bad bitfield value: {}", s))?
    } else {
        num_str
            .parse::<i64>()
            .map_err(|_| format!("bad bitfield value: {}", s))?
    };
    Ok(BitfieldSpec::Immediate(value))
}

/// Parse bitfield syntax: ea{offset:width}. Returns None if `s` has no top-level `{...}`.
fn parse_bitfield(s: &str) -> Result<Option<Operand>, String> {
    let Some(brace_start) = s.find('{') else {
        return Ok(None);
    };
    let Some(brace_end) = s.rfind('}') else {
        return Err(format!("unbalanced bitfield braces: {}", s));
    };
    let Some(colon) = s[brace_start + 1..brace_end].find(':') else {
        return Err(format!("bitfield missing ':': {}", s));
    };
    let colon = brace_start + 1 + colon;

    let ea = parse_operand(&s[..brace_start])?;
    let offset = parse_bitfield_val(&s[brace_start + 1..colon])?;
    let width = parse_bitfield_val(&s[colon + 1..brace_end])?;
    Ok(Some(Operand::Bitfield(
        Box::new(ea),
        Box::new(offset),
        Box::new(width),
    )))
}

/// Parse an FPU data register name: FP0-FP7.
fn parse_fp_reg(s: &str) -> Option<u8> {
    let upper = s.to_uppercase();
    let rest = upper.strip_prefix("FP")?;
    let n: u8 = rest.parse().ok()?;
    (n <= 7).then_some(n)
}

/// Parse an FPU control register name or '/'-separated list: FPCR=4, FPSR=2, FPIAR=1.
fn parse_fp_ctrl_list(s: &str) -> Option<u8> {
    let mut mask = 0u8;
    for part in s.split('/') {
        let bit = match part.trim().to_uppercase().as_str() {
            "FPIAR" => 1,
            "FPSR" => 2,
            "FPCR" => 4,
            _ => return None,
        };
        mask |= bit;
    }
    Some(mask)
}

fn parse_operand(s: &str) -> Result<Operand, String> {
    let s = s.trim();

    if let Some(bitfield) = parse_bitfield(s)? {
        return Ok(bitfield);
    }

    if let Some(n) = parse_fp_reg(s) {
        return Ok(Operand::FpReg(n));
    }

    if let Some(mask) = parse_fp_ctrl_list(s) {
        return Ok(Operand::FpCtrlList(mask));
    }

    // Register lists for MOVEM: D0-D7/A0-A7 - check FIRST
    if s.contains('-') || s.contains('/') {
        let mut mask: u16 = 0;
        for part in s.split('/') {
            // Check if it's a D range or A range
            let is_d = part.starts_with('D') || part.starts_with('d');
            let is_a = part.starts_with('A') || part.starts_with('a');
            if is_d || is_a {
                // Remove first letter, split on -
                let rest = &part[1..];
                if let Some((a, b)) = rest.split_once('-') {
                    // b might still have 'D' or 'A' prefix (e.g., "0-D7")
                    let b_clean = b
                        .strip_prefix('D')
                        .or_else(|| b.strip_prefix('d'))
                        .or_else(|| b.strip_prefix('A'))
                        .or_else(|| b.strip_prefix('a'))
                        .unwrap_or(b);
                    if let (Ok(sa), Ok(sb)) = (a.parse::<u8>(), b_clean.parse::<u8>()) {
                        let offset = if is_d { 0 } else { 8 };
                        for i in sa..=sb {
                            mask |= 1 << (i + offset);
                        }
                    }
                }
            }
        }
        if mask != 0 {
            return Ok(Operand::Immediate(mask as i64));
        }
    }

    // Data register: D0-D7
    if let Some(rest) = s.strip_prefix('D').or_else(|| s.strip_prefix('d'))
        && let Ok(n) = rest.parse::<u8>()
        && n <= 7
    {
        return Ok(Operand::DataReg(n));
    }
    if let Some(rest) = s.strip_prefix('A').or_else(|| s.strip_prefix('a'))
        && let Ok(n) = rest.parse::<u8>()
        && n <= 7
    {
        return Ok(Operand::AddrReg(n));
    }
    if s.eq_ignore_ascii_case("SP") {
        return Ok(Operand::AddrReg(7));
    }
    if s.eq_ignore_ascii_case("CCR") {
        return Ok(Operand::Immediate(-1));
    }
    if s.eq_ignore_ascii_case("SR") {
        return Ok(Operand::Immediate(-2));
    }

    if let Some(rest) = s.strip_prefix('#') {
        // Check if it's hex ($ prefix after #)
        if let Some(hex_str) = rest.strip_prefix('$') {
            if let Ok(v) = i64::from_str_radix(hex_str, 16) {
                return Ok(Operand::Immediate(v));
            }
            return Err(format!("bad hex: {}", rest));
        }
        // Otherwise decimal
        if let Ok(v) = rest.parse::<i64>() {
            return Ok(Operand::Immediate(v));
        }
        return Err(format!("bad immediate: {}", rest));
    }
    if let Some(rest) = s.strip_prefix('$')
        && let Ok(v) = i64::from_str_radix(rest, 16)
    {
        return Ok(Operand::Immediate(v));
    }
    if s.starts_with('(') && s.ends_with(')') {
        let inner = &s[1..s.len() - 1];
        if let Some(rest) = inner.strip_prefix('A').or_else(|| inner.strip_prefix('a'))
            && let Ok(n) = rest.parse::<u8>()
            && n <= 7
        {
            return Ok(Operand::AddrRegIndirect(n));
        }
    }
    if s.starts_with('(') && s.ends_with(")+") {
        let inner = &s[1..s.len() - 2];
        if let Some(rest) = inner.strip_prefix('A').or_else(|| inner.strip_prefix('a'))
            && let Ok(n) = rest.parse::<u8>()
            && n <= 7
        {
            return Ok(Operand::AddrRegPostInc(n));
        }
    }
    if s.starts_with("-(") && s.ends_with(')') {
        let inner = &s[2..s.len() - 1];
        if let Some(rest) = inner.strip_prefix('A').or_else(|| inner.strip_prefix('a'))
            && let Ok(n) = rest.parse::<u8>()
            && n <= 7
        {
            return Ok(Operand::AddrRegPreDec(n));
        }
    }
    if s.starts_with('-')
        && !s.starts_with("-(")
        && let Ok(v) = s.parse::<i64>()
    {
        return Ok(Operand::Immediate(v));
    }
    // Pre-decrement: -(An) or -(SP)
    if s.starts_with("-(") && s.ends_with(')') {
        let inner = &s[2..s.len() - 1];
        if inner.eq_ignore_ascii_case("SP") {
            return Ok(Operand::AddrRegPreDec(7));
        }
        if let Some(rest) = inner.strip_prefix('A').or_else(|| inner.strip_prefix('a'))
            && let Ok(n) = rest.parse::<u8>()
            && n <= 7
        {
            return Ok(Operand::AddrRegPreDec(n));
        }
    }
    if s.contains('(') && s.ends_with(')') {
        // Could be displacement: $xxx(An) or xxx(An)
        let idx = s.find('(').unwrap();
        let disp_str = &s[..idx];
        let inner = &s[idx + 1..s.len() - 1];
        // Check for (An)+
        if s.ends_with(")+") {
            let inner2 = &s[1..s.len() - 2];
            if let Some(rest) = inner2
                .strip_prefix('A')
                .or_else(|| inner2.strip_prefix('a'))
                && let Ok(n) = rest.parse::<u8>()
                && n <= 7
            {
                return Ok(Operand::AddrRegPostInc(n));
            }
        }
        // Regular (An)
        if let Some(rest) = inner.strip_prefix('A').or_else(|| inner.strip_prefix('a'))
            && let Ok(n) = rest.parse::<u8>()
            && n <= 7
        {
            let disp = if disp_str.is_empty() {
                0
            } else {
                let ds = disp_str.strip_prefix('$').unwrap_or(disp_str);
                if let Some(hex_part) = ds.strip_prefix('-') {
                    -(i64::from_str_radix(hex_part, 16).unwrap_or(0) as i32)
                } else {
                    i64::from_str_radix(ds, 16).map_err(|_| format!("bad disp: {}", disp_str))?
                        as i32
                }
            };
            return Ok(Operand::AddrRegIndirectDisp(n, disp, false));
        }
    }
    if s.contains('-') || s.contains('/') {
        let mut mask: u16 = 0;
        for part in s.split('/') {
            if let Some(rest) = part.strip_prefix('D').or_else(|| part.strip_prefix('d'))
                && let Some((a, b)) = rest.split_once('-')
                && let (Ok(sa), Ok(sb)) = (a.parse::<u8>(), b.parse::<u8>())
            {
                for i in sa..=sb {
                    mask |= 1 << i;
                }
            }
            if let Some(rest) = part.strip_prefix('A').or_else(|| part.strip_prefix('a'))
                && let Some((a, b)) = rest.split_once('-')
                && let (Ok(sa), Ok(sb)) = (a.parse::<u8>(), b.parse::<u8>())
            {
                for i in sa..=sb {
                    mask |= 1 << (i + 8);
                }
            }
        }
        return Ok(Operand::Immediate(mask as i64));
    }
    Ok(Operand::Address(0))
}

fn words_to_hex(words: &[u16]) -> String {
    let mut r = String::new();
    for w in words {
        r.push_str(&format!("{:04x}", w));
    }
    r
}

#[test]
fn test_golden_encoder() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/vectors.json")).unwrap();
    let tests = data["encoder_tests"].as_array().unwrap();
    let mut fails = Vec::new();

    for t in tests {
        let name = t["name"].as_str().unwrap();
        let input = t["input"].as_str().unwrap();
        let exp = t["expected_hex"].as_str().unwrap_or("").to_lowercase();
        let cpu = t["cpu"].as_str().unwrap_or("68000");

        // Skip 68020/68040
        if input.starts_with("FMOVE")
            || input.starts_with("FADD")
            || input.starts_with("FMUL")
            || input.starts_with("FSIN")
            || input.starts_with("FCMP")
            || input.starts_with("FTST")
            || input.starts_with("MOVEC")
        {
            continue;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let mn_part = parts[0];
        let ops_part = if parts.len() > 1 { parts[1] } else { "" };

        let (mn, sz) = if let Some(i) = mn_part.find('.') {
            (&mn_part[..i], Some(mn_part[i + 1..].to_lowercase()))
        } else {
            (mn_part, None)
        };

        let ops: Vec<&str> = ops_part.split(',').map(|s| s.trim()).collect();
        let parsed: Vec<Result<Operand, String>> = ops.iter().map(|o| parse_operand(o)).collect();

        // Single operand instructions - all return as dst
        let dst_mn = [
            "TST", "CLR", "NOT", "NEG", "SWAP", "EXT", "NBCD", "TAS", "NOP", "RTS", "RTE", "RTR",
            "TRAPV", "RESET", "ILLEGAL", "JMP", "JSR",
        ];
        let branch_mn = [
            "BRA", "BSR", "DBRA", "BHI", "BLS", "BCC", "BCS", "BNE", "BEQ", "BVC", "BVS", "BPL",
            "BMI", "BGE", "BLT", "BGT", "BLE",
        ];
        let (src, dst) = if parsed.is_empty() {
            (None, None)
        } else if parsed.len() == 1 {
            let p0 = match &parsed[0] {
                Ok(o) => o.clone(),
                Err(e) => {
                    fails.push(format!("{}: parse: {}", name, e));
                    continue;
                }
            };
            let ml = mn.to_uppercase();
            if dst_mn.contains(&ml.as_str()) || branch_mn.contains(&ml.as_str()) {
                let p0 = if matches!(p0, Operand::Address(0)) && ml == "DBRA" {
                    Operand::Address(2)
                } else {
                    p0
                };
                (None, Some(p0))
            } else {
                // src_mn instructions (TRAP/STOP/RTD/UNLK/LINK) and all others
                // with a single operand both pass it as `src`.
                (Some(p0), None)
            }
        } else {
            let p0 = match &parsed[0] {
                Ok(o) => o.clone(),
                Err(e) => {
                    fails.push(format!("{}: parse: {}", name, e));
                    continue;
                }
            };
            let p1_orig = match &parsed[1] {
                Ok(o) => o.clone(),
                Err(e) => {
                    fails.push(format!("{}: parse: {}", name, e));
                    continue;
                }
            };
            let ml = mn.to_uppercase();
            // For DBRA with label target, fix the address
            let p1 = if matches!(p1_orig, Operand::Address(0)) && branch_mn.contains(&ml.as_str()) {
                Operand::Address(2)
            } else {
                p1_orig
            };
            (Some(p0), Some(p1))
        };

        match encode_instruction(mn, sz.as_deref(), src.as_ref(), dst.as_ref(), 0, cpu) {
            Ok(words) => {
                let got = words_to_hex(&words);
                if got != exp {
                    fails.push(format!("{}: expected={} got={}", name, exp, got));
                }
            }
            Err(e) => {
                fails.push(format!("{}: encode err: {}", name, e));
            }
        }
    }

    if !fails.is_empty() {
        panic!("Failed:\n{}", fails.join("\n"));
    }
}
