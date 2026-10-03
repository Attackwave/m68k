//! Hardware asset conversion: PNG/Image to Amiga Planar/Interleaved Bitplanes and Copper Palettes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ConvertImageRequest {
    pub image_bytes: Vec<u8>,
    pub max_bitplanes: u8,
    pub interleaved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BitplaneConvertResponse {
    pub width: u32,
    pub height: u32,
    pub bitplanes_used: u8,
    pub palette_hex: Vec<String>,
    pub copper_palette_asm: String,
    pub bitplane_asm_dc: String,
    pub total_bytes: usize,
}

pub fn convert_image_to_bitplanes(
    req: ConvertImageRequest,
) -> Result<BitplaneConvertResponse, String> {
    let img = image::load_from_memory(&req.image_bytes)
        .map_err(|e| format!("Failed to decode image: {}", e))?
        .to_rgb8();

    let width = img.width();
    let height = img.height();

    // Collect unique colors (quantize down to max 32 colors / 12-bit Amiga RGB444)
    let mut palette: Vec<(u8, u8, u8)> = Vec::new();
    let max_colors = 1usize << req.max_bitplanes.min(5);

    // Map each pixel to color index
    let mut indexed_pixels = Vec::with_capacity((width * height) as usize);

    for pixel in img.pixels() {
        // Quantize to 4 bits per channel (Amiga OCS/ECS 4096 colors)
        let r = (pixel[0] >> 4) << 4;
        let g = (pixel[1] >> 4) << 4;
        let b = (pixel[2] >> 4) << 4;
        let color = (r, g, b);

        let color_idx = if let Some(idx) = palette.iter().position(|&c| c == color) {
            idx
        } else if palette.len() < max_colors {
            palette.push(color);
            palette.len() - 1
        } else {
            // Find closest color
            palette
                .iter()
                .enumerate()
                .min_by_key(|(_, (pr, pg, pb))| {
                    let dr = (r as i32) - (*pr as i32);
                    let dg = (g as i32) - (*pg as i32);
                    let db = (b as i32) - (*pb as i32);
                    dr * dr + dg * dg + db * db
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        };

        indexed_pixels.push(color_idx);
    }

    // Determine actual planes needed
    let num_colors = palette.len().max(2);
    let mut bitplanes_needed = 1u8;
    while (1usize << bitplanes_needed) < num_colors && bitplanes_needed < req.max_bitplanes.min(5) {
        bitplanes_needed += 1;
    }

    // Format Copper palette assembly
    let mut copper_lines = Vec::new();
    let mut palette_hex_strings = Vec::new();

    for (i, &(r, g, b)) in palette.iter().enumerate() {
        let r4 = (r >> 4) as u16;
        let g4 = (g >> 4) as u16;
        let b4 = (b >> 4) as u16;
        let rgb12 = (r4 << 8) | (g4 << 4) | b4;
        let reg_offset = 0x0180 + (i as u16 * 2);

        palette_hex_strings.push(format!("#{:01X}{:01X}{:01X}", r4, g4, b4));
        copper_lines.push(format!(
            "    dc.w    ${:04X}, ${:04X} ; COLOR{:02}",
            reg_offset, rgb12, i
        ));
    }

    let copper_palette_asm = copper_lines.join("\n");

    // Convert pixel grid to planar bytes
    let row_bytes = width.div_ceil(16) * 2; // Word-aligned row width in bytes
    let total_bytes = (row_bytes * height) as usize * bitplanes_needed as usize;

    let mut raw_data = Vec::with_capacity(total_bytes);

    if req.interleaved {
        // Interleaved: For each scanline, emit Plane 0, Plane 1, ...
        for y in 0..height {
            for plane in 0..bitplanes_needed {
                for x_word in 0..(row_bytes / 2) {
                    let mut word = 0u16;
                    for bit in 0..16 {
                        let x = x_word * 16 + bit;
                        if x < width {
                            let idx = indexed_pixels[(y * width + x) as usize];
                            if (idx & (1 << plane)) != 0 {
                                word |= 1 << (15 - bit);
                            }
                        }
                    }
                    raw_data.extend_from_slice(&word.to_be_bytes());
                }
            }
        }
    } else {
        // Planar: All scanlines of Plane 0, then Plane 1, ...
        for plane in 0..bitplanes_needed {
            for y in 0..height {
                for x_word in 0..(row_bytes / 2) {
                    let mut word = 0u16;
                    for bit in 0..16 {
                        let x = x_word * 16 + bit;
                        if x < width {
                            let idx = indexed_pixels[(y * width + x) as usize];
                            if (idx & (1 << plane)) != 0 {
                                word |= 1 << (15 - bit);
                            }
                        }
                    }
                    raw_data.extend_from_slice(&word.to_be_bytes());
                }
            }
        }
    }

    // Generate DC.W assembly statements
    let mut dc_lines = Vec::new();
    for chunk in raw_data.chunks(16) {
        let words = chunk
            .chunks(2)
            .map(|w| {
                if w.len() == 2 {
                    format!("${:02X}{:02X}", w[0], w[1])
                } else {
                    format!("${:02X}00", w[0])
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        dc_lines.push(format!("    dc.w    {}", words));
    }

    let bitplane_asm_dc = dc_lines.join("\n");

    Ok(BitplaneConvertResponse {
        width,
        height,
        bitplanes_used: bitplanes_needed,
        palette_hex: palette_hex_strings,
        copper_palette_asm,
        bitplane_asm_dc,
        total_bytes: raw_data.len(),
    })
}
