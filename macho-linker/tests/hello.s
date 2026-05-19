.section __TEXT,__cstring
.global _msg
_msg:
    .ascii "Hello, world!\n"

.section __TEXT,__text
.global _main
.p2align 2
_main:
    ; write(1, &msg, 14)
    mov  x0, #1
    adrp x1, _msg@PAGE
    add  x1, x1, _msg@PAGEOFF
    mov  x2, #14
    bl   _write
    ; exit(0)
    mov  x0, #0
    bl   _exit
