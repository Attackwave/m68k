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
| `recursion.s` | A routine that calls itself, with a `link`/`unlk` frame — revisited addresses, and displacements that are frame offsets rather than addresses |
| `word_dispatch.s` | A table of 16-bit *offsets* (data) and a table of `bra` instructions (code), neither containing an address |
| `inline_data.s` | Arguments stored after the `jsr` that calls the routine, read over the return address |
| `self_modifying.s` | Code that patches its own immediate, and a byte loaded from inside an opword |

Each of the four was assembled against `vasmm68k_mot -Fbin -no-opt` and is
byte-identical to it, so the addresses named in their header comments are
measured rather than assumed.

## Findings

**1. A 3-way jump table was not recognized — fixed.** Two independent
causes, and the second was the worse one:

- `MIN_POINTER_TABLE` demanded four consecutive in-image pointers. Three
  is an entirely ordinary `switch` size. Now 3.
- The scan walked a **longword grid**, so it could only ever see tables
  whose distance from the origin is a multiple of four. Nothing makes that
  true — a table follows whatever code precedes it. `jump_table.s` puts
  one at origin+`$1a`, which was invisible at *any* threshold. The scan
  now walks a word grid.

Both were needed: fixing only the threshold would have left this file
still broken, which is exactly the kind of half-fix a ROM measurement
would have hidden.

**2. Text detection could swallow real code — half fixed.** In
`pc_relative_data.s` the bytes `4e75 4440 4e75` are `rts; neg.w d0; rts`
— all printable (`"NuD@Nu"`) — and the `dc.w` after them supplies the NUL
terminator the heuristic requires.

Fixed part: `clamp_to_traced` cuts a run where proven code resumes, which
could leave two bytes, and those were emitted as `dc.b "Nu"`. The minimum
length is now re-checked *after* clamping.

Not fixed, and not fixable this way: with only the program's own entry
point, nothing proves `negate` is code, and the run is a genuine
NUL-terminated printable sequence over the threshold. The honest remedy is
better reachability — an entry point, a PC trace — not a stricter string
test, which would start losing real strings instead.

**3. Pure code stays pure.** `nested_loops.s` yields zero `dc.*` lines.
Worth pinning down: the heuristics must not invent data in ordinary code,
and that direction of error is easy to introduce while fixing the others.

Kickstart 1.3 roundtrip is unchanged by both fixes (OK 80.7%, MISMATCH
424), while `--scan-tables` now recovers 348 more lines of code there.

**4. Recursion, offset tables and self-modifying code all come out
right.** Added with the second batch of programs, and worth recording
because each was a plausible place to fail:

- `recursion.s` — the recursive `bsr` resolves to the routine's own entry
  and the walk continues past it. A tracer that tracked "visited" per call
  rather than per address would loop or truncate here. Frame offsets stay
  displacements off `a6` instead of becoming labels below the origin.
- `word_dispatch.s` — the word-offset table is data and the `bra` table is
  code, in one image. These pull in opposite directions: a heuristic loose
  enough to call the offset table data would also swallow four real
  instructions in the branch table.
- `self_modifying.s` — the patched `moveq` is listed as assembled, not as
  patched. That is the only answer that reassembles, and the test exists so
  no later heuristic starts speculating about run-time values.

**5. Inline arguments after a `jsr` are still read as code — not fixed.**
In `inline_data.s` the callee reads its arguments over the return address
and resumes past them, so the words after the call site are data. Nothing
in any opword says so, and every other `jsr` in this corpus *is* followed
by code, so the linear walk decodes `dc.w 7 / dc.w 9` as `ori.b #$09,d7`.

The honest fix is for the tracer to model a callee adjusting its own
return address, not a stricter data heuristic — the bytes are
indistinguishable from code without that. Recorded as a pinned test so the
day it improves is visible.

## Adding a program

Keep each file focused on one construct and small enough to read whole.
State the intent in a header comment, including which addresses are data
and why — that comment is the specification the test checks against.
