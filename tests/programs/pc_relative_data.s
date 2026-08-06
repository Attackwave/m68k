; PC-relative data access — how position-independent Amiga code reaches
; its own tables.
;
; The interesting property for a disassembler is that `lea table(pc),a0`
; reports a target address just as a branch does, but that address is
; *data*, not code. Following it as a branch target seeds the walk with
; addresses that are not instructions.
	org	$1000

start:	lea	values(pc),a0	; data, not a branch target
	moveq	#3,d1
	moveq	#0,d0
.sum:	add.w	(a0)+,d0
	dbra	d1,.sum

	lea	handlers(pc),a1
	movea.l	(a1),a2
	jsr	(a2)
	rts

double:	add.w	d0,d0
	rts
negate:	neg.w	d0
	rts

; Word data, deliberately not longword-aligned pointers, so a
; pointer-table heuristic must not claim it.
values:	dc.w	1,2,3,4

; Real pointers, which a heuristic may claim.
handlers:
	dc.l	double
	dc.l	negate
