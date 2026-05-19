.section __TEXT,__text
.global _main
.p2align 2
_main:
    mov  x0, #42
    mov  x16, #1
    svc  #0x80
