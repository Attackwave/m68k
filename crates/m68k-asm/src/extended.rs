//! Decimal literal to MC68881 extended precision, correctly rounded.
//!
//! Going through `f64` kept 53 of the 64 mantissa bits and limited the
//! range to about 1e308. Here the literal is read as an exact fraction of
//! two big integers, `digits * 10^exp`, and the 64-bit mantissa is cut from
//! its binary expansion with round-to-nearest-even.

/// Unsigned big integer, little-endian 32-bit limbs, no leading zero limbs.
#[derive(Clone, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from_u32(v: u32) -> Self {
        let mut b = Big(vec![v]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn bits(&self) -> u64 {
        self.0.last().map_or(0, |top| {
            (self.0.len() as u64 - 1) * 32 + u64::from(32 - top.leading_zeros())
        })
    }

    fn mul_add(&mut self, mul: u32, add: u32) {
        let mut carry = u64::from(add);
        for limb in &mut self.0 {
            let v = u64::from(*limb) * u64::from(mul) + carry;
            *limb = v as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
        self.trim();
    }

    fn shl(&mut self, n: u64) {
        if self.is_zero() {
            return;
        }
        let (limbs, bits) = ((n / 32) as usize, (n % 32) as u32);
        if bits != 0 {
            let mut carry = 0;
            for limb in &mut self.0 {
                let v = *limb;
                *limb = v << bits | carry;
                carry = v >> (32 - bits);
            }
            if carry != 0 {
                self.0.push(carry);
            }
        }
        self.0.splice(0..0, std::iter::repeat_n(0, limbs));
    }

    /// `self -= other`, requires `self >= other`.
    fn sub(&mut self, other: &Big) {
        let mut borrow = 0i64;
        for (i, limb) in self.0.iter_mut().enumerate() {
            let v = i64::from(*limb) - i64::from(other.0.get(i).copied().unwrap_or(0)) - borrow;
            *limb = v as u32;
            borrow = i64::from(v < 0);
        }
        self.trim();
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Big {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.iter().rev().cmp(other.0.iter().rev()))
    }
}

/// Largest decimal exponent accepted; beyond it every value overflows or
/// underflows anyway, and it bounds the size of `10^exp`.
const MAX_DECIMAL_EXPONENT: i64 = 6000;

/// Most digits read. Converting costs time quadratic in the digit count;
/// this is far beyond what any constant needs to round correctly in
/// practice.
const MAX_DIGITS: usize = 20_000;

const BIAS: i64 = 16383;

/// The six words of a `DC.X` slot for a decimal literal such as `0.1`,
/// `-3.14159e-2` or `12`. `None` when `text` is not a decimal literal
/// (`inf`, `nan`); an error when it is out of the extended range.
pub(crate) fn decimal_to_extended(text: &str) -> Option<Result<[u16; 6], String>> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = || int_part.bytes().chain(frac_part.bytes());
    if int_part.len() + frac_part.len() == 0 || !digits().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut exp10: i64 = match exponent {
        None => 0,
        Some(e) => {
            let body = e.strip_prefix(['+', '-']).unwrap_or(e);
            if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            // Clamp rather than overflow; the clamped value is out of
            // range either way.
            let magnitude = body.parse::<i64>().unwrap_or(i64::MAX).min(1 << 40);
            if e.starts_with('-') {
                -magnitude
            } else {
                magnitude
            }
        }
    };
    if int_part.len() + frac_part.len() > MAX_DIGITS {
        return Some(Err(format!(
            "{} digits, at most {MAX_DIGITS} are supported",
            int_part.len() + frac_part.len()
        )));
    }
    exp10 -= frac_part.len() as i64;
    let sign = if negative { 0x8000 } else { 0 };
    let mut num = Big(Vec::new());
    for d in digits() {
        num.mul_add(10, u32::from(d - b'0'));
    }
    if num.is_zero() {
        return Some(Ok([sign, 0, 0, 0, 0, 0]));
    }
    // Leading digits beyond what the exponent can absorb do not matter
    // for the range check below; only the order of magnitude does.
    let magnitude = exp10 + num.bits() as i64 * 3 / 10;
    if magnitude.abs() > MAX_DECIMAL_EXPONENT {
        return Some(Err(out_of_range(text)));
    }
    let mut den = Big::from_u32(1);
    for _ in 0..exp10.unsigned_abs() {
        if exp10 > 0 {
            num.mul_add(10, 0);
        } else {
            den.mul_add(10, 0);
        }
    }

    // Align so that 1 <= num/den < 2; the value is then num/den * 2^exp2.
    let mut exp2 = num.bits() as i64 - den.bits() as i64;
    if exp2 >= 0 {
        den.shl(exp2 as u64);
    } else {
        num.shl(exp2.unsigned_abs());
    }
    if num < den {
        num.shl(1);
        exp2 -= 1;
    }
    // 64 mantissa bits, then the rounding bit; the rest is the sticky bit.
    let mut mantissa = 0u64;
    let mut round = false;
    for i in 0..65 {
        let bit = num >= den;
        if bit {
            num.sub(&den);
        }
        num.shl(1);
        if i < 64 {
            mantissa = mantissa << 1 | u64::from(bit);
        } else {
            round = bit;
        }
    }
    let sticky = !num.is_zero();
    if round && (sticky || mantissa & 1 == 1) {
        mantissa = mantissa.wrapping_add(1);
        if mantissa == 0 {
            mantissa = 1 << 63;
            exp2 += 1;
        }
    }
    let biased = exp2 + BIAS;
    // ponytail: no denormals; a literal below 2^-16382 is rejected.
    if !(1..0x7FFF).contains(&biased) {
        return Some(Err(out_of_range(text)));
    }
    Some(Ok([
        sign | biased as u16,
        0,
        (mantissa >> 48) as u16,
        (mantissa >> 32) as u16,
        (mantissa >> 16) as u16,
        mantissa as u16,
    ]))
}

fn out_of_range(text: &str) -> String {
    format!("{text} is outside the extended precision range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(text: &str) -> [u16; 6] {
        decimal_to_extended(text).unwrap().unwrap()
    }

    #[test]
    fn exact_values() {
        assert_eq!(x("1.0"), [0x3FFF, 0, 0x8000, 0, 0, 0]);
        assert_eq!(x("-1"), [0xBFFF, 0, 0x8000, 0, 0, 0]);
        assert_eq!(x("1.5"), [0x3FFF, 0, 0xC000, 0, 0, 0]);
        assert_eq!(x("0.375"), [0x3FFD, 0, 0xC000, 0, 0, 0]);
        assert_eq!(x("-0.0"), [0x8000, 0, 0, 0, 0, 0]);
    }

    /// Values a double cannot hold to 64 bits, against the reference
    /// extended encodings.
    #[test]
    fn rounds_to_64_mantissa_bits() {
        assert_eq!(x("0.1"), [0x3FFB, 0, 0xCCCC, 0xCCCC, 0xCCCC, 0xCCCD]);
        assert_eq!(
            x("3.14159265358979323846264338327950288"),
            [0x4000, 0, 0xC90F, 0xDAA2, 0x2168, 0xC235]
        );
        assert_eq!(
            x("2.71828182845904523536028747135266250"),
            [0x4000, 0, 0xADF8, 0x5458, 0xA2BB, 0x4A9B]
        );
        // 2^64 + 1 is a tie at 64 bits and rounds to even.
        assert_eq!(x("18446744073709551617"), [0x403F, 0, 0x8000, 0, 0, 0]);
        // 2^64 + 3 is a tie that rounds up.
        assert_eq!(x("18446744073709551619"), [0x403F, 0, 0x8000, 0, 0, 0x0002]);
    }

    #[test]
    fn range_beyond_double() {
        assert_eq!(x("1e4000"), [0x73E6, 0, 0xD1BA, 0x8323, 0xFE55, 0x8C61]);
        assert_eq!(x("1e-4000"), [0x0C17, 0, 0x9C3D, 0x7386, 0x4F38, 0x05C0]);
        assert!(decimal_to_extended("1e5000").unwrap().is_err());
        assert!(decimal_to_extended("1e-5000").unwrap().is_err());
        assert!(decimal_to_extended("1e999999999999").unwrap().is_err());
    }

    #[test]
    fn digit_count_is_bounded() {
        let long = "1".repeat(MAX_DIGITS + 1);
        assert!(decimal_to_extended(&long).unwrap().is_err());
    }

    #[test]
    fn non_decimal_text_is_left_to_the_caller() {
        for text in ["inf", "-nan", "1e", "1.2.3", "", "."] {
            assert!(decimal_to_extended(text).is_none(), "{text}");
        }
    }
}
