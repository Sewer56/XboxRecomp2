// The Xbox specific stuff to add to `x86_context.h`.
//
// - `//@state`: members added to `struct X86Context`.
// - `//@declarations`: file-scope declarations.

//@state
    uint32_t kpcr_ptr; //FS Segment (Xbox kernel)
    uint32_t stack_ptr; //FS Stack Ptr FS: 4
//@declarations
extern uint8_t g_mmio[];
extern uint8_t* g_nv2aPtr;
extern uint32_t XboxSignatureKey[];
