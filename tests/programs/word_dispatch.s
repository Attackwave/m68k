; A word-offset dispatch table, and a branch table of instructions.
;
; `jump_table.s` covers the longword-pointer form. This file covers the
; two variants that a pointer-table scan cannot see at all, because
; neither contains an address:
;
;   - a table of 16-bit *offsets* relative to the table itself, which is
;     how a compiler keeps a switch position-independent. The entries are
;     small integers, so nothing about them looks like a pointer.
;   - a table of fixed-size `bra` instructions jumped into by index, where
;     the dispatch targets are the branch opcodes themselves. Here the
;     table really is code, and a heuristic that widens "table" to "data"
;     destroys it.
;
; Both dispatches compute their destination at run time, so the only way
; to reach the case bodies is to recognize the table.
;
; DATA $102c-$1035   five word offsets (`offsets`)
; CODE $1042-$1051   the bra table (`brtab`) — instructions, not data
	org	$1000

; --- Variant 1: table of word offsets, added to the table's own address.
start:	moveq	#3,d0		; case index
	lea	offsets(pc),a0
	add.w	d0,d0		; index * 2
	move.w	(a0,d0.w),d1	; offset, not an address
	lea	offsets(pc),a1
	adda.w	d1,a1		; base + offset -> target
	jmp	(a1)

case0:	moveq	#10,d2
	bra.s	joined
case1:	moveq	#11,d2
	bra.s	joined
case2:	moveq	#12,d2
	bra.s	joined
case3:	moveq	#13,d2
	bra.s	joined
case4:	moveq	#14,d2

joined:	bsr.w	dispatch2
	rts

; Offsets are differences, so they stay correct wherever the code loads.
; A pointer scan sees five small integers and nothing else.
offsets:
	dc.w	case0-offsets
	dc.w	case1-offsets
	dc.w	case2-offsets
	dc.w	case3-offsets
	dc.w	case4-offsets

; --- Variant 2: index into a run of equal-sized branch instructions.
;
; Each entry is one `bra.w` (4 bytes), so entry n is at brtab+n*4. The
; table is executable: `jmp` lands *on* an instruction here.
dispatch2:
	moveq	#2,d0
	lsl.w	#2,d0		; index * 4 = entry size
	lea	brtab(pc),a0
	jmp	(a0,d0.w)	; jumps into the table itself

brtab:	bra.w	handler0
	bra.w	handler1
	bra.w	handler2
	bra.w	handler3

handler0:
	moveq	#0,d3
	rts
handler1:
	moveq	#1,d3
	rts
handler2:
	moveq	#2,d3
	rts
handler3:
	moveq	#3,d3
	rts
