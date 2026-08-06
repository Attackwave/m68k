; Strings sitting between routines — the Kickstart pattern.
;
; A linear scan decodes the message text as instructions, inventing both
; opcodes and branch targets; the invented targets then pull further wrong
; labels in behind them. This is the case the text heuristic exists for.
;
; Every string here is NUL-terminated, as string constants are in
; practice, and each is followed by code again so a run that overshoots is
; visible as a swallowed instruction rather than as trailing noise.
	org	$1000

start:	lea	greeting(pc),a0
	bsr.w	strlen
	lea	failure(pc),a0
	bsr.w	strlen
	rts

; A string immediately after an rts, with no padding: the tightest case
; for deciding where code ends.
greeting:
	dc.b	"Hello, Amiga!",0
	even

strlen:	moveq	#0,d0
.loop:	tst.b	(a0)+
	beq.s	.done
	addq.l	#1,d0
	bra.s	.loop
.done:	rts

; Text containing bytes that decode as plausible instructions on their
; own — `Nu` is $4E75, which is `rts`.
failure:
	dc.b	"Nuisance: %ld errors",0
	even

	rts
