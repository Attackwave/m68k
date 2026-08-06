; Nested loops and a movem prologue — the shape of ordinary compiled code.
;
; No data at all: every byte is an instruction. This is the control case
; for the heuristics — a disassembler that invents data here is wrong in
; the opposite direction from the string tests, and nothing but a
; whole-program test catches that.
;
; The backward branches also make a control-flow walk revisit addresses,
; so a tracer that does not terminate properly hangs on this file.
	org	$1000

start:	movem.l	d2-d7/a2-a6,-(sp)
	moveq	#7,d2		; outer counter
	moveq	#0,d0

.outer:	moveq	#15,d3		; inner counter
.inner:	addq.l	#1,d0
	cmpi.l	#100,d0
	bhi.s	.overflow
	dbra	d3,.inner
	dbra	d2,.outer
	bra.s	.done

.overflow:
	moveq	#-1,d0

.done:	movem.l	(sp)+,d2-d7/a2-a6
	rts
