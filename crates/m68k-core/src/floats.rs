//! 80-bit extended precision floating-point support.
//!
//! The m68k FPU uses IEEE 754 80-bit extended precision format.
//! This module provides basic conversion and encoding support.

/// An 80-bit extended precision float stored as raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendedFloat {
    pub bytes: [u8; 10],
}

impl ExtendedFloat {
    pub fn from_bytes(bytes: [u8; 10]) -> Self {
        Self { bytes }
    }

    pub fn to_bytes(&self) -> &[u8; 10] {
        &self.bytes
    }

    pub fn from_f64(val: f64) -> Self {
        let bits = val.to_bits();
        let sign = (bits >> 63) as u8;
        let exp64 = ((bits >> 52) & 0x7FF) as i64;
        let frac64 = bits & 0x000F_FFFF_FFFF_FFFF;

        let mut bytes = [0u8; 10];

        if exp64 == 0 && frac64 == 0 {
            // Zero
            bytes[0] = sign << 7;
        } else if exp64 == 0x7FF {
            // Inf or NaN
            bytes[0] = sign << 7 | 0x7F;
            bytes[1] = 0xFF;
            if frac64 != 0 {
                bytes[2] = 0x80;
            }
        } else {
            // Normal number: convert from double to extended
            let exp80 = exp64 - 1023 + 16383;
            bytes[0] = sign << 7 | ((exp80 >> 8) & 0x7F) as u8;
            bytes[1] = (exp80 & 0xFF) as u8;
            bytes[2] = 0x80 | ((frac64 >> 56) & 0x7F) as u8;
            bytes[3] = ((frac64 >> 48) & 0xFF) as u8;
            bytes[4] = ((frac64 >> 40) & 0xFF) as u8;
            bytes[5] = ((frac64 >> 32) & 0xFF) as u8;
            bytes[6] = ((frac64 >> 24) & 0xFF) as u8;
            bytes[7] = ((frac64 >> 16) & 0xFF) as u8;
            bytes[8] = ((frac64 >> 8) & 0xFF) as u8;
            bytes[9] = (frac64 & 0xFF) as u8;
        }

        Self { bytes }
    }

    pub fn to_f64(&self) -> f64 {
        let sign = (self.bytes[0] >> 7) & 1;
        let exp = (((self.bytes[0] & 0x7F) as i64) << 8) | self.bytes[1] as i64;
        let frac_hi = self.bytes[2] as u64;
        let frac_rest = ((self.bytes[3] as u64) << 40)
            | ((self.bytes[4] as u64) << 32)
            | ((self.bytes[5] as u64) << 24)
            | ((self.bytes[6] as u64) << 16)
            | ((self.bytes[7] as u64) << 8)
            | self.bytes[8] as u64;

        if exp == 0 && frac_hi == 0 && frac_rest == 0 {
            return if sign == 1 { -0.0 } else { 0.0 };
        }

        if exp == 0x7FFF {
            if frac_hi == 0x80 && frac_rest == 0 {
                return if sign == 1 {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
            } else {
                return f64::NAN;
            }
        }

        let exp64 = exp - 16383 + 1023;
        let frac64 = ((frac_hi & 0x7F) << 56) | frac_rest;

        let bits = ((sign as u64) << 63) | ((exp64 as u64) << 52) | (frac64 >> 11);
        f64::from_bits(bits)
    }
}

/// Parse a floating-point number from a string.
pub fn parse_float(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .map_err(|e| format!("invalid float: {}", e))
}
