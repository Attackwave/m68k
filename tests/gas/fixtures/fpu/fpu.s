| FPU code as GCC emits it with -m68020/-m68040: floating-point
| immediates as bit patterns, register lists, control registers, FPU
| branches near, far and to another file.
	.text
	.globl	start
start:
	fmove.s #0x420c0000,%fp0
	fmove.s #0x0,%fp1
	fcmp.d #0x3f847ae147ae147b,%fp0
	fmove.d #0xbff0000000000000,%fp2
	fmove.x #0x3fff00008000000000000000,%fp3
	fmove.l #5,%fp0
	fmove.w #-3,%fp0
	fsub.s #0x3f800000,%fp0
	fmove.s %fp0,(%a0)
	fmove.d %fp0,-(%sp)
	fmove.d (%sp)+,%fp1
	fmove.x %fp0,%fp1
	fmove.l %fp0,%d3
	fmove.s value,%fp0
	fmove.x %fp0,value
	fmovecr #0xf,%fp0
	fintrz.x %fp0,%fp0
	fabs.x %fp0
	fsabs.x %fp0,%fp1
	fdneg.d (%a0),%fp1
	fsmove.l %d2,%fp0
	fsglmul.l %d1,%fp0
	ftst.x %fp3
	ftst.s %d5
	ftst.d 8(%a0)
	fmovem.x %fp2-%fp3,-(%sp)
	fmovem.x (%sp)+,%fp2-%fp3
	fmovem.x %fp2/%fp4/%fp7,-(%sp)
	fmovem.x #12,-(%sp)
	fmovem.x (%sp)+,#48
	fmove.l %fpcr,%d0
	fmove.l %d0,%fpcr
	fmovem.l %fpcr/%fpsr,-(%sp)
	fsne %d0
	fjlt start
	fjeq ext
	fjgt .Lfar
	fjne .Lnear
	nop
.Lnear:
	fbne start
	fbeq.l start
	.zero	32760
.Lfar:
	fjngt start
	rts
	.data
	.align	2
value:
	.long	0,0,0
