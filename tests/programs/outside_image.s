; Variables outside the image — absolute addresses that have no bytes.
;
; A program keeps its variables in memory that exists only at run time
; (a BSS area, chip RAM, a hardware register) and reaches them through
; absolute long addresses. Those addresses must stay numbers: nothing in
; the image is data, there is no byte to put a label on at $2000, and no
; in-image word may be mistaken for a pointer into that area.
;
; No data in this image.
	org	$1000

counter	equ	$2000
limit	equ	$2004

start:	clr.l	counter
	move.l	#10,limit
loop:	addq.l	#1,counter
	move.l	counter,d0
	cmp.l	limit,d0
	bne.s	loop
	rts
