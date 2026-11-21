//! GAS expressions: parsing to a tree, and evaluation to a linear
//! combination of symbols.
//!
//! Precedence follows GAS: unary `-` `~` `+` bind tightest, then
//! `*` `/` `%` `<<` `>>`, then `&` `^` (`|` is the comment character on
//! m68k), then `+` `-`. Numbers are decimal, `0x` hex, `0b` binary or
//! octal with a leading `0`; `'c` is a character constant.

/// A parsed expression. Symbol names are kept as written; they are
/// resolved per file when the expression is evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Num(i64),
    Sym(String),
    /// `.`, the location of the statement (or data element) it is in.
    Dot,
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    And,
    Xor,
    Or,
}

impl Expr {
    /// Whether the expression names no symbol and no location, so its
    /// value is known while parsing.
    pub fn is_const(&self) -> bool {
        match self {
            Expr::Num(_) => true,
            Expr::Sym(_) | Expr::Dot => false,
            Expr::Neg(e) | Expr::Not(e) => e.is_const(),
            Expr::Bin(_, a, b) => a.is_const() && b.is_const(),
        }
    }

    /// The value of a constant expression.
    pub fn const_value(&self) -> Option<i64> {
        let lin: Linear<u8> = self.eval(&mut |_| Err(String::new()), None).ok()?;
        lin.terms.is_empty().then_some(lin.k)
    }

    /// The single symbol an expression adds, with the constant beside it:
    /// `sym` or `sym+n`/`sym-n`/`n+sym`. GAS relaxes only these.
    pub fn symbol_plus_const(&self) -> Option<(&str, i64)> {
        match self {
            Expr::Sym(s) => Some((s, 0)),
            Expr::Bin(BinOp::Add, a, b) => match (a.as_ref(), b.as_ref()) {
                (Expr::Sym(s), c) | (c, Expr::Sym(s)) => Some((s, c.const_value()?)),
                _ => None,
            },
            Expr::Bin(BinOp::Sub, a, b) => match a.as_ref() {
                Expr::Sym(s) => Some((s, b.const_value()?.checked_neg()?)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Evaluate to `k + sum(coeff * symbol)`. `sym` turns a symbol name
    /// into its id; `dot` is the id standing for `.`.
    pub fn eval<S: Copy + PartialEq>(
        &self,
        sym: &mut dyn FnMut(&str) -> Result<S, String>,
        dot: Option<S>,
    ) -> Result<Linear<S>, String> {
        Ok(match self {
            Expr::Num(n) => Linear::num(*n),
            Expr::Sym(name) => Linear::sym(sym(name)?),
            Expr::Dot => Linear::sym(dot.ok_or("'.' is not available here")?),
            Expr::Neg(e) => e.eval(sym, dot)?.scale(-1)?,
            Expr::Not(e) => Linear::num(!e.eval(sym, dot)?.constant("~")?),
            Expr::Bin(op, a, b) => {
                let a = a.eval(sym, dot)?;
                let b = b.eval(sym, dot)?;
                match op {
                    BinOp::Add => a.add(b, 1)?,
                    BinOp::Sub => a.add(b, -1)?,
                    BinOp::Mul if a.terms.is_empty() => b.scale(a.k)?,
                    BinOp::Mul => a.scale(b.constant("*")?)?,
                    _ => {
                        let (x, y) = (a.constant("operator")?, b.constant("operator")?);
                        let v = match op {
                            BinOp::Div | BinOp::Rem if y == 0 => {
                                return Err("division by zero".into());
                            }
                            BinOp::Div => x.wrapping_div(y),
                            BinOp::Rem => x.wrapping_rem(y),
                            BinOp::Shl | BinOp::Shr if !(0..64).contains(&y) => {
                                return Err(format!("shift count {y} out of range"));
                            }
                            BinOp::Shl => x << y,
                            // GAS shifts the value as unsigned.
                            BinOp::Shr => ((x as u64) >> y) as i64,
                            BinOp::And => x & y,
                            BinOp::Xor => x ^ y,
                            BinOp::Or => x | y,
                            BinOp::Add | BinOp::Sub | BinOp::Mul => unreachable!(),
                        };
                        Linear::num(v)
                    }
                }
            }
        })
    }
}

/// `k + sum(coeff * symbol)`, the value of an expression before symbols
/// have addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linear<S> {
    pub terms: Vec<(S, i64)>,
    pub k: i64,
}

impl<S: Copy + PartialEq> Linear<S> {
    fn num(k: i64) -> Self {
        Linear {
            terms: Vec::new(),
            k,
        }
    }

    fn sym(s: S) -> Self {
        Linear {
            terms: vec![(s, 1)],
            k: 0,
        }
    }

    fn constant(&self, what: &str) -> Result<i64, String> {
        if self.terms.is_empty() {
            Ok(self.k)
        } else {
            Err(format!("{what} needs constant operands, not addresses"))
        }
    }

    fn scale(mut self, by: i64) -> Result<Self, String> {
        for t in &mut self.terms {
            t.1 = t.1.checked_mul(by).ok_or("overflow")?;
        }
        self.terms.retain(|t| t.1 != 0);
        self.k = self.k.wrapping_mul(by);
        Ok(self)
    }

    fn add(mut self, other: Self, sign: i64) -> Result<Self, String> {
        self.k = self.k.wrapping_add(other.k.wrapping_mul(sign));
        for (s, c) in other.terms {
            let c = c.checked_mul(sign).ok_or("overflow")?;
            match self.terms.iter_mut().find(|t| t.0 == s) {
                Some(t) => t.1 = t.1.checked_add(c).ok_or("overflow")?,
                None => self.terms.push((s, c)),
            }
        }
        self.terms.retain(|t| t.1 != 0);
        Ok(self)
    }
}

/// Parses `text` as one expression. `local_ref(n, forward)` names the
/// numeric local label a `1b`/`1f` refers to.
pub fn parse_expr(text: &str, local_ref: &dyn Fn(u32, bool) -> String) -> Result<Expr, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
        local_ref,
        nodes: 0,
    };
    let e = p.sum()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("junk in expression: '{}'", text.trim()));
    }
    Ok(e)
}

/// A character that can start a symbol name.
pub fn is_sym_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'.' || c == b'$'
}

/// A character that can continue a symbol name.
pub fn is_sym_char(c: u8) -> bool {
    is_sym_start(c) || c.is_ascii_digit()
}

/// Most operators and parentheses one expression may hold. Evaluating
/// and dropping the tree recurse once per level, so this bounds the stack
/// they use; compiler output stays far below it.
const MAX_NODES: u32 = 256;

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    local_ref: &'a dyn Fn(u32, bool) -> String,
    /// Operators and parentheses so far.
    nodes: u32,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn node(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("expression has more than {MAX_NODES} operators"));
        }
        Ok(())
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(tok.as_bytes()) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.bits()?;
        loop {
            let op = match self.peek() {
                Some(b'+') => BinOp::Add,
                Some(b'-') => BinOp::Sub,
                _ => return Ok(e),
            };
            self.i += 1;
            self.node()?;
            e = Expr::Bin(op, Box::new(e), Box::new(self.bits()?));
        }
    }

    fn bits(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        loop {
            let op = match self.peek() {
                Some(b'&') => BinOp::And,
                Some(b'^') => BinOp::Xor,
                Some(b'|') => BinOp::Or,
                _ => return Ok(e),
            };
            self.i += 1;
            self.node()?;
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat("<<") {
                BinOp::Shl
            } else if self.eat(">>") {
                BinOp::Shr
            } else {
                match self.peek() {
                    Some(b'*') => BinOp::Mul,
                    Some(b'/') => BinOp::Div,
                    Some(b'%') => BinOp::Rem,
                    _ => return Ok(e),
                }
            };
            if matches!(op, BinOp::Mul | BinOp::Div | BinOp::Rem) {
                self.i += 1;
            }
            self.node()?;
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Some(b'-' | b'+' | b'~' | b'(')) {
            self.node()?;
        }
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            Some(b'~') => {
                self.i += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(b'(') => {
                self.i += 1;
                let e = self.sum()?;
                if !self.eat(")") {
                    return Err("missing ')' in expression".into());
                }
                Ok(e)
            }
            Some(b'\'') => {
                self.i += 1;
                let (c, used) = parse_escape(&self.s[self.i..])
                    .ok_or("character constant needs a character")?;
                self.i += used;
                Ok(Expr::Num(i64::from(c)))
            }
            Some(c) if c.is_ascii_digit() => self.number(),
            Some(c) if is_sym_start(c) => {
                let start = self.i;
                while self.i < self.s.len() && is_sym_char(self.s[self.i]) {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
                Ok(if name == "." {
                    Expr::Dot
                } else {
                    Expr::Sym(name.to_string())
                })
            }
            Some(c) => Err(format!("unexpected '{}' in expression", c as char)),
            None => Err("missing expression".into()),
        }
    }

    fn number(&mut self) -> Result<Expr, String> {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphanumeric() {
            self.i += 1;
        }
        let tok = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
        let lower = tok.to_ascii_lowercase();
        // `1b`/`1f`: the nearest numeric local label behind/ahead.
        if let Some(digits) = lower
            .strip_suffix('b')
            .map(|d| (d, false))
            .or(lower.strip_suffix('f').map(|d| (d, true)))
            .filter(|(d, _)| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
            .filter(|_| !lower.starts_with("0x") && !lower.starts_with("0b") || lower.len() == 2)
        {
            let n = digits.0.parse::<u32>().map_err(|e| e.to_string())?;
            return Ok(Expr::Sym((self.local_ref)(n, digits.1)));
        }
        let parsed = if let Some(h) = lower.strip_prefix("0x") {
            i64::from_str_radix(h, 16)
        } else if let Some(b) = lower.strip_prefix("0b") {
            i64::from_str_radix(b, 2)
        } else if lower.len() > 1 && lower.starts_with('0') {
            i64::from_str_radix(&lower[1..], 8)
        } else {
            lower.parse::<i64>()
        };
        let v = parsed.map_err(|_| format!("bad number '{tok}'"))?;
        // Values above 32 bits are kept as written; range checks happen
        // where the value is stored.
        Ok(Expr::Num(v))
    }
}

/// One character of a string or character constant, with GAS escapes:
/// `\b \f \n \r \t \\ \" \'`, octal `\NNN` and hex `\xHH`. Returns the
/// byte and how many input bytes it used.
pub fn parse_escape(s: &[u8]) -> Option<(u8, usize)> {
    let c = *s.first()?;
    if c != b'\\' {
        return Some((c, 1));
    }
    let e = *s.get(1)?;
    Some(match e {
        b'b' => (8, 2),
        b'f' => (12, 2),
        b'n' => (10, 2),
        b'r' => (13, 2),
        b't' => (9, 2),
        b'v' => (11, 2),
        b'0'..=b'7' => {
            let mut v: u32 = 0;
            let mut n = 1;
            while n < 4 && s.get(n).is_some_and(|d| (b'0'..=b'7').contains(d)) {
                v = v * 8 + u32::from(s[n] - b'0');
                n += 1;
            }
            (v as u8, n)
        }
        b'x' | b'X' => {
            let mut v: u32 = 0;
            let mut n = 2;
            while s.get(n).is_some_and(|d| d.is_ascii_hexdigit()) {
                v = (v << 4) | (s[n] as char).to_digit(16).unwrap_or(0);
                n += 1;
            }
            if n == 2 {
                return None;
            }
            (v as u8, n)
        }
        other => (other, 2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Expr {
        parse_expr(s, &|n, f| format!("L{n}{}", if f { "f" } else { "b" })).unwrap()
    }

    fn val(s: &str) -> i64 {
        p(s).const_value().unwrap()
    }

    #[test]
    fn numbers_and_precedence() {
        assert_eq!(val("0x10"), 16);
        assert_eq!(val("010"), 8);
        assert_eq!(val("0b101"), 5);
        assert_eq!(val("0"), 0);
        assert_eq!(val("-4"), -4);
        assert_eq!(val("1+2*3"), 7);
        assert_eq!(val("(1+2)*3"), 9);
        assert_eq!(val("1<<4+1"), 17);
        assert_eq!(val("~0&0xff"), 255);
        assert_eq!(val("'a"), 97);
        assert_eq!(val("'\\n"), 10);
        assert_eq!(val("-1>>40"), 0xFF_FFFF);
        assert_eq!(val("1<<63>>63"), 1);
    }

    #[test]
    fn hostile_expressions_are_errors() {
        let e = |s: &str| {
            parse_expr(s, &|_, _| String::new())
                .and_then(|e| e.eval(&mut |_: &str| Ok(0u8), None).map(|_| ()))
        };
        assert!(e("1<<64").is_err());
        assert!(e("1>>-1").is_err());
        assert!(e(&format!("{}1{}", "(".repeat(5000), ")".repeat(5000))).is_err());
        assert!(e(&"-".repeat(5000)).is_err());
        assert!(e(&vec!["1"; 5000].join("+")).is_err());
        assert!(e("x*0x7fffffffffffffff+x").is_err());
        assert!(e(&format!("{}1{}", "(".repeat(100), ")".repeat(100))).is_ok());
    }

    #[test]
    fn symbols_and_local_labels() {
        assert_eq!(p("reverse.isra.0"), Expr::Sym("reverse.isra.0".into()));
        assert_eq!(p("1b"), Expr::Sym("L1b".into()));
        assert_eq!(p("12f"), Expr::Sym("L12f".into()));
        assert_eq!(p("."), Expr::Dot);
        assert_eq!(p("sym+4").symbol_plus_const(), Some(("sym", 4)));
        assert_eq!(p("sym-4").symbol_plus_const(), Some(("sym", -4)));
        assert_eq!(p("a-b").symbol_plus_const(), None);
    }

    #[test]
    fn differences_cancel() {
        let mut ids = |s: &str| Ok(if s == "a" { 1 } else { 2 });
        let lin = p("a-b+a-a+4").eval(&mut ids, None).unwrap();
        assert_eq!(lin.terms, vec![(1, 1), (2, -1)]);
        assert_eq!(lin.k, 4);
        assert!(p("a*b").eval(&mut ids, None).is_err());
    }
}
