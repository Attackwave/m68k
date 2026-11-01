# m68k Assembly for VS Code

Motorola 68000-family assembly with Amiga support: editing, building and
source-level debugging in an emulator. The assembler, language server and
debug adapter ship with the extension; nothing else needs to be on `PATH`.

## Features

- **Language server**: diagnostics while typing, hover documentation,
  completion, go to definition, find references, rename, document symbols,
  formatting, signature help and semantic highlighting. `INCLUDE` files are
  resolved from the workspace root, its `include/` and `includes/`
  directories and `m68k.includePaths`.
- **One program, many files**: set `m68k.mainFile` for a program that is
  assembled as one unit from `INCLUDE`s. The server assembles that file and
  reports each included file's own errors on its own lines, with the
  symbols and macros of the whole program in scope; unsaved edits count.
  Without it, every file is checked on its own.
- **Cycle counts**: exact 68000 timing at the end of every instruction line,
  from the official execution time tables with the effective address of
  each operand, shown as `(12 cyc)`. Instructions that depend on runtime
  data show their range (`(10/8 cyc)` for a branch taken/not taken,
  `(38-70 cyc)` for MULU). Select lines to
  see their total in the status bar. Shown when `m68k.cpu` is `68000`; on an
  Amiga, chip RAM accesses can take longer while DMA is busy.
- **Custom chip registers**: hover any register name (`DMACON`, `BPLCON0`,
  `COLOR17`) or `$DFFxxx` address for its access, chip, chipset and bit
  fields. The value a line writes is decoded, including copper MOVE pairs
  in `dc.w` lists: `move.w #$8380,DMACON(a6)` shows
  `SET DMAEN|BPLEN|COPEN`. Reading a write-only register, read-modify-write
  on one, and writing a read-only register are flagged. Register names
  complete. Covers OCS, ECS and AGA.
- **Build tasks**: `m68k-asm` as a task (hunk executable by default), with
  errors and warnings in the Problems panel, including errors raised inside
  macro expansions.
- **Program analyses** (off by default): `m68k.stackCheck` reports a
  routine that returns or jumps on with the stack pointer off its entry
  level, or a line reached with different stack heights; `m68k.flagCheck`
  warns about a `TST`/`CMP`/`BTST` whose condition codes every following
  path overwrites before anything reads them. Both show up as you type and
  in build tasks. A task with `"stackReport": "stack.txt"` also writes the
  maximum stack depth of every routine, callees included.
- **Debugging** (F5): builds the active file with line debug info, starts
  the emulator, boots straight into the program and stops at breakpoints.
  - Breakpoints in source and in the disassembly view
  - Step over / into / out, by line or by instruction
  - Registers with decoded SR flags, memory view, hover values for
    registers and labels
  - Call stack recovered from return addresses on the stack
  - Disassembly with symbols and source lines
  - **Custom chips** scope: every custom register with its current value
    and decoded bit fields, including write-only ones such as `DMACON`,
    `BPLCON0` or `COP1LC` (the emulator reports the value last written)
  - **Copper list** scopes for `COP1LC` and `COP2LC`: the list decoded
    instruction by instruction (`MOVE COLOR00 = $0F00  R=15 G=0 B=0`,
    `WAIT v=$2C h=$06`, `END`), with MOVEs the copper cannot perform
    flagged

## Debugging setup

Debugging needs a UAE build with a GDB remote debugger. Stock FS-UAE and
WinUAE releases do not include one; use a build with remote debugger
support (FS-UAE started with `--remote_debugger`, or WinUAE with
`debugging_features=gdbserver`).

1. Set `m68k.emulator` to the emulator binary.
2. Optionally add a Kickstart and model via `m68k.emulatorArgs`, e.g.
   `["--amiga_model=A1200", "--kickstart_file=/path/to/kick.rom"]`.
   Without a Kickstart, FS-UAE boots its built-in replacement ROM.
3. Open an `.s` file and press F5.

The program's directory is mounted as `SYS:`. If it has no
`s/Startup-Sequence`, one is created that runs `SYS:<program>`; an existing
one is left untouched and must start the program as `SYS:<program>` for the
debugger to attach.

Without an emulator configured, F5 offers to attach to one that is already
running on port 2345.

## Settings

| Setting | Default | Purpose |
|---|---|---|
| `m68k.cpu` | `68000` | Target CPU for diagnostics and builds |
| `m68k.includePaths` | `[]` | Extra `INCLUDE` directories (`-I`), relative to the workspace folder |
| `m68k.mainFile` | | Main source of a program built from `INCLUDE`s; checked as one unit, built and debugged by F5 |
| `m68k.stackCheck` | `false` | Stack balance and reachability errors (`--stack-check`) |
| `m68k.flagCheck` | `false` | Warnings about condition codes nothing reads (`--flag-check`) |
| `m68k.toolsPath` | | Use tools from this directory instead of the bundled ones |
| `m68k.emulator` | | Emulator binary for debugging |
| `m68k.emulatorArgs` | `[]` | Extra emulator arguments |

## Notes

- Debug builds embed absolute source paths (line debug info). Plain build
  tasks leave it out; set `"linedebug": true` in a task to opt in.
- Hunk executables contain a relocation table. An address used in a field
  smaller than 32 bits (e.g. `move.w #label,d0`) cannot be relocated and is
  reported as an error.
- The call stack is a heuristic: a saved register that happens to hold a
  return address can show up as an extra frame.
