| A common of the same name as in asm.s, larger: one block of 8 bytes,
| 4-aligned.
	.comm	asm_common,8,4
	.data
	.long	asm_common+4
