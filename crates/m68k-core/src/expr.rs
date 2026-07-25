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
        self.evaluate_dyn(&resolve_symbol)
    }

    // Recurses through `&dyn Fn` rather than the generic `impl Fn` above:
    // each recursive call to a function still generic over the closure
    // type instantiates a *new* concrete type (an extra layer of `&`), and
    // Expr::Binary makes two such calls per level, so the instantiation
    // count explodes with tree depth until rustc hits its recursion limit
    // even for small, non-pathological expressions. `&dyn Fn` is already a
    // concrete (non-generic) type, so recursing through it doesn't grow.
    fn evaluate_dyn(
        &self,
        resolve_symbol: &dyn Fn(&str) -> Result<i64, String>,
    ) -> Result<i64, String> {
        match self {
            Expr::Num(n) => Ok(*n),
            Expr::Ident(name) => resolve_symbol(name),
            Expr::Unary(op, expr) => {
                let val = expr.evaluate_dyn(resolve_symbol)?;
                match op {
                    // wrapping_neg avoids a debug-build panic on -i64::MIN.
                    UnaryOp::Neg => Ok(val.wrapping_neg()),
                    UnaryOp::Not => Ok(!val),
                    UnaryOp::High => Ok((val >> 8) & 0xFF),
                    UnaryOp::Low => Ok(val & 0xFF),
                    UnaryOp::Tilde => Ok(!val),
                }
            }
            Expr::Binary(op, left, right) => {
                let l = left.evaluate_dyn(resolve_symbol)?;
                let r = right.evaluate_dyn(resolve_symbol)?;
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
                    // wrapping_shl/shr mask the shift count (mod 64) like
                    // directives.rs's evaluator, instead of panicking in
                    // debug builds (or, in release, invoking Rust's own
                    // unspecified-but-defined masking implicitly) when the
                    // count is >= 64 or negative.
                    BinOp::Shl => Ok(l.wrapping_shl(r as u32)),
                    BinOp::Shr => Ok(l.wrapping_shr(r as u32)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn no_symbols(name: &str) -> Result<i64, String> {
        Err(format!("undefined symbol: {}", name))
    }

    /// Regression: a shift count >= 64 (or negative, once cast to u32)
    /// previously panicked in debug builds via the plain `<<`/`>>`
    /// operators. wrapping_shl/shr mask the count instead, matching
    /// directives.rs's evaluator.
    #[test]
    fn test_shift_overflow_does_not_panic() {
        let expr = Expr::Binary(BinOp::Shl, Box::new(Expr::Num(1)), Box::new(Expr::Num(64)));
        assert!(expr.evaluate(no_symbols).is_ok());

        let expr = Expr::Binary(BinOp::Shl, Box::new(Expr::Num(1)), Box::new(Expr::Num(100)));
        assert!(expr.evaluate(no_symbols).is_ok());

        let expr = Expr::Binary(BinOp::Shr, Box::new(Expr::Num(8)), Box::new(Expr::Num(-1)));
        assert!(expr.evaluate(no_symbols).is_ok());
    }

    /// Regression: negating i64::MIN previously panicked in debug builds
    /// via the plain unary `-` operator (its magnitude has no positive
    /// i64 representation). wrapping_neg wraps back to i64::MIN instead.
    #[test]
    fn test_neg_i64_min_does_not_panic() {
        let expr = Expr::Unary(UnaryOp::Neg, Box::new(Expr::Num(i64::MIN)));
        assert_eq!(expr.evaluate(no_symbols), Ok(i64::MIN));
    }

    #[test]
    fn test_basic_arithmetic() {
        let expr = Expr::Binary(BinOp::Add, Box::new(Expr::Num(2)), Box::new(Expr::Num(3)));
        assert_eq!(expr.evaluate(no_symbols), Ok(5));
    }
}
