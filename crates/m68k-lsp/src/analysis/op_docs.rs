//! Motorola 68000 family opcode documentation, syntax, condition codes, and CPU requirements.

use m68k_core::cpu_gate::MinCpu;

/// Condition code register (CCR) flag impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagEffect {
    /// Flag is not affected / unchanged.
    Unchanged,
    /// Flag is cleared to 0.
    Cleared,
    /// Flag is set to 1.
    Set,
    /// Modified according to the result.
    Modified,
    /// Undefined / unpredictable.
    Undefined,
}

impl FlagEffect {
    pub fn symbol(self) -> &'static str {
        match self {
            FlagEffect::Unchanged => "-",
            FlagEffect::Cleared => "0",
            FlagEffect::Set => "1",
            FlagEffect::Modified => "*",
            FlagEffect::Undefined => "U",
        }
    }

    pub fn description(self, flag: &'static str) -> &'static str {
        match self {
            FlagEffect::Unchanged => "Unchanged",
            FlagEffect::Cleared => "Cleared (0)",
            FlagEffect::Set => "Set (1)",
            FlagEffect::Modified => match flag {
                "X" => "Set the same as the Carry bit",
                "N" => "Set if result is negative, cleared otherwise",
                "Z" => "Set if result is zero, cleared otherwise",
                "V" => "Set if an overflow occurs, cleared otherwise",
                "C" => "Set if a carry or borrow occurs, cleared otherwise",
                _ => "Modified according to result",
            },
            FlagEffect::Undefined => "Undefined",
        }
    }
}

/// CCR status flag effects for an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcrFlags {
    pub x: FlagEffect,
    pub n: FlagEffect,
    pub z: FlagEffect,
    pub v: FlagEffect,
    pub c: FlagEffect,
}

impl CcrFlags {
    pub const fn new(
        x: FlagEffect,
        n: FlagEffect,
        z: FlagEffect,
        v: FlagEffect,
        c: FlagEffect,
    ) -> Self {
        Self { x, n, z, v, c }
    }

    pub const fn none() -> Self {
        Self::new(
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
        )
    }

    pub const fn standard_logic() -> Self {
        Self::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        )
    }

    pub const fn standard_arith() -> Self {
        Self::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
        )
    }

    pub const fn compare() -> Self {
        Self::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
        )
    }
}

/// Rich metadata for a 68k instruction.
#[derive(Debug, Clone)]
pub struct OpcodeDoc {
    pub mnemonic: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub syntax: &'static [&'static str],
    pub valid_sizes: &'static [&'static str],
    pub min_cpu: MinCpu,
    pub ccr: CcrFlags,
    pub base_cycles: &'static str,
}

/// Returns the documentation entry for an instruction mnemonic, if known.
pub fn lookup_opcode(mnemonic: &str) -> Option<OpcodeDoc> {
    let lower = mnemonic.to_ascii_lowercase();

    // Check direct mnemonic table
    if let Some(doc) = OPCODES
        .iter()
        .find(|d| d.mnemonic.eq_ignore_ascii_case(&lower))
    {
        return Some((*doc).clone());
    }

    // Check Bcc branch family
    if let Some(cc) = lower.strip_prefix('b')
        && let Some(desc) = condition_description(cc)
    {
        return Some(OpcodeDoc {
            mnemonic: "Bcc",
            title: "Branch Conditionally",
            summary: desc,
            syntax: &["B<cc> <label>", "B<cc>.<size> <label>"],
            valid_sizes: &[".S", ".W", ".L (68020+)"],
            min_cpu: MinCpu::Mc68000,
            ccr: CcrFlags::none(),
            base_cycles: "10 (taken) / 8 (not taken)",
        });
    }

    // Check DBcc branch family
    if let Some(cc) = lower.strip_prefix("db")
        && let Some(desc) = condition_description(cc)
    {
        return Some(OpcodeDoc {
            mnemonic: "DBcc",
            title: "Test Condition, Decrement and Branch",
            summary: desc,
            syntax: &["DB<cc> Dn,<label>"],
            valid_sizes: &[".W"],
            min_cpu: MinCpu::Mc68000,
            ccr: CcrFlags::none(),
            base_cycles: "10 (taken) / 14 (not taken / expired)",
        });
    }

    // Check Scc set conditionally family
    if let Some(cc) = lower.strip_prefix('s')
        && let Some(desc) = condition_description(cc)
    {
        return Some(OpcodeDoc {
            mnemonic: "Scc",
            title: "Set Byte Conditionally",
            summary: desc,
            syntax: &["S<cc> <ea>"],
            valid_sizes: &[".B"],
            min_cpu: MinCpu::Mc68000,
            ccr: CcrFlags::none(),
            base_cycles: "4 (Dn, true) / 6 (Dn, false) / 8+ (ea)",
        });
    }

    // Check TRAPcc family
    if let Some(cc) = lower.strip_prefix("trap")
        && let Some(desc) = condition_description(cc)
    {
        return Some(OpcodeDoc {
            mnemonic: "TRAPcc",
            title: "Trap Conditionally",
            summary: desc,
            syntax: &["TRAP<cc>", "TRAP<cc>.W #<data>", "TRAP<cc>.L #<data>"],
            valid_sizes: &["", ".W", ".L"],
            min_cpu: MinCpu::Mc68020,
            ccr: CcrFlags::none(),
            base_cycles: "4 (no trap) / 24+ (trap taken)",
        });
    }

    // Check FPU FBcc family
    if let Some(cc) = lower.strip_prefix("fb")
        && let Some(desc) = fpu_condition_description(cc)
    {
        return Some(OpcodeDoc {
            mnemonic: "FBcc",
            title: "Branch on Floating-Point Condition",
            summary: desc,
            syntax: &["FB<cc> <label>", "FB<cc>.<size> <label>"],
            valid_sizes: &[".W", ".L"],
            min_cpu: MinCpu::Mc68020,
            ccr: CcrFlags::none(),
            base_cycles: "coprocessor dependent",
        });
    }

    None
}

/// Description of standard 68k branch condition codes.
fn condition_description(cc: &str) -> Option<&'static str> {
    match cc {
        "t" | "ra" => Some("True / Always taken"),
        "f" | "sr" => Some("False / Never taken"),
        "hi" => Some("High: unsigned greater than (C = 0 and Z = 0)"),
        "ls" => Some("Low or Same: unsigned less or equal (C = 1 or Z = 1)"),
        "cc" | "hs" => Some("Carry Clear / Higher or Same: unsigned greater/equal (C = 0)"),
        "cs" | "lo" => Some("Carry Set / Lower: unsigned strictly less than (C = 1)"),
        "ne" => Some("Not Equal: non-zero (Z = 0)"),
        "eq" => Some("Equal: zero (Z = 1)"),
        "vc" => Some("Overflow Clear: no signed overflow (V = 0)"),
        "vs" => Some("Overflow Set: signed overflow (V = 1)"),
        "pl" => Some("Plus: positive result (N = 0)"),
        "mi" => Some("Minus: negative result (N = 1)"),
        "ge" => Some("Greater or Equal: signed greater/equal (N and V same)"),
        "lt" => Some("Less Than: signed strictly less (N and V differ)"),
        "gt" => Some("Greater Than: signed strictly greater (Z = 0 and N,V same)"),
        "le" => Some("Less or Equal: signed less/equal (Z = 1 or N,V differ)"),
        _ => None,
    }
}

/// Description of FPU condition codes.
fn fpu_condition_description(cc: &str) -> Option<&'static str> {
    match cc {
        "eq" => Some("Equal (Z = 1)"),
        "neq" | "ne" => Some("Not Equal (Z = 0)"),
        "gt" => Some("Greater Than (N = 0, Z = 0, NAN = 0)"),
        "nge" => Some("Not Greater or Equal (NAN = 1 or (N = 1 and Z = 0))"),
        "ge" => Some("Greater or Equal (Z = 1 or (N = 0 and NAN = 0))"),
        "ngt" => Some("Not Greater Than (Z = 1 or N = 1 or NAN = 1)"),
        "lt" => Some("Less Than (N = 1, Z = 0, NAN = 0)"),
        "nle" => Some("Not Less or Equal (NAN = 1 or (N = 0 and Z = 0))"),
        "le" => Some("Less or Equal (Z = 1 or (N = 1 and NAN = 0))"),
        "nlt" => Some("Not Less Than (Z = 1 or N = 0 or NAN = 1)"),
        "gl" => Some("Greater or Less (NAN = 0, Z = 0)"),
        "ngl" => Some("Not Greater or Less (NAN = 1 or Z = 1)"),
        "gle" => Some("Greater, Less or Equal (NAN = 0)"),
        "ngle" => Some("Not (Greater, Less or Equal) (NAN = 1)"),
        "ogt" => Some("Ordered Greater Than"),
        "oge" => Some("Ordered Greater or Equal"),
        "olt" => Some("Ordered Less Than"),
        "ole" => Some("Ordered Less or Equal"),
        "ogl" => Some("Ordered Greater or Less"),
        "or" => Some("Ordered"),
        "un" => Some("Unordered"),
        "ueq" => Some("Unordered or Equal"),
        "ugt" => Some("Unordered or Greater Than"),
        "uge" => Some("Unordered or Greater or Equal"),
        "ult" => Some("Unordered or Less Than"),
        "ule" => Some("Unordered or Less or Equal"),
        "sf" => Some("Signaling False"),
        "seq" => Some("Signaling Equal"),
        "sne" => Some("Signaling Not Equal"),
        "st" => Some("Signaling True"),
        _ => None,
    }
}

/// Static table of all core m68k instructions.
static OPCODES: &[OpcodeDoc] = &[
    // Data Movement
    OpcodeDoc {
        mnemonic: "move",
        title: "Move Data",
        summary: "Moves data from source effective address to destination effective address.",
        syntax: &["MOVE.<size> <ea>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4 (Dn,Dn) / 8-16+ (Memory)",
    },
    OpcodeDoc {
        mnemonic: "movea",
        title: "Move Address",
        summary: "Moves data from source effective address to an address register with sign-extension.",
        syntax: &["MOVEA.<size> <ea>,An", "MOVE.<size> <ea>,An"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "4 (Dn,An) / 8-16+ (Memory)",
    },
    OpcodeDoc {
        mnemonic: "moveq",
        title: "Move Quick",
        summary: "Loads a sign-extended 8-bit immediate value into a data register.",
        syntax: &["MOVEQ #<data8>,Dn"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4",
    },
    OpcodeDoc {
        mnemonic: "movem",
        title: "Move Multiple Registers",
        summary: "Transfers multiple data and address registers to/from memory.",
        syntax: &["MOVEM.<size> <list>,<ea>", "MOVEM.<size> <ea>,<list>"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "8+4n (Reg to Mem) / 12+4n (Mem to Reg)",
    },
    OpcodeDoc {
        mnemonic: "movep",
        title: "Move Peripheral Data",
        summary: "Transfers data between a data register and alternate bytes of memory (for 8-bit peripherals).",
        syntax: &["MOVEP.<size> Dn,d(An)", "MOVEP.<size> d(An),Dn"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "16 (.W) / 24 (.L)",
    },
    OpcodeDoc {
        mnemonic: "exg",
        title: "Exchange Registers",
        summary: "Exchanges the 32-bit contents of two registers (Dn/Dm, An/Am, or Dn/An).",
        syntax: &["EXG Rx,Ry"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "6",
    },
    OpcodeDoc {
        mnemonic: "swap",
        title: "Swap Register Halves",
        summary: "Exchanges the upper and lower 16-bit halves of a data register.",
        syntax: &["SWAP Dn"],
        valid_sizes: &[".W"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4",
    },
    OpcodeDoc {
        mnemonic: "ext",
        title: "Sign Extend",
        summary: "Sign-extends byte to word or word to longword in a data register.",
        syntax: &["EXT.W Dn", "EXT.L Dn"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4",
    },
    OpcodeDoc {
        mnemonic: "extb",
        title: "Sign Extend Byte to Long",
        summary: "Sign-extends an 8-bit byte directly to 32-bit longword in a data register.",
        syntax: &["EXTB.L Dn"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68020,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4",
    },
    OpcodeDoc {
        mnemonic: "lea",
        title: "Load Effective Address",
        summary: "Calculates an effective address and loads it into an address register without accessing memory.",
        syntax: &["LEA <ea>,An"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "4-12 (depends on ea)",
    },
    OpcodeDoc {
        mnemonic: "pea",
        title: "Push Effective Address",
        summary: "Calculates an effective address and pushes it onto the stack (SP).",
        syntax: &["PEA <ea>"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "12-20 (depends on ea)",
    },
    OpcodeDoc {
        mnemonic: "link",
        title: "Link and Allocate",
        summary: "Pushes An onto the stack, sets An to the new SP, and reserves stack frame space.",
        syntax: &["LINK An,#<disp16>", "LINK.L An,#<disp32> (68020+)"],
        valid_sizes: &[".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "16 (.W) / 16 (.L 68020)",
    },
    OpcodeDoc {
        mnemonic: "unlk",
        title: "Unlink Stack Frame",
        summary: "Restores SP from An, then pops the previous An from the stack.",
        syntax: &["UNLK An"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "12",
    },
    // Arithmetic Operations
    OpcodeDoc {
        mnemonic: "add",
        title: "Add Binary",
        summary: "Adds source to destination and stores result in destination.",
        syntax: &["ADD.<size> <ea>,Dn", "ADD.<size> Dn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn,Dn) / 6-8 (.L) / 8-24 (ea)",
    },
    OpcodeDoc {
        mnemonic: "adda",
        title: "Add Address",
        summary: "Adds source operand to an address register with sign-extension.",
        syntax: &["ADDA.<size> <ea>,An", "ADD.<size> <ea>,An"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "8 (.W) / 6-8 (.L)",
    },
    OpcodeDoc {
        mnemonic: "addi",
        title: "Add Immediate",
        summary: "Adds immediate value to destination operand.",
        syntax: &["ADDI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "8 (Dn) / 14 (.L Dn) / 12-28 (ea)",
    },
    OpcodeDoc {
        mnemonic: "addq",
        title: "Add Quick",
        summary: "Adds a fast 3-bit immediate (1-8) to destination operand.",
        syntax: &["ADDQ.<size> #<1-8>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn) / 8 (An) / 8-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "addx",
        title: "Add with Extend",
        summary: "Adds source operand, destination operand, and extend bit (X).",
        syntax: &["ADDX.<size> Dy,Dx", "ADDX.<size> -(Ay),-(Ax)"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Reg) / 8 (.L Reg) / 18-30 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "sub",
        title: "Subtract Binary",
        summary: "Subtracts source from destination and stores result in destination.",
        syntax: &["SUB.<size> <ea>,Dn", "SUB.<size> Dn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn,Dn) / 6-8 (.L) / 8-24 (ea)",
    },
    OpcodeDoc {
        mnemonic: "suba",
        title: "Subtract Address",
        summary: "Subtracts source operand from address register with sign-extension.",
        syntax: &["SUBA.<size> <ea>,An", "SUB.<size> <ea>,An"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "8 (.W) / 6-8 (.L)",
    },
    OpcodeDoc {
        mnemonic: "subi",
        title: "Subtract Immediate",
        summary: "Subtracts immediate value from destination operand.",
        syntax: &["SUBI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "8 (Dn) / 14 (.L Dn) / 12-28 (ea)",
    },
    OpcodeDoc {
        mnemonic: "subq",
        title: "Subtract Quick",
        summary: "Subtracts a fast 3-bit immediate (1-8) from destination operand.",
        syntax: &["SUBQ.<size> #<1-8>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn) / 8 (An) / 8-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "subx",
        title: "Subtract with Extend",
        summary: "Subtracts source operand and extend bit (X) from destination operand.",
        syntax: &["SUBX.<size> Dy,Dx", "SUBX.<size> -(Ay),-(Ax)"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Reg) / 8 (.L Reg) / 18-30 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "cmp",
        title: "Compare",
        summary: "Subtracts source operand from destination data register and updates CCR without storing result.",
        syntax: &["CMP.<size> <ea>,Dn"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::compare(),
        base_cycles: "4 (Dn,Dn) / 6 (.L) / 8-14 (ea)",
    },
    OpcodeDoc {
        mnemonic: "cmpa",
        title: "Compare Address",
        summary: "Compares source operand with an address register.",
        syntax: &["CMPA.<size> <ea>,An", "CMP.<size> <ea>,An"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::compare(),
        base_cycles: "6",
    },
    OpcodeDoc {
        mnemonic: "cmpi",
        title: "Compare Immediate",
        summary: "Compares immediate value with destination operand.",
        syntax: &["CMPI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::compare(),
        base_cycles: "8 (Dn) / 12-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "cmpm",
        title: "Compare Memory",
        summary: "Compares memory location with another using post-increment.",
        syntax: &["CMPM.<size> (Ay)+,(Ax)+"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::compare(),
        base_cycles: "12 (.B/.W) / 20 (.L)",
    },
    OpcodeDoc {
        mnemonic: "cmp2",
        title: "Compare Register Against Bounds",
        summary: "Compares register against a lower and upper pair of bounds in memory.",
        syntax: &["CMP2.<size> <ea>,Rn"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68020,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Undefined,
            FlagEffect::Modified,
            FlagEffect::Undefined,
            FlagEffect::Modified,
        ),
        base_cycles: "18-24 (68020)",
    },
    OpcodeDoc {
        mnemonic: "muls",
        title: "Signed Multiply",
        summary: "Multiplies two signed 16-bit operands (or 32-bit on 68020+) to produce product.",
        syntax: &[
            "MULS.W <ea>,Dn",
            "MULS.L <ea>,Dl (68020+)",
            "MULS.L <ea>,Dh:Dl (68020+)",
        ],
        valid_sizes: &[".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
        ),
        base_cycles: "70 (max 68000) / 27-43 (68020)",
    },
    OpcodeDoc {
        mnemonic: "mulu",
        title: "Unsigned Multiply",
        summary: "Multiplies two unsigned 16-bit operands (or 32-bit on 68020+) to produce product.",
        syntax: &[
            "MULU.W <ea>,Dn",
            "MULU.L <ea>,Dl (68020+)",
            "MULU.L <ea>,Dh:Dl (68020+)",
        ],
        valid_sizes: &[".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
        ),
        base_cycles: "70 (max 68000) / 27-43 (68020)",
    },
    OpcodeDoc {
        mnemonic: "divs",
        title: "Signed Divide",
        summary: "Divides signed 32-bit destination by signed 16-bit divisor (produces 16-bit quotient and 16-bit remainder).",
        syntax: &[
            "DIVS.W <ea>,Dn",
            "DIVS.L <ea>,Dq (68020+)",
            "DIVS.L <ea>,Dr:Dq (68020+)",
        ],
        valid_sizes: &[".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
        ),
        base_cycles: "158 (max 68000) / 56-90 (68020)",
    },
    OpcodeDoc {
        mnemonic: "divu",
        title: "Unsigned Divide",
        summary: "Divides unsigned 32-bit destination by unsigned 16-bit divisor (produces 16-bit quotient and 16-bit remainder).",
        syntax: &[
            "DIVU.W <ea>,Dn",
            "DIVU.L <ea>,Dq (68020+)",
            "DIVU.L <ea>,Dr:Dq (68020+)",
        ],
        valid_sizes: &[".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
        ),
        base_cycles: "140 (max 68000) / 56-90 (68020)",
    },
    OpcodeDoc {
        mnemonic: "clr",
        title: "Clear an Operand",
        summary: "Sets destination operand to zero.",
        syntax: &["CLR.<size> <ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Cleared,
            FlagEffect::Set,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "neg",
        title: "Negate Binary",
        summary: "Subtracts destination operand from zero (two's complement).",
        syntax: &["NEG.<size> <ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "negx",
        title: "Negate with Extend",
        summary: "Subtracts destination operand and extend bit (X) from zero.",
        syntax: &["NEGX.<size> <ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "tst",
        title: "Test an Operand",
        summary: "Compares operand with zero and sets condition codes.",
        syntax: &["TST.<size> <ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Cleared,
        ),
        base_cycles: "4 (Dn) / 4-12 (ea)",
    },
    // Logical Operations
    OpcodeDoc {
        mnemonic: "and",
        title: "Logical AND",
        summary: "Performs bitwise logical AND between source and destination.",
        syntax: &["AND.<size> <ea>,Dn", "AND.<size> Dn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "andi",
        title: "AND Immediate",
        summary: "Performs bitwise logical AND with immediate data.",
        syntax: &["ANDI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "8 (Dn) / 14 (.L Dn) / 12-24 (ea)",
    },
    OpcodeDoc {
        mnemonic: "or",
        title: "Logical Inclusive OR",
        summary: "Performs bitwise logical inclusive OR between source and destination.",
        syntax: &["OR.<size> <ea>,Dn", "OR.<size> Dn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "ori",
        title: "OR Immediate",
        summary: "Performs bitwise logical OR with immediate data.",
        syntax: &["ORI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "8 (Dn) / 14 (.L Dn) / 12-24 (ea)",
    },
    OpcodeDoc {
        mnemonic: "eor",
        title: "Logical Exclusive OR",
        summary: "Performs bitwise logical exclusive OR between data register and destination.",
        syntax: &["EOR.<size> Dn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "4 (Dn) / 8 (.L Dn) / 8-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "eori",
        title: "Exclusive OR Immediate",
        summary: "Performs bitwise exclusive OR with immediate data.",
        syntax: &["EORI.<size> #<data>,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "8 (Dn) / 14 (.L Dn) / 12-24 (ea)",
    },
    OpcodeDoc {
        mnemonic: "not",
        title: "Logical NOT (One's Complement)",
        summary: "Inverts all bits of destination operand.",
        syntax: &["NOT.<size> <ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_logic(),
        base_cycles: "4 (Dn) / 6 (.L Dn) / 8-16 (ea)",
    },
    // Shifts and Rotates
    OpcodeDoc {
        mnemonic: "asl",
        title: "Arithmetic Shift Left",
        summary: "Shifts bits left arithmetically. Lowest bit is filled with 0, high bit goes to C and X flags.",
        syntax: &[
            "ASL.<size> #<cnt>,Dy",
            "ASL.<size> Dx,Dy",
            "ASL.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "asr",
        title: "Arithmetic Shift Right",
        summary: "Shifts bits right arithmetically. Sign bit is preserved and replicated, low bit goes to C and X flags.",
        syntax: &[
            "ASR.<size> #<cnt>,Dy",
            "ASR.<size> Dx,Dy",
            "ASR.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::standard_arith(),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "lsl",
        title: "Logical Shift Left",
        summary: "Shifts bits left logically. Lowest bit is filled with 0, high bit goes to C and X flags.",
        syntax: &[
            "LSL.<size> #<cnt>,Dy",
            "LSL.<size> Dx,Dy",
            "LSL.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "lsr",
        title: "Logical Shift Right",
        summary: "Shifts bits right logically. Highest bit is filled with 0, low bit goes to C and X flags.",
        syntax: &[
            "LSR.<size> #<cnt>,Dy",
            "LSR.<size> Dx,Dy",
            "LSR.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "rol",
        title: "Rotate Left without Extend",
        summary: "Rotates bits left cyclically. High bit rotates into low bit and C flag (X unaffected).",
        syntax: &[
            "ROL.<size> #<cnt>,Dy",
            "ROL.<size> Dx,Dy",
            "ROL.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "ror",
        title: "Rotate Right without Extend",
        summary: "Rotates bits right cyclically. Low bit rotates into high bit and C flag (X unaffected).",
        syntax: &[
            "ROR.<size> #<cnt>,Dy",
            "ROR.<size> Dx,Dy",
            "ROR.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "roxl",
        title: "Rotate Left with Extend",
        summary: "Rotates bits left cyclically through the 9/17/33-bit chain including X flag.",
        syntax: &[
            "ROXL.<size> #<cnt>,Dy",
            "ROXL.<size> Dx,Dy",
            "ROXL.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    OpcodeDoc {
        mnemonic: "roxr",
        title: "Rotate Right with Extend",
        summary: "Rotates bits right cyclically through the 9/17/33-bit chain including X flag.",
        syntax: &[
            "ROXR.<size> #<cnt>,Dy",
            "ROXR.<size> Dx,Dy",
            "ROXR.<size> <ea>",
        ],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Cleared,
            FlagEffect::Modified,
        ),
        base_cycles: "6+2n (Reg) / 8 (Mem)",
    },
    // Bit Manipulation
    OpcodeDoc {
        mnemonic: "btst",
        title: "Test a Bit",
        summary: "Tests a specific bit in destination and reflects its state in the Z flag (Z=1 if bit is 0).",
        syntax: &["BTST #<bit>,<ea>", "BTST Dn,<ea>"],
        valid_sizes: &[".B (Memory)", ".L (Register)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
        ),
        base_cycles: "6 (Dn) / 4-8 (ea)",
    },
    OpcodeDoc {
        mnemonic: "bset",
        title: "Test and Set a Bit",
        summary: "Tests a bit (storing old state in Z flag), then sets the bit to 1.",
        syntax: &["BSET #<bit>,<ea>", "BSET Dn,<ea>"],
        valid_sizes: &[".B (Memory)", ".L (Register)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
        ),
        base_cycles: "8 (Dn) / 12-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "bclr",
        title: "Test and Clear a Bit",
        summary: "Tests a bit (storing old state in Z flag), then clears the bit to 0.",
        syntax: &["BCLR #<bit>,<ea>", "BCLR Dn,<ea>"],
        valid_sizes: &[".B (Memory)", ".L (Register)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
        ),
        base_cycles: "8 (Dn) / 12-16 (ea)",
    },
    OpcodeDoc {
        mnemonic: "bchg",
        title: "Test and Change a Bit",
        summary: "Tests a bit (storing old state in Z flag), then inverts the bit.",
        syntax: &["BCHG #<bit>,<ea>", "BCHG Dn,<ea>"],
        valid_sizes: &[".B (Memory)", ".L (Register)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
            FlagEffect::Modified,
            FlagEffect::Unchanged,
            FlagEffect::Unchanged,
        ),
        base_cycles: "8 (Dn) / 12-16 (ea)",
    },
    // Program Control & Jumps
    OpcodeDoc {
        mnemonic: "bra",
        title: "Branch Always",
        summary: "Unconditionally branches to a PC-relative target label.",
        syntax: &["BRA <label>", "BRA.<size> <label>"],
        valid_sizes: &[".S", ".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "10",
    },
    OpcodeDoc {
        mnemonic: "bsr",
        title: "Branch to Subroutine",
        summary: "Pushes return address onto stack and branches to target subroutine.",
        syntax: &["BSR <label>", "BSR.<size> <label>"],
        valid_sizes: &[".S", ".W", ".L (68020+)"],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "18",
    },
    OpcodeDoc {
        mnemonic: "jmp",
        title: "Jump",
        summary: "Unconditionally jumps to an effective address target.",
        syntax: &["JMP <ea>"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "8 (An) / 10-14 (ea)",
    },
    OpcodeDoc {
        mnemonic: "jsr",
        title: "Jump to Subroutine",
        summary: "Pushes return address onto stack and jumps to an effective address target.",
        syntax: &["JSR <ea>"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "16 (An) / 18-20 (ea)",
    },
    OpcodeDoc {
        mnemonic: "rts",
        title: "Return from Subroutine",
        summary: "Pulls 32-bit return address from stack into PC.",
        syntax: &["RTS"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "16",
    },
    OpcodeDoc {
        mnemonic: "rte",
        title: "Return from Exception",
        summary: "Privileged. Pulls SR and PC (and exception stack frame) from the supervisor stack.",
        syntax: &["RTE"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
        ),
        base_cycles: "20 (68000)",
    },
    OpcodeDoc {
        mnemonic: "rtr",
        title: "Return and Restore Condition Codes",
        summary: "Pulls CCR and PC from the stack.",
        syntax: &["RTR"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::new(
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
            FlagEffect::Modified,
        ),
        base_cycles: "20",
    },
    OpcodeDoc {
        mnemonic: "nop",
        title: "No Operation",
        summary: "Performs no operation, advances PC by 2 bytes.",
        syntax: &["NOP"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "4",
    },
    OpcodeDoc {
        mnemonic: "trap",
        title: "Trap (Software Interrupt)",
        summary: "Initiates exception processing for vector 32 to 47 (TRAP #0 to #15).",
        syntax: &["TRAP #<vector0-15>"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "34 (68000)",
    },
    OpcodeDoc {
        mnemonic: "trapv",
        title: "Trap on Overflow",
        summary: "Takes vector 7 exception if V flag is set.",
        syntax: &["TRAPV"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "4 (no overflow) / 34 (trap taken)",
    },
    OpcodeDoc {
        mnemonic: "illegal",
        title: "Illegal Instruction",
        summary: "Forces an Illegal Instruction exception (vector 4, opcode $4AFC).",
        syntax: &["ILLEGAL"],
        valid_sizes: &[""],
        min_cpu: MinCpu::Mc68000,
        ccr: CcrFlags::none(),
        base_cycles: "34",
    },
    // System / Privileged Instructions
    OpcodeDoc {
        mnemonic: "movec",
        title: "Move Control Register",
        summary: "Privileged (68010+). Moves data between general register and CPU control register (VBR, CACR, CAAR, SFC, DFC, USP, MSP, ISP).",
        syntax: &["MOVEC Rc,Rn", "MOVEC Rn,Rc"],
        valid_sizes: &[".L"],
        min_cpu: MinCpu::Mc68010,
        ccr: CcrFlags::none(),
        base_cycles: "12 (68010)",
    },
    OpcodeDoc {
        mnemonic: "moves",
        title: "Move to/from Alternate Address Space",
        summary: "Privileged (68010+). Accesses memory using SFC (Source Function Code) or DFC (Destination Function Code).",
        syntax: &["MOVES.<size> <ea>,Rn", "MOVES.<size> Rn,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68010,
        ccr: CcrFlags::none(),
        base_cycles: "12-16",
    },
    OpcodeDoc {
        mnemonic: "cas",
        title: "Compare and Swap with Operand",
        summary: "Atomic memory update (68020+). Compares Dc with destination memory; if equal, updates with Du, otherwise updates Dc.",
        syntax: &["CAS.<size> Dc,Du,<ea>"],
        valid_sizes: &[".B", ".W", ".L"],
        min_cpu: MinCpu::Mc68020,
        ccr: CcrFlags::compare(),
        base_cycles: "12-25 (68020)",
    },
    OpcodeDoc {
        mnemonic: "cas2",
        title: "Compare and Swap Two Operands",
        summary: "Dual atomic memory update (68020+). Updates two memory locations simultaneously if both match comparison registers.",
        syntax: &["CAS2.<size> Dc1:Dc2,Du1:Du2,(Rn1):(Rn2)"],
        valid_sizes: &[".W", ".L"],
        min_cpu: MinCpu::Mc68020,
        ccr: CcrFlags::compare(),
        base_cycles: "24-40 (68020)",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_opcode() {
        let move_doc = lookup_opcode("move").expect("move should exist");
        assert_eq!(move_doc.mnemonic, "move");
        assert_eq!(move_doc.min_cpu, MinCpu::Mc68000);

        let extb_doc = lookup_opcode("extb").expect("extb should exist");
        assert_eq!(extb_doc.min_cpu, MinCpu::Mc68020);

        let bne_doc = lookup_opcode("bne").expect("bne should exist");
        assert_eq!(bne_doc.mnemonic, "Bcc");

        let dbeq_doc = lookup_opcode("dbeq").expect("dbeq should exist");
        assert_eq!(dbeq_doc.mnemonic, "DBcc");
    }
}
