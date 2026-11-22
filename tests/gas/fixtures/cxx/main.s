| C++ output in miniature: COMDAT groups, constructor and destructor
| arrays with priorities, wide strings, an undefined weak symbol.
	.text
	.globl	start
start:
	lea	__init_array_start,%a2
	lea	__init_array_end,%a3
	jbsr	twice
	jbsr	only_main
	tst.l	maybe
	rts

	.section	.text.twice,"axG",@progbits,twice,comdat
	.weak	twice
twice:
	moveq	#1,%d0
	rts

	.section	.rodata.vtbl,"aG",@progbits,vtbl,comdat
	.weak	vtbl
	.align	2
vtbl:
	.long	0
	.long	twice
	.long	__cxa_pure_virtual

	.section	.bss.counter,"awG",@nobits,counter,comdat
	.weak	counter
	.align	2
counter:
	.zero	4

	.section	.text.only_main,"ax",@progbits
only_main:
	lea	wide,%a0
	lea	half,%a1
	rts

	.section	.rodata.str4.4,"aMS",@progbits,4
	.align	2
wide:
	.string	"\0\0\0a\0\0\0b\0\0\0c\0\0\0"
wide_tail:
	.string	"\0\0\0c\0\0\0"
	.section	.rodata.str2.2,"aMS",@progbits,2
	.align	1
half:
	.string	"\0x\0y\0z\0"
half_tail:
	.string	"\0z\0"

	.section	.init_array,"aw"
	.align	2
	.long	start
	.section	.init_array.00200,"aw"
	.align	2
	.long	twice
	.section	.fini_array,"aw"
	.align	2
	.long	only_main

	.data
	.long	__fini_array_start,__fini_array_end
	.long	maybe

	.weak	maybe
	.weak	__cxa_pure_virtual
