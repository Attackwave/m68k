; Recursion through a stack frame — LINK/UNLK with locals.
;
; Every routine in the existing corpus is flat: one entry, one exit, no
; frame. This one calls itself, which changes what a control-flow walk
; sees. The recursive `bsr` targets an address *above* the current PC and
; already-visited addresses, so a tracer that marks "visited" per call
; rather than per address either loops or stops early.
;
; The frame matters for a second reason. `link a6,#-4` allocates a local,
; so the negative displacements in `-4(a6)` are frame offsets, not
; addresses — a disassembler that resolves displacements against the image
; invents a label somewhere below the origin for each one.
;
; DATA $104c-$104f   the seed value; everything before it is code.
	org	$1000

start:	move.l	seed(pc),d0
	bsr.w	fact
	rts

; fact(d0) -> d0, recursive.
;
; The guard branch is taken on the way down and falls through on the way
; back up, so both directions of the same address are exercised.
fact:	link	a6,#-4
	move.l	d0,-4(a6)	; frame offset, not an address
	cmpi.l	#1,d0
	bls.s	.base

	subq.l	#1,d0
	bsr.w	fact		; the call that revisits this routine
	move.l	-4(a6),d1
	bsr.w	mul32

	unlk	a6
	rts

.base:	moveq	#1,d0
	unlk	a6
	rts

; d0 * d1 -> d0, by shift-and-add, so the file needs no 68020 MULS.L.
mul32:	movem.l	d2-d3,-(sp)
	move.l	d0,d2
	moveq	#0,d0
.loop:	lsr.l	#1,d1
	bcc.s	.skip
	add.l	d2,d0
.skip:	lsl.l	#1,d2
	tst.l	d1
	bne.s	.loop
	movem.l	(sp)+,d2-d3
	rts

seed:	dc.l	5
