; Code that patches itself, and a byte read from the middle of an opword.
;
; Every other file in this corpus keeps code and data in disjoint ranges.
; Here they overlap on purpose, which is the one case where "is this byte
; code or data?" has no single right answer — the same bytes are both:
;
;   - `patch` writes a new immediate into the `moveq` at `target`, so the
;     word at target+0 is an instruction *and* the destination of a store.
;     A disassembler cannot know the run-time value, and should render the
;     instruction as assembled rather than guess.
;   - `readback` loads a byte from inside an instruction it does not
;     execute, so an address in the middle of an opword is a legitimate
;     data reference. Nothing marks it: the `lea` looks like any other.
;
; The right output here is the *static* one — what the bytes say before
; anything runs. This file exists to pin that down, so that a future
; "smarter" heuristic cannot start speculating about patched values.
;
; CODE $1000-$102d   all of it, including the patched `moveq` at $102a
; DATA $102e-$1031   the saved original opword
	org	$1000

start:	bsr.w	patch
	bsr.w	readback
	bsr.w	target
	rts

; Overwrites the immediate byte of the `moveq` at `target`.
;
; A moveq's operand is the low byte of its single opword, so the store is
; to target+1 — an odd address inside an instruction.
; The store to `saved` goes through a register: PC-relative modes are not
; alterable on any 68k, so `move.w d0,saved(pc)` is not an instruction —
; only the load direction `saved(pc),d0` exists.
patch:	lea	target(pc),a0
	move.w	(a0),d0
	lea	saved(pc),a1
	move.w	d0,(a1)		; keep the original for comparison
	move.b	#42,1(a0)	; patch the immediate in place
	rts

; Reads the opcode byte of an instruction as data.
readback:
	lea	target(pc),a1
	move.b	(a1),d1		; the $70 of `moveq #n,d0`
	rts

; The instruction that gets patched. As assembled it loads 1; after
; `patch` runs it loads 42. A listing must show the assembled form.
target:	moveq	#1,d0
	rts

; Storage for the original opword, written by `patch` at run time. It is
; data, but nothing in the image ever reads it as a constant, so a
; heuristic that only trusts loads will not see it referenced.
saved:	dc.w	0
	dc.w	0
