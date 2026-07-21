//! Utility functions for the m68k assembler.

/// Parse a number from string, supporting multiple formats:
/// - `$FF` or `0xFF` for hex
/// - `%1010` for binary
/// - `42` for decimal
pub fn parse_number(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('$') {
        i64::from_str_radix(stripped, 16).map_err(|e| format!("invalid hex: {}", e))
    } else if let Some(stripped) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(stripped, 16).map_err(|e| format!("invalid hex: {}", e))
    } else if let Some(stripped) = s.strip_prefix('%') {
        i64::from_str_radix(stripped, 2).map_err(|e| format!("invalid binary: {}", e))
    } else if s.len() == 2 && s.starts_with('\'') {
        Ok(s.as_bytes()[1] as i64)
    } else {
        s.parse::<i64>()
            .map_err(|e| format!("invalid number: {}", e))
    }
}

/// Sign-extend an 8-bit integer to i32.
#[inline]
pub fn sign_extend_8(val: u8) -> i32 {
    val as i8 as i32
}

/// Sign-extend a 16-bit integer to i32.
#[inline]
pub fn sign_extend_16(val: u16) -> i32 {
    val as i16 as i32
}

/// Mask a value to 32 bits.
#[inline]
pub fn mask32(val: i64) -> u32 {
    (val & 0xFFFFFFFF) as u32
}

/// Convert a 32-bit unsigned value to a signed i32.
#[inline]
pub fn to_signed32(val: u32) -> i32 {
    val as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number_decimal() {
        assert_eq!(parse_number("42").unwrap(), 42);
        assert_eq!(parse_number("0").unwrap(), 0);
        assert_eq!(parse_number("-1").unwrap(), -1);
    }

    #[test]
    fn test_parse_number_hex_dollar() {
        assert_eq!(parse_number("$FF").unwrap(), 0xFF);
        assert_eq!(parse_number("$1234").unwrap(), 0x1234);
        assert_eq!(parse_number("$0").unwrap(), 0);
    }

    #[test]
    fn test_parse_number_hex_0x() {
        assert_eq!(parse_number("0xFF").unwrap(), 0xFF);
        assert_eq!(parse_number("0XFF").unwrap(), 0xFF);
        assert_eq!(parse_number("0x1234").unwrap(), 0x1234);
    }

    #[test]
    fn test_parse_number_binary() {
        assert_eq!(parse_number("%1010").unwrap(), 10);
        assert_eq!(parse_number("%1111").unwrap(), 15);
        assert_eq!(parse_number("%0").unwrap(), 0);
    }

    #[test]
    fn test_parse_number_invalid() {
        assert!(parse_number("$GG").is_err());
        assert!(parse_number("%2").is_err());
        assert!(parse_number("abc").is_err());
    }

    #[test]
    fn test_sign_extend_8() {
        assert_eq!(sign_extend_8(0x7F), 0x7F);
        assert_eq!(sign_extend_8(0x80), -128);
        assert_eq!(sign_extend_8(0xFF), -1);
    }

    #[test]
    fn test_sign_extend_16() {
        assert_eq!(sign_extend_16(0x7FFF), 0x7FFF);
        assert_eq!(sign_extend_16(0x8000), -32768);
        assert_eq!(sign_extend_16(0xFFFF), -1);
    }

    #[test]
    fn test_mask32() {
        assert_eq!(mask32(0x1_0000_0000), 0);
        assert_eq!(mask32(0x1_0000_1234), 0x1234);
        assert_eq!(mask32(-1), 0xFFFF_FFFF);
    }

    #[test]
    fn test_to_signed32() {
        assert_eq!(to_signed32(0x7FFF_FFFF), 0x7FFF_FFFF);
        assert_eq!(to_signed32(0x8000_0000), -2147483648);
        assert_eq!(to_signed32(0xFFFF_FFFF), -1);
    }
}
