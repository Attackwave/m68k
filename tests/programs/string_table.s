; A table of pointers to strings — a pointer run whose targets are text.
;
; `--scan-tables` seeds entry points from runs of in-image longword
; pointers, because that is what a jump table looks like. The three
; longwords here are such a run, but they point at NUL-terminated strings:
; tracing their targets as code would turn "first" into `bcs.w` and the
; strings would vanish from the listing. The index comes from a register,
; so nothing in the code names a string directly either.
;
; DATA $101a-$1025  the pointer table
; DATA $1026-$1037  the strings "first", "second", "third" (NUL-terminated)
	org	$1000

start:	moveq	#1,d0
	lsl.w	#2,d0
	lea	names(pc),a0
	movea.l	(a0,d0.w),a0	; a0 -> the string
	bsr.s	strlen
	rts

strlen:	moveq	#-1,d0
.next:	addq.l	#1,d0
	tst.b	(a0)+
	bne.s	.next
	rts

names:	dc.l	first,second,third
first:	dc.b	'first',0
second:	dc.b	'second',0
third:	dc.b	'third',0
	even
