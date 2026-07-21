# Motorola 68000 Family (m68k) Assembler & Disassembler

A robust, modular, and fast two-pass assembler and disassembler for the Motorola 68000 (m68k) family of processors, written in Rust. It supports instructions from the base 68000 up to the 68060, as well as the 68881/68882 Floating Point Unit (FPU) and MMU coprocessor instructions.

## Features

- **Two-Pass Architecture**:
  - **Pass 1**: Performs sequential decoding of instructions and automatically discovers local branch and jump targets to generate label names.
  - **Pass 2**: Formats the final assembly output, replacing absolute offsets with the discovered branch labels.
- **CPU limits**: Constrains decoding based on target CPU models: `68000`, `68010`, `68020`, `68030`, `68040`, `68060`.
- **FPU & MMU Support**: Complete decoding of coprocessor instructions, including complex floating-point instructions (`FMOVECR`, `FSINCOS`, transcendentals) and MMU structures (`PMOVE`, `PFLUSH`, `PTEST`).
- **Bitfield Manipulation**: Decodes 68020+ bitfield instructions (`BFEXTU`, `BFINS`, etc.).
- **Symbol Mapping**: Ability to feed a symbol file mapping absolute addresses to descriptive label names.
- **Flexible Formatting**: Custom flags for outputting addresses, raw instruction hex bytes, uppercase mnemonics, and output redirection.

## Building

```bash
cargo build --release
# binaries land in target/release/{m68k-asm,m68k-disasm,m68k-floppy}
```

## Disassembler (m68k-disasm)

```bash
m68k-disasm <binary-file> [options]
```

### Options

- `file`: Path to the input binary file (use `-` for standard input).
- `--org`/`--address`: Set origin base execution address (e.g. `0x1000`, `$1000`, or `4096`). Default is `0`.
- `-x`, `--hex`/`--raw`: Include raw instruction hexadecimal bytes in the output.
- `-a`, `--addr`: Include the absolute memory address column in the output.
- `-u`, `--upper`: Force formatting of mnemonics and operands in uppercase.
- `-s`, `--symbols`: Path to a custom symbol text file.
- `-c`, `--cpu`: Target CPU limit for decoding (`68000`, `68010`, `68020`, `68030`, `68040`, `68060`). Default is `68060`.
- `-out`, `--output`: Path to write the disassembly output (defaults to standard output).

### Symbol File Format

The custom symbol file should map absolute addresses to symbol names, one per line. Blank lines and lines starting with `#` or `;` are ignored.

```text
# custom_symbols.txt
$00001000 main_entry
$00002054 interrupt_handler
4500      my_data_offset
```

### Library usage

`m68k-disasm` is usable as a Rust library, not just a CLI binary — see the crate docs (`cargo doc --open -p m68k-disasm`) for a runnable example, starting with `m68k_disasm::disassembler::Disassembler`.

---

## Assembler (m68k-asm)

A complete two-pass assembler that matches the disassembler output, enabling full roundtrip (disassemble → assemble → byte-compare).

### CLI Usage

```bash
m68k-asm hello.s -o hello.bin [options]
```

Write a source file:

```assembly
; hello.s
    ORG $1000
START:
    MOVEQ   #1,D0
    MOVE.L  #MESSAGE,A0
    RTS
MESSAGE:
    DC.B    "Hello, 68000!",0
    EVEN
```

Assemble it to a raw binary:

```bash
m68k-asm hello.s -o hello.bin
# Assembled 3 instructions, ... bytes -> hello.bin
```

Other output formats via `-f`/`--format`:

```bash
m68k-asm hello.s -f srecord -o hello.srec     # Motorola S-Record
m68k-asm hello.s -f intel-hex -o hello.hex    # Intel Hex
m68k-asm hello.s -f elf -o hello.o            # ELF32 (EM_68K, ET_REL)
m68k-asm hello.s -f ieee695 -o hello.ieee     # IEEE-695
```

### Options

- `input`: Path to the input source file (use `-` for stdin).
- `-o`, `--output`: Path to output binary file (required).
- `-c`, `--cpu`: Target CPU model (`68000`, `68010`, `68020`, `68030`, `68040`, `68060`). Default is `68060`.
- `--origin <addr>` — default origin if the source has no `ORG` (hex with `$` or `0x` prefix, or decimal).
- `-f`, `--format`: Output format (`binary`, `srecord`, `intel-hex`, `elf`, `ieee695`). Default is `binary`.
- `-l`/`--listing <file>` — write an address/bytes/source listing.
- `--sym <file>` — export the resolved symbol table.
- `--map <file>` — export a memory map (address ranges per instruction).

`m68k-asm -f elf` output has been checked against `readelf -a` for
structural validity. `m68k-asm -f ieee695` output has been checked against
a self-built GNU binutils `ieee` BFD reader (`objdump -b ieee -m m68k`) —
see `crates/m68k-asm/src/ieee695.rs`'s module docs for details.

Both `-f elf` and `-f ieee695` emit one section per non-empty `SECTION` in
the source. The assembler resolves all symbols to absolute addresses during
assembly and does not emit relocation entries.

### Supported Directives

| Directive | Description |
|---|---|
| `org <addr>` | Set origin address |
| `equ <expr>` | Define constant |
| `dc.b/w/l` | Define byte/word/long data |
| `dc.s/d/x` | Define single/double/extended float |
| `dc.p` | Define packed decimal (raw hex) |
| `ds.b/w/l` | Define storage space |
| `even` | Align to word boundary |
| `align <n>` | Align to n-byte boundary |
| `include "file"` | Include external source file |
| `incbin "file"` | Include binary file |
| `macro` / `endm` | Define macros with arguments |
| `rept <count>` / `endm` | Repeat block |
| `section <name>` | Define named sections |

### Library usage

`m68k-asm` is usable as a Rust library, not just a CLI binary — see the crate docs (`cargo doc --open -p m68k-asm`) for runnable examples, starting with `m68k_asm::assembler::Assembler`.

---

## Floppy Disk Support

Read Amiga floppy disk images (ADF, IPF, UAE extended ADF) and disassemble directly:

```bash
m68k-disasm --bootblock --org 0xF80000 roms/game.adf
m68k-disasm --sector 0 0 0 --org 0xF80000 roms/game.adf
m68k-floppy --bootblock roms/game.adf > bootblock.bin
```

### Supported Formats

| Format | Backend | Description |
|---|---|---|
| Standard ADF | `adf` | 80 tracks, 2 sides, 11 sectors (DD) |
| Extended ADF | `adf` | Larger images with more sectors per track |
| UAE-0/1ADF | `uae` | Raw MFM bitstream per track (copy-protected disks) |
| IPF | `native` | Clean-room IPF parser with MFM bitstream decoding |

---

## Running Tests

```bash
cargo test --all --verbose
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Golden-vector tests (`crates/m68k-asm/tests/golden_*.rs`) check encoder,
decoder, float, expression, and token behavior against fixed reference
output in `tests/golden/vectors.json`.
