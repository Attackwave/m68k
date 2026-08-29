//! Cycle timing estimations for Motorola 68000 instructions.

/// Cycle estimation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleEstimate {
    /// Nominal or base clock cycles.
    pub cycles: u32,
    /// Human-readable explanation / breakdown (e.g. "4 cycles", "10 (taken) / 8 (not taken)", "6 + 2n").
    pub display: String,
}

impl CycleEstimate {
    pub fn fixed(cycles: u32) -> Self {
        Self {
            cycles,
            display: format!("{} cycles", cycles),
        }
    }

    pub fn variable(cycles: u32, display: &str) -> Self {
        Self {
            cycles,
            display: display.to_string(),
        }
    }
}

/// Estimate base execution cycles on a Motorola 68000 for a parsed instruction.
pub fn estimate_cycles(
    mnemonic: &str,
    size: Option<&str>,
    operands: &[String],
) -> Option<CycleEstimate> {
    let m = mnemonic.to_ascii_lowercase();
    let sz = size.map(|s| s.to_ascii_lowercase()).unwrap_or_default();

    match m.as_str() {
        "nop" => Some(CycleEstimate::fixed(4)),
        "rts" => Some(CycleEstimate::fixed(16)),
        "rte" => Some(CycleEstimate::fixed(20)),
        "rtr" => Some(CycleEstimate::fixed(20)),
        "unlk" => Some(CycleEstimate::fixed(12)),
        "illegal" | "trapv" => Some(CycleEstimate::fixed(4)),
        "swap" | "ext" => Some(CycleEstimate::fixed(4)),
        "exg" => Some(CycleEstimate::fixed(6)),
        "moveq" => Some(CycleEstimate::fixed(4)),

        "bra" => Some(CycleEstimate::fixed(10)),
        "bsr" => Some(CycleEstimate::fixed(18)),

        // Bcc
        _ if m.starts_with('b')
            && m.len() <= 4
            && m != "bset"
            && m != "bclr"
            && m != "bchg"
            && m != "btst"
            && m != "bfextu"
            && m != "bfexts"
            && m != "bftst" =>
        {
            Some(CycleEstimate::variable(10, "10 (taken) / 8 (not taken)"))
        }

        // DBcc
        _ if m.starts_with("db") && m.len() <= 5 => Some(CycleEstimate::variable(
            10,
            "10 (loop taken) / 14 (not taken / condition met)",
        )),

        // Scc
        _ if m.starts_with('s')
            && m.len() <= 4
            && m != "sub"
            && m != "suba"
            && m != "subi"
            && m != "subq"
            && m != "subx"
            && m != "swap"
            && m != "stop" =>
        {
            if operands.first().is_some_and(|op| is_data_reg(op)) {
                Some(CycleEstimate::variable(4, "4 (true) / 6 (false)"))
            } else {
                Some(CycleEstimate::variable(8, "8+ (memory)"))
            }
        }

        "jmp" => {
            if operands.first().is_some_and(|op| is_addr_indirect(op)) {
                Some(CycleEstimate::fixed(8))
            } else {
                Some(CycleEstimate::variable(10, "10-14 cycles"))
            }
        }

        "jsr" => {
            if operands.first().is_some_and(|op| is_addr_indirect(op)) {
                Some(CycleEstimate::fixed(16))
            } else {
                Some(CycleEstimate::variable(18, "18-20 cycles"))
            }
        }

        "link" => Some(CycleEstimate::fixed(16)),
        "lea" => {
            if operands.first().is_some_and(|op| is_addr_indirect(op)) {
                Some(CycleEstimate::fixed(4))
            } else {
                Some(CycleEstimate::variable(8, "8-12 cycles"))
            }
        }
        "pea" => Some(CycleEstimate::variable(12, "12-20 cycles")),

        "move" | "movea" => {
            if operands.len() >= 2 {
                let src = &operands[0];
                let dst = &operands[1];
                let is_long = sz == "l";
                let src_reg = is_reg(src);
                let dst_reg = is_reg(dst);

                if src_reg && dst_reg {
                    Some(CycleEstimate::fixed(4))
                } else if (is_addr_indirect(src) && dst_reg) || (src_reg && is_addr_indirect(dst)) {
                    Some(CycleEstimate::fixed(if is_long { 12 } else { 8 }))
                } else {
                    Some(CycleEstimate::variable(
                        if is_long { 16 } else { 12 },
                        if is_long {
                            "12-24 cycles"
                        } else {
                            "8-16 cycles"
                        },
                    ))
                }
            } else {
                Some(CycleEstimate::fixed(4))
            }
        }

        "add" | "sub" | "and" | "or" => {
            let is_long = sz == "l";
            if operands.len() >= 2 && is_reg(&operands[0]) && is_reg(&operands[1]) {
                Some(CycleEstimate::fixed(if is_long { 6 } else { 4 }))
            } else {
                Some(CycleEstimate::variable(
                    if is_long { 14 } else { 8 },
                    if is_long {
                        "6-16 cycles"
                    } else {
                        "4-12 cycles"
                    },
                ))
            }
        }

        "adda" | "suba" | "cmpa" => {
            let is_long = sz == "l";
            Some(CycleEstimate::fixed(if is_long { 6 } else { 8 }))
        }

        "addq" | "subq" => {
            let is_long = sz == "l";
            if operands.get(1).is_some_and(|op| is_data_reg(op)) {
                Some(CycleEstimate::fixed(if is_long { 8 } else { 4 }))
            } else if operands.get(1).is_some_and(|op| is_addr_reg(op)) {
                Some(CycleEstimate::fixed(8))
            } else {
                Some(CycleEstimate::variable(8, "8-16 cycles"))
            }
        }

        "cmp" => {
            let is_long = sz == "l";
            if operands.len() >= 2 && is_reg(&operands[0]) && is_reg(&operands[1]) {
                Some(CycleEstimate::fixed(if is_long { 6 } else { 4 }))
            } else {
                Some(CycleEstimate::variable(6, "4-14 cycles"))
            }
        }

        "clr" | "tst" | "not" | "neg" | "negx" => {
            let is_long = sz == "l";
            if operands.first().is_some_and(|op| is_data_reg(op)) {
                Some(CycleEstimate::fixed(if is_long { 6 } else { 4 }))
            } else {
                Some(CycleEstimate::variable(8, "4-16 cycles"))
            }
        }

        "btst" => {
            if operands.get(1).is_some_and(|op| is_data_reg(op)) {
                Some(CycleEstimate::fixed(6))
            } else {
                Some(CycleEstimate::variable(4, "4-8 cycles"))
            }
        }

        "bset" | "bclr" | "bchg" => {
            if operands.get(1).is_some_and(|op| is_data_reg(op)) {
                Some(CycleEstimate::fixed(8))
            } else {
                Some(CycleEstimate::variable(12, "12-16 cycles"))
            }
        }

        "asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr" => {
            if operands.first().is_some_and(|op| op.starts_with('#')) {
                if let Ok(count) = operands[0].trim_start_matches('#').parse::<u32>() {
                    Some(CycleEstimate::fixed(6 + 2 * count))
                } else {
                    Some(CycleEstimate::variable(8, "6 + 2n cycles"))
                }
            } else if operands.len() == 1 {
                Some(CycleEstimate::fixed(8))
            } else {
                Some(CycleEstimate::variable(8, "6 + 2n cycles"))
            }
        }

        "muls" | "mulu" => Some(CycleEstimate::variable(70, "38-70 cycles (max 70)")),
        "divs" => Some(CycleEstimate::variable(158, "max 158 cycles")),
        "divu" => Some(CycleEstimate::variable(140, "max 140 cycles")),

        _ => None,
    }
}

fn is_data_reg(op: &str) -> bool {
    let s = op.trim().to_ascii_lowercase();
    matches!(
        s.as_str(),
        "d0" | "d1" | "d2" | "d3" | "d4" | "d5" | "d6" | "d7"
    )
}

fn is_addr_reg(op: &str) -> bool {
    let s = op.trim().to_ascii_lowercase();
    matches!(
        s.as_str(),
        "a0" | "a1" | "a2" | "a3" | "a4" | "a5" | "a6" | "a7" | "sp"
    )
}

fn is_reg(op: &str) -> bool {
    is_data_reg(op) || is_addr_reg(op)
}

fn is_addr_indirect(op: &str) -> bool {
    let s = op.trim();
    s.starts_with('(') && s.ends_with(')')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_cycles() {
        assert_eq!(
            estimate_cycles("nop", None, &[]),
            Some(CycleEstimate::fixed(4))
        );
        assert_eq!(
            estimate_cycles("rts", None, &[]),
            Some(CycleEstimate::fixed(16))
        );
        assert_eq!(
            estimate_cycles("move", Some("w"), &["d0".into(), "d1".into()]),
            Some(CycleEstimate::fixed(4))
        );
        assert_eq!(
            estimate_cycles("bra", None, &["target".into()]),
            Some(CycleEstimate::fixed(10))
        );
        assert_eq!(
            estimate_cycles("asl", Some("w"), &["#2".into(), "d0".into()]),
            Some(CycleEstimate::fixed(10))
        );
    }
}
