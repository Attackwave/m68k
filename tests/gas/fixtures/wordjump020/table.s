| A switch table whose entries reach beyond 32 KB: GNU as puts a jump
| for each far target behind the table and points the entry at it.
	.text
	.globl	start
start:
	move.w .Ltab(%pc,%d0.w),%a0
	jmp %pc@(2,%a0:w)
	.balignw 2,0x284c
.Ltab:
	.word .Lnear-.Ltab
	.word .Lfar-.Ltab
	.word .Lfar2-.Ltab
	.word .Lfar-.Ltab
	.word .Lnear-.Ltab
.Lnear:
	moveq #1,%d0
	rts
	.zero 33000
.Lfar:
	moveq #2,%d0
	rts
.Lfar2:
	moveq #3,%d0
	rts
