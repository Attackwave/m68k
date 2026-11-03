; Register-save prologues and epilogues — `movem` masks that look like data.
;
; `movem.l d0-d7/a0-a6,-(sp)` carries its register list as a 16-bit mask in
; the extension word, and the predecrement form stores the mask reversed,
; so the prologue's word ($fffe) and the epilogue's ($7fff) differ although
; they name the same registers. Both are instruction words, not table
; entries, and a listing must reassemble to the same masks.
;
; DATA $101a-$101d  one word table between the two routines, holding the
;                   very words a movem would use
	org	$1000

start:	bsr	heavy
	bsr	light
	rts

heavy:	movem.l	d0-d7/a0-a6,-(sp)
	lea	table(pc),a0
	move.w	(a0),d0
	movem.l	(sp)+,d0-d7/a0-a6
	rts

table:	dc.w	$fffe,$7fff

light:	movem.l	d2/a2,-(sp)
	moveq	#1,d2
	movem.l	(sp)+,d2/a2
	rts
