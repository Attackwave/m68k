; Indexed jump table — the classic compiled `switch`.
;
; The dispatch target is computed at run time from a table, so nothing in
; any opword names it. A disassembler that does not recognize the table
; decodes its longwords as instructions, which is how `dc.l h0` becomes
; `ori.b #$0e,d0`.
;
; DATA $1c-$27   three-entry table (a 3-way switch is entirely ordinary,
;                which is why the pointer-run threshold matters here)
	org	$1000

start:	moveq	#2,d0
	lsl.w	#2,d0		; index * 4
	lea	jt(pc),a0
	movea.l	(a0,d0.w),a1	; target comes from memory
	jmp	(a1)		; ...so it is not in the opword

case0:	moveq	#0,d1
	rts
case1:	moveq	#1,d1
	rts
case2:	moveq	#2,d1
	rts

jt:	dc.l	case0
	dc.l	case1
	dc.l	case2
