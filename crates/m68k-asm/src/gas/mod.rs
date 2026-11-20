//! GNU as (GAS) syntax for the m68k, as GCC emits it with `-S`: several
//! files assembled and linked into one flat image with a table of the
//! longwords that hold image addresses.
//!
//! The image holds, in this order, every text section (`.text`,
//! `.text.*`), every read-only data section (`.rodata`, `.rodata.*`) and
//! every data section (`.data`, `.data.*`), each class in command-line
//! file order and, within a file, in the order GNU as creates the
//! sections. BSS (`.bss*`, local and global `.comm`) follows, not stored.
//! This is the layout of `ld` with
//!
//! ```text
//! .image : { *(.text .text.*) *(.rodata .rodata.*) *(.data .data.*) }
//! .bss   : { *(.bss .bss.*) *(COMMON) }
//! ```
//!
//! including its sharing of strings between `"aMS"` sections. The link
//! defines `__image_end` (the end of the stored image), `__bss_start`
//! (the same address) and `__bss_end`.
//!
//! `.reloc_table` places the fixup table at the very end of the image,
//! in the format of a TOS program: the first fixup's offset as a
//! longword (0: none), then one byte per further fixup with the distance
//! to it, 1 for a step of 254 without a fixup, and 0 at the end.
//!
//! Symbols are local to their file unless `.globl`; a name a file uses
//! but does not define refers to the global one.
//!
//! The CPU is the one GCC compiled for (`-m68020` with `--cpu 68020`):
//! from the 68020 on, `jbsr`/`jra`/`jXX` out of the word range or to
//! another file become 32-bit branches (`bsr.l`, `bra.l`, `bcc.l`) instead
//! of absolute jumps, and displacements wider than an index or `d16`
//! takes use the full-format extension word, as GNU as does.

mod expr;
mod link;
mod parse;

use std::fmt;

/// One source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

/// The linked program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GasImage {
    /// The stored image, from the origin to `__image_end`.
    pub image: Vec<u8>,
    /// Offsets in the image of every longword holding an image address.
    pub fixups: Vec<u32>,
    pub image_end: u32,
    pub bss_start: u32,
    pub bss_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GasError {
    pub file: String,
    /// 1-based; 0 when the error belongs to no single line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for GasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: error: {}", self.file, self.message)
        } else {
            write!(f, "{}:{}: error: {}", self.file, self.line, self.message)
        }
    }
}

/// Assembles and links `files` into one image at `origin`. Every
/// problem found is returned, each with its file and line.
pub fn assemble(files: &[SourceFile], cpu: &str, origin: u32) -> Result<GasImage, Vec<GasError>> {
    let fail = |message: String| {
        Err(vec![GasError {
            file: files.first().map(|f| f.name.clone()).unwrap_or_default(),
            line: 0,
            message,
        }])
    };
    if files.is_empty() {
        return fail("no input files".into());
    }
    // The CPU decides how far branches reach (absolute jumps on the
    // 68000/68010, 32-bit displacements from the 68020 on) and whether
    // full-format indices exist; an unknown name would silently pick one.
    if let Err(e) = m68k_core::cpu_gate::validate_cpu_name(cpu) {
        return fail(e);
    }
    let mut models = Vec::new();
    let mut errors = Vec::new();
    for file in files {
        let (model, errs) = parse::parse_file(&file.name, &file.text);
        errors.extend(errs.into_iter().map(|(line, message)| GasError {
            file: file.name.clone(),
            line,
            message,
        }));
        models.push(model);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    link::link(models, cpu, origin)
}
