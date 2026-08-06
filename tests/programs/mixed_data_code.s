; Data of several widths interleaved with code, plus an odd-length string.
;
; Two things are pinned down here that whole-image tests otherwise miss:
;
;   - a byte string of odd length, so the following code starts on an odd
;     offset from the string's start. A data line that does not round up
;     to a word boundary leaves everything after it decoding at odd
;     addresses, which desynchronizes the rest of the listing.
;   - byte, word and longword data in one image, so a heuristic that
;     assumes one width mis-renders the others.
	org	$1000

start:	lea	bytes(pc),a0
	move.b	(a0),d0
	lea	words(pc),a1
	move.w	(a1),d1
	lea	longs(pc),a2
	move.l	(a2),d2
	rts

; Odd number of payload bytes before the terminator.
odd:	dc.b	"abcde",0
	even

check:	tst.l	d2
	beq.s	.zero
	moveq	#1,d0
	rts
.zero:	moveq	#0,d0
	rts

bytes:	dc.b	$01,$02,$03,$04
words:	dc.w	$1234,$5678
longs:	dc.l	$deadbeef
	dc.l	check
