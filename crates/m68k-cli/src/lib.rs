//! CLI binaries for m68k assembler/disassembler.
//!
//! This crate has no library API of its own — it is a thin home for three
//! `clap`-based binaries built on top of `m68k-asm`, `m68k-disasm`, and
//! `m68k-floppy`:
//!
//! - **`m68k-asm`** (`src/bin/m68k-asm.rs`) — assembles a source file.
//!   `-f`/`--format` selects the output format: `binary` (default),
//!   `srecord`, `intel-hex`, `elf`, or `ieee695` (see that format's own
//!   docs in `m68k_asm::ieee695` — unverified against a real reader).
//!   `--listing`/`-l`, `--sym`, `--map` write auxiliary output files.
//!   Example: `m68k-asm hello.s -f elf -o hello.o --cpu 68020`.
//! - **`m68k-disasm`** (`src/bin/m68k-disasm.rs`) — disassembles a raw
//!   binary file back to source-like text, with automatic label discovery
//!   for branch/jump targets (two-pass: scan then format).
//! - **`m68k-floppy`** (`src/bin/m68k-floppy.rs`) — Amiga floppy image
//!   inspection, built on the (currently unported — see
//!   `m68k_floppy`'s docs) `m68k-floppy` crate.
//!
//! Run any binary with `--help` for its full flag list.
