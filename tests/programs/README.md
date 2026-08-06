# Whole-program test corpus

Small, complete m68k programs whose source we control, so the *truth* of
every byte is known: which addresses are instructions, which are data, and
what each pointer points at.

This is what the other suites cannot check. `instruction_coverage.rs` and
`tests/data/all_forms.s` verify instruction *forms* — assemble one
instruction, decode it, reassemble it. They say nothing about whether the
disassembler can tell a jump table from arithmetic, or a copyright string
from code, because a single instruction has no surroundings to get wrong.

Every finding below came from running these files, and none of them was
visible in ROM measurements: on a ROM nobody knows which answer is right.

## The programs

| File | Construct it isolates |
|---|---|
| `jump_table.s` | Indexed dispatch (`jmp (a1)` with the target from a table) — a compiled `switch` |
| `strings_between_code.s` | NUL-terminated text between routines, the Kickstart layout |
| `pc_relative_data.s` | `lea table(pc),a0` — reports a target address like a branch, but names *data* |
| `nested_loops.s` | Pure code, no data at all — the control case against inventing data |
| `mixed_data_code.s` | Byte/word/long data interleaved with code, plus an odd-length string |

## Findings

Run against `m68k-disasm` at the time of writing (`--entry $1000`):

**1. A 3-way jump table is not recognized.** `MIN_POINTER_TABLE` requires
four consecutive in-image pointers; `jump_table.s` has three, which is an
entirely ordinary `switch` size. Its table renders as `ori.b #$0e,d0`.
Lowering the threshold to 3 takes the Kickstart 1.3 count from 10 tables /
87 targets to 24 / 129 — whether those extra ones are real cannot be
decided on a ROM, but can be decided here.

**2. Text detection can swallow real code.** In `pc_relative_data.s` the
bytes `4e75 4440 4e75` are `rts; neg.w d0; rts` — all printable
(`"NuD@Nu"`) — and the `dc.w 1,2,3,4` that follows supplies the NUL
terminator the heuristic requires. Two real instructions are lost.

The cause is reachability, not the heuristic itself: `negate` is reached
only through the pointer table, so the walk never claims it, and unclaimed
bytes are what the text test is allowed to judge. Recognizing the table
would fix this too.

**3. Pure code stays pure.** `nested_loops.s` yields zero `dc.*` lines.
Worth pinning down: the heuristics must not invent data in ordinary code,
and that direction of error is easy to introduce while fixing the others.

## Adding a program

Keep each file focused on one construct and small enough to read whole.
State the intent in a header comment, including which addresses are data
and why — that comment is the specification the test checks against.
