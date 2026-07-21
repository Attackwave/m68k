//! Expression types for the m68k assembler.
//!
//! Supports arithmetic, bitwise, and comparison operators with proper precedence.

/// Binary operators in expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LAnd,
    LOr,
}

/// Unary operators in expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    High,
    Low,
    Tilde,
}

/// AST node for an expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Num(i64),
    Ident(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

impl Expr {
    pub fn evaluate(
        &self,
        resolve_symbol: impl Fn(&str) -> Result<i64, String>,
    ) -> Result<i64, String> {
        match self {
            Expr::Num(n) => Ok(*n),
            Expr::Ident(name) => resolve_symbol(name),
            Expr::Unary(op, expr) => {
                let val = expr.evaluate(&resolve_symbol)?;
                match op {
                    UnaryOp::Neg => Ok(-val),
                    UnaryOp::Not => Ok(!val),
                    UnaryOp::High => Ok((val >> 8) & 0xFF),
                    UnaryOp::Low => Ok(val & 0xFF),
                    UnaryOp::Tilde => Ok(!val),
                }
            }
            Expr::Binary(op, left, right) => {
                let l = left.evaluate(&resolve_symbol)?;
                let r = right.evaluate(&resolve_symbol)?;
                match op {
                    BinOp::Add => Ok(l.wrapping_add(r)),
                    BinOp::Sub => Ok(l.wrapping_sub(r)),
                    BinOp::Mul => Ok(l.wrapping_mul(r)),
                    BinOp::Div => {
                        if r == 0 {
                            Err("division by zero".to_string())
                        } else {
                            Ok(l.wrapping_div(r))
                        }
                    }
                    BinOp::Mod => {
                        if r == 0 {
                            Err("modulo by zero".to_string())
                        } else {
                            Ok(l.wrapping_rem(r))
                        }
                    }
                    BinOp::And => Ok(l & r),
                    BinOp::Or => Ok(l | r),
                    BinOp::Xor => Ok(l ^ r),
                    BinOp::Shl => Ok(l << (r as u32)),
                    BinOp::Shr => Ok(l >> (r as u32)),
                    BinOp::Eq => Ok(if l == r { 1 } else { 0 }),
                    BinOp::Ne => Ok(if l != r { 1 } else { 0 }),
                    BinOp::Lt => Ok(if l < r { 1 } else { 0 }),
                    BinOp::Le => Ok(if l <= r { 1 } else { 0 }),
                    BinOp::Gt => Ok(if l > r { 1 } else { 0 }),
                    BinOp::Ge => Ok(if l >= r { 1 } else { 0 }),
                    BinOp::LAnd => Ok(if l != 0 && r != 0 { 1 } else { 0 }),
                    BinOp::LOr => Ok(if l != 0 || r != 0 { 1 } else { 0 }),
                }
            }
        }
    }
}
