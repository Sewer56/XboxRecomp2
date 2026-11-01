//! A small piece of x86 code that the recompile tests run.

/// Returns 0x200 bytes of code to map at 0x11000.
pub(crate) fn smoke_text() -> Vec<u8> {
    // Bytes not shown are CC (`int3`).
    //
    // ```text
    // .text:
    //   0x11000: E8 0B 00 00 00  call 0x11010
    //   0x11005: C3              ret
    //   0x11010: C3              ret             ; the function we call
    //   0x11020: 0F A2           cpuid           ; config size: 3 bytes
    //   0x11022: C3              ret
    //   0x11030: B8 05 00 00 00  mov eax, 5      ; config size: 5, no ret
    //   0x110EF: 90              nop             ; see below
    //   0x110F0: C3              ret             ; config size: 2 bytes
    //   0x110F1: CC              int3
    // ```
    let mut text = vec![0xCCu8; 0x200];
    text[0x00..0x05].copy_from_slice(&[0xE8, 0x0B, 0x00, 0x00, 0x00]);
    text[0x05] = 0xC3;
    text[0x10] = 0xC3;
    text[0x20] = 0x0F;
    text[0x21] = 0xA2;
    text[0x22] = 0xC3;
    text[0x30..0x35].copy_from_slice(&[0xB8, 0x05, 0x00, 0x00, 0x00]);
    text[0xEF] = 0x90;
    text[0xF0] = 0xC3;
    text
}
