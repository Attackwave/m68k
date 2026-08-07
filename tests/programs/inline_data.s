; Data embedded in the instruction stream, reached over the return address.
;
; The routine's arguments sit *after* the `jsr` that calls it: the callee
; reads them through the return address on the stack, then advances that
; address past them before returning. This is a real Amiga/Atari idiom
; (and how `Trap #x` argument blocks work), and it is the hardest case for
; a control-flow walk, because:
;
;   - the bytes right after a `jsr` are data, though every other `jsr` in
;     every other file is followed by code. A tracer that continues
;     straight through the call site decodes the arguments as
;     instructions.
;   - execution resumes at an address that appears nowhere in any opword
;     — it is computed from the return address at run time.
;
; The last routine also ends the image with no `rts` after its data, so
; the final bytes are data with nothing following them: a run that
; extends past the end must be clamped rather than read off the end.
;
; DATA $100a-$100d   two words after the first jsr
; DATA $1016-$101d   four words after the second jsr
; DATA $1044-$1047   the buffer (ds.w 2)
; DATA $1048-$104f   trailing constants, to the end of the image
	org	$1000

start:	lea	buffer(pc),a0

	jsr	addwords
	dc.w	7		; argument, not an instruction
	dc.w	9		; argument, not an instruction

	move.w	d0,(a0)+

	jsr	addwords
	dc.w	100
	dc.w	200
	dc.w	0		; padding argument
	dc.w	0

	move.w	d0,(a0)+
	bsr.w	finish
	rts

; Reads two inline words over the return address, sums them into d0, and
; returns past them.
;
; `move.l (sp),a1` is the return address — the address of the first
; argument word. Adding 4 to it before writing it back is what makes the
; caller resume after the data.
addwords:
	movem.l	a1,-(sp)
	move.l	4(sp),a1	; return address = &args
	move.w	(a1)+,d0
	add.w	(a1)+,d0
	move.l	a1,4(sp)	; resume after the arguments
	movem.l	(sp)+,a1
	rts

; Ends with data and no trailing instruction, so the image's last byte is
; a constant.
finish:	lea	constants(pc),a1
	move.l	(a1),d1
	rts

buffer:	ds.w	2

constants:
	dc.l	$cafebabe
	dc.l	$0badf00d
