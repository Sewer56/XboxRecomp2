
// Sewer note: This file is an absolute mess and needs some serious cleanup.
//             Unfortunately, time does not allow.
//
//             Most of this is from original XboxRecomp, with minimal fixes applied
//             and a lot of speed hax. Time to clean this up, was not there...

#ifndef X86_CONTEXT_H_INCLUDED
#define X86_CONTEXT_H_INCLUDED

#ifndef X86_RECOMP_DEBUG
#define X86_RECOMP_DEBUG 0
#endif

// Whether the nonvolatile_memory speed hack is on.
#ifndef X86_NONVOLATILE_MEMORY
#define X86_NONVOLATILE_MEMORY 0
#endif

// Tells the compiler a point is never reached. The switch `default` case uses
// it when debug is off.
#if defined(_MSC_VER)
#define X86_UNREACHABLE() __assume(0)
#elif defined(__GNUC__) || defined(__clang__)
#define X86_UNREACHABLE() __builtin_unreachable()
#else
#error "The host compiler must provide an unreachable intrinsic"
#endif

// Inlines even where the compiler thinks the call is cold.
#if defined(_MSC_VER)
#define X86_FORCEINLINE __forceinline
#elif defined(__GNUC__) || defined(__clang__)
#define X86_FORCEINLINE inline __attribute__((always_inline))
#else
#define X86_FORCEINLINE inline
#endif

#ifndef X86_CONFIG_H_INCLUDED
#error "x86_config.h must be included before x86_context.h"
#endif

#include <climits>
#include <cmath>
#include <csetjmp>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <atomic>

#include "stdio.h"
#include "assert.h"

// For non-x86 hosts we went with the fastest form on an x86 machine.
// It may differ on AArch64, but I lack an AArch64 machine to test. - Sewer
#if defined(__i386__) || defined(__x86_64__)
#define X86_HOST_X86 1
#endif

#if defined(X86_HOST_X86) || defined(_M_IX86) || defined(_M_X64)
#include <x86intrin.h>
#ifdef _WIN32
#include <intrin.h>
#else
#include <xmmintrin.h>
#include <smmintrin.h>
#endif
#else
#if !defined(__aarch64__)
#include <chrono>
#endif
//@context:portable-simd
#endif

// Pick the packed SIMD backend at compile time.
#if !defined(X86_PACKED_SCALAR) && defined(__SSE2__) && \
    (defined(__i386__) || defined(__x86_64__))
#include <emmintrin.h>
#define X86_PACKED_SSE2 1
#elif !defined(X86_PACKED_SCALAR) && \
    (defined(__ARM_NEON) || defined(__ARM_NEON__)) && \
    defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
#include <arm_neon.h>
#define X86_PACKED_NEON 1
#endif

// ADC and SBB with the carry builtins are fastest on x86.
#if defined(__has_builtin)
#if __has_builtin(__builtin_subcb) && __has_builtin(__builtin_subcs) && __has_builtin(__builtin_subc) && \
    __has_builtin(__builtin_addcb) && __has_builtin(__builtin_addcs) && __has_builtin(__builtin_addc)
#define X86_HOST_SUBC 1
#endif
#endif

#ifndef M_LOG2E
#define M_LOG2E 1.44269504088896340735992468100189214L
#endif

#ifndef M_PI
#define M_PI 3.14159265358979323846L
#endif

// X87_DOUBLE_PRECISION: speed hack.
// 
// When enabled, we run x87 stack as `double` (64-bit).
// On MSVC, `long double` is already 64-bit. So only affects GCC/Clang.
//
// Non-x86 hosts always use `double`. Cause I have no way to bench it there.
#if defined(X87_DOUBLE_PRECISION) || !defined(X86_HOST_X86)
using x87_float = double;
#else
using x87_float = long double;
#endif

#define X86_JOIN(x, y) x##y
#define X86_XSTRINGIFY(x) #x
#define X86_STRINGIFY(x) X86_XSTRINGIFY(x)
#define X86_FUNC(x) void x(X86Context& __restrict ctx, uint8_t* base)
#define X86_OVERRIDE_FUNC(x) \
    static_assert(X86_JOIN(X86_OVERRIDE_ALLOWED_, x), \
        "Replacing this function requires disabling ABI-changing optimisations. " \
        "Add " X86_JOIN(X86_OVERRIDE_ADDRESS_, x) " to " \
        "[fixes].function_hooks in recompConfig.toml and regenerate."); \
    X86_FUNC(x)
#define X86_FUNC_IMPL(x) extern "C" X86_FUNC(x)
#define X86_EXTERN_FUNC(x) extern X86_FUNC(x)
#define X86_WEAK_ENTRY(x) __attribute__((weak)) X86_FUNC(x)

// Apple targets need wrappers because they do not support function aliases.
// Define X86_USE_FUNCTION_ALIAS=0 when another target needs wrappers too.
//
// Isn't C/C++ compiler standardization great? ffs.
#ifndef X86_USE_FUNCTION_ALIAS
#if defined(__APPLE__)
#define X86_USE_FUNCTION_ALIAS 0
#else
#define X86_USE_FUNCTION_ALIAS 1
#endif
#endif

// Defines the public entry and a callable original body named __imp__<name>.
#if X86_USE_FUNCTION_ALIAS
#define X86_DEFINE_FUNC(x, linkage) \
    __attribute__((alias(X86_STRINGIFY(X86_JOIN(__imp__, x))))) linkage(x); \
    X86_FUNC_IMPL(X86_JOIN(__imp__, x))
#else
#define X86_DEFINE_FUNC(x, linkage) \
    X86_FUNC_IMPL(X86_JOIN(__imp__, x)); \
    linkage(x) { X86_JOIN(__imp__, x)(ctx, base); } \
    X86_FUNC_IMPL(X86_JOIN(__imp__, x))
#endif
#define X86_WEAK_FUNC(x) X86_DEFINE_FUNC(x, X86_WEAK_ENTRY)
#define X86_PROTECTED_FUNC(x) X86_DEFINE_FUNC(x, X86_FUNC)

//Memory Load Macros
#define X86_LOAD_U8(addr)  (*reinterpret_cast<volatile uint8_t*>(base + (addr)))
#define X86_LOAD_U16(addr) (*reinterpret_cast<volatile uint16_t*>(base + (addr)))
#define X86_LOAD_U32(addr) (*reinterpret_cast<volatile uint32_t*>(base + (addr)))
#define X86_LOAD_U64(addr) (*reinterpret_cast<volatile uint64_t*>(base + (addr)))

#define X86_LOAD_F32(addr) (*reinterpret_cast<volatile float*>(base + (addr)))
#define X86_LOAD_F64(addr) (*reinterpret_cast<volatile double*>(base + (addr)))
#if defined(X86_HOST_X86)
#define X86_LOAD_F80(addr) (*reinterpret_cast<volatile long double*>(base + (addr)))
#else
#define X86_LOAD_F80(addr) x86_load_f80(base + (addr))
#endif

//Memory Store Macros
#define X86_STORE_U8(addr, value)  (*reinterpret_cast<volatile uint8_t*>(base + (addr)) = (value))
#define X86_STORE_U16(addr, value) (*reinterpret_cast<volatile uint16_t*>(base + (addr)) = (value))
#define X86_STORE_U32(addr, value) (*reinterpret_cast<volatile uint32_t*>(base + (addr)) = (value))
#define X86_STORE_U64(addr, value) (*reinterpret_cast<volatile uint64_t*>(base + (addr)) = (value))

//Floating point variants
#define X86_STORE_F32(addr, value) (*reinterpret_cast<volatile float*>(base + (addr)) = static_cast<float>(value))
#define X86_STORE_F64(addr, value) (*reinterpret_cast<volatile double*>(base + (addr)) = static_cast<double>(value))
#if defined(X86_HOST_X86)
#define X86_STORE_F80(addr, value) (*reinterpret_cast<volatile long double*>(base + (addr)) = static_cast<long double>(value))
#else
#define X86_STORE_F80(addr, value) x86_store_f80(base + (addr), static_cast<double>(value))

// m80 <-> double, for hosts without x87 long double.
inline double x86_load_f80(const uint8_t* address)
{
    uint64_t mantissa;
    uint16_t sign_exponent;
    memcpy(&mantissa, address, 8);
    memcpy(&sign_exponent, address + 8, 2);

    const int exponent = sign_exponent & 0x7FFF;
    double value;
    if (exponent == 0x7FFF)
        value = (mantissa << 1) ? std::numeric_limits<double>::quiet_NaN() : INFINITY;
    else
        value = std::ldexp(double(mantissa), (exponent ? exponent : 1) - 16383 - 63);
    return (sign_exponent & 0x8000) ? -value : value;
}

inline void x86_store_f80(uint8_t* address, double value)
{
    uint64_t mantissa = 0;
    int exponent = 0;
    if (std::isnan(value)) {
        mantissa = 0xC000000000000000ull;
        exponent = 0x7FFF;
    } else if (std::isinf(value)) {
        mantissa = 1ull << 63;
        exponent = 0x7FFF;
    } else if (value != 0) {
        int e;
        mantissa = uint64_t(std::ldexp(std::frexp(std::fabs(value), &e), 64));
        exponent = e - 1 + 16383;
    }
    const uint16_t sign_exponent = uint16_t(exponent | (std::signbit(value) ? 0x8000 : 0));
    memcpy(address, &mantissa, 8);
    memcpy(address + 8, &sign_exponent, 2);
}
#endif

// SSE Load/Store 128-bit Macros
#define XMM_F32(r, idx) (ctx.xmm[r].reg[idx])
#define XMM_U32(reg, idx) (ctx.xmm[reg].u32[idx])

typedef float x86_guest_m128_u __attribute__((vector_size(16), aligned(1), may_alias));
typedef float x86_guest_m128 __attribute__((vector_size(16), aligned(16), may_alias));

// Each operand is one volatile 16-byte source access; scalar hosts may split it.
#define X86_LOAD_PS(addr) (*reinterpret_cast<const volatile x86_guest_m128_u*>(base + (addr)))
#define X86_STORE_PS(addr, val) (*reinterpret_cast<volatile x86_guest_m128_u*>(base + (addr)) = (val))

// Aligned SSE Load/Store (16-byte alignment required)
#define X86_LOAD_APS(addr) (*reinterpret_cast<const volatile x86_guest_m128*>(base + (addr)))
#define X86_STORE_APS(addr, val) (*reinterpret_cast<volatile x86_guest_m128*>(base + (addr)) = (val))

// The plain_sse_memory speed hack drops volatile, so Clang can merge or drop these.
#define X86_NONVOLATILE_LOAD_PS(addr) (*reinterpret_cast<const x86_guest_m128_u*>(base + (addr)))
#define X86_NONVOLATILE_STORE_PS(addr, val) (*reinterpret_cast<x86_guest_m128_u*>(base + (addr)) = (val))
#define X86_NONVOLATILE_LOAD_APS(addr) (*reinterpret_cast<const x86_guest_m128*>(base + (addr)))
#define X86_NONVOLATILE_STORE_APS(addr, val) (*reinterpret_cast<x86_guest_m128*>(base + (addr)) = (val))

// Speed hacks: `nonvolatile_stack` and `nonvolatile_memory` use `Volatile = false` accesses.
template<typename T, bool Volatile>
struct x86_slot { typedef volatile T type; };
template<typename T>
struct x86_slot<T, false> { typedef T __attribute__((may_alias)) type; };

#define X86_LOAD_AS(T, Volatile, addr) (*reinterpret_cast<typename x86_slot<T, Volatile>::type*>(base + (addr)))
#define X86_STORE_AS(T, Volatile, addr, value) (X86_LOAD_AS(T, Volatile, addr) = (value))

#define X86_NONVOLATILE_LOAD_U8(addr)  X86_LOAD_AS(uint8_t, false, addr)
#define X86_NONVOLATILE_LOAD_U16(addr) X86_LOAD_AS(uint16_t, false, addr)
#define X86_NONVOLATILE_LOAD_U32(addr) X86_LOAD_AS(uint32_t, false, addr)
#define X86_NONVOLATILE_LOAD_U64(addr) X86_LOAD_AS(uint64_t, false, addr)
#define X86_NONVOLATILE_LOAD_F32(addr) X86_LOAD_AS(float, false, addr)
#define X86_NONVOLATILE_LOAD_F64(addr) X86_LOAD_AS(double, false, addr)

#define X86_NONVOLATILE_STORE_U8(addr, value)  X86_STORE_AS(uint8_t, false, addr, value)
#define X86_NONVOLATILE_STORE_U16(addr, value) X86_STORE_AS(uint16_t, false, addr, value)
#define X86_NONVOLATILE_STORE_U32(addr, value) X86_STORE_AS(uint32_t, false, addr, value)
#define X86_NONVOLATILE_STORE_U64(addr, value) X86_STORE_AS(uint64_t, false, addr, value)
#define X86_NONVOLATILE_STORE_F32(addr, value) X86_STORE_AS(float, false, addr, static_cast<float>(value))
#define X86_NONVOLATILE_STORE_F64(addr, value) X86_STORE_AS(double, false, addr, static_cast<double>(value))

#if defined(X86_HOST_X86)
#define X86_NONVOLATILE_LOAD_F80(addr) X86_LOAD_AS(long double, false, addr)
#define X86_NONVOLATILE_STORE_F80(addr, value) X86_STORE_AS(long double, false, addr, static_cast<long double>(value))
#else
#define X86_NONVOLATILE_LOAD_F80(addr) X86_LOAD_F80(addr)
#define X86_NONVOLATILE_STORE_F80(addr, value) X86_STORE_F80(addr, value)
#endif

// The streaming intrinsic drops volatile; assembly keeps the x86 streaming store.
#if defined(X86_HOST_X86)
X86_FORCEINLINE void x86_store_ntps(uint8_t* address, __m128 value)
{
#if defined(__AVX__)
    __asm__ volatile("vmovntps %1, %0" : "=m"(*reinterpret_cast<volatile x86_guest_m128*>(address)) : "x"(value) : "memory");
#else
    __asm__ volatile("movntps %1, %0" : "=m"(*reinterpret_cast<volatile x86_guest_m128*>(address)) : "x"(value) : "memory");
#endif
}
#define X86_STORE_NTPS(addr, val) x86_store_ntps(base + (addr), (val))
#else
#define X86_STORE_NTPS(addr, val) X86_STORE_APS(addr, val)
#endif
#if defined(__i386__)
X86_FORCEINLINE void x86_store_ntq(uint8_t* address, uint64_t value)
{
    // Clang splits _mm_stream_pi into two stores on i386. MOVNTQ keeps it eight bytes.
    // Spill live x87 values before using MMX, then leave the host stack empty.
    __asm__ volatile("movq %1, %%mm0; movntq %%mm0, %0; emms"
        : "=m"(*reinterpret_cast<volatile uint64_t*>(address)) : "m"(value)
        : "mm0", "st", "st(1)", "st(2)", "st(3)", "st(4)", "st(5)", "st(6)", "st(7)", "memory");
}
#define X86_STORE_NTQ(addr, val) x86_store_ntq(base + (addr), static_cast<uint64_t>(val))
#else
#define X86_STORE_NTQ(addr, val) \
    _mm_stream_si64(reinterpret_cast<long long*>(base + (addr)), \
                    static_cast<long long>(val))
#endif

// ── rep_memmove_memset speed hack ────────────────────────────────────────────

// Does a `rep movs` with one `memmove`, or a `rep stos` with one `memset`.
// Returns false if it can't, and the caller runs the normal loop instead.
#define X86_REP_MEMMOVE_MEMSET_MIN_BYTES 16u

// Size of the copy or fill in bytes, or 0 to use the normal loop.
template <typename T>
X86_FORCEINLINE uint32_t x86_rep_bytes(uint32_t ecx)
{
    // Too big to count in bytes.
    if (ecx > 0xFFFFFFFFu / sizeof(T))
        return 0;

    // Small copies are faster with the loop.
    // Note: If size is constant, then clang will unroll inner loop; which
    //       is how we can stay 'performant' here.
    const uint32_t bytes = ecx * sizeof(T);
    if (!__builtin_constant_p(bytes) && bytes < X86_REP_MEMMOVE_MEMSET_MIN_BYTES)
        return 0;

    return bytes;
}

template <typename T>
X86_FORCEINLINE bool x86_rep_movs_memmove(uint8_t* base, uint32_t& esi, uint32_t& edi, uint32_t& ecx, bool backward)
{
    const uint32_t bytes = x86_rep_bytes<T>(ecx);
    if (bytes == 0)
        return false;

    // When the destination overlaps the source, ahead of it, x86 behaves
    // differently than `memmove`, so we can't use it.
    const uint32_t ahead = backward ? esi - edi : edi - esi;
    if (ahead < bytes)
        return false;

    if (backward) {
        const uint32_t below = bytes - sizeof(T);
        memmove(base + edi - below, base + esi - below, bytes);
        esi -= bytes;
        edi -= bytes;
    } else {
        memmove(base + edi, base + esi, bytes);
        esi += bytes;
        edi += bytes;
    }

    ecx = 0;
    return true;
}

template <typename T>
X86_FORCEINLINE bool x86_rep_stos_memset(uint8_t* base, uint32_t& edi, uint32_t& ecx, T value, bool backward)
{
    const uint32_t bytes = x86_rep_bytes<T>(ecx);
    if (bytes == 0)
        return false;

    // Check if first byte is repeated.
    if (value != T(uint8_t(value) * 0x01010101u))
        return false;

    if (backward) {
        memset(base + edi - (bytes - sizeof(T)), uint8_t(value), bytes);
        edi -= bytes;
    } else {
        memset(base + edi, uint8_t(value), bytes);
        edi += bytes;
    }

    ecx = 0;
    return true;
}

// A release fence is a no-op on x86.
#if defined(X86_HOST_X86)
#define X86_SFENCE() _mm_sfence()
#else
#define X86_SFENCE() std::atomic_thread_fence(std::memory_order_release)
#endif

// Register Operations
#define X86_MOVAPS_REG(dest, src) \
    ctx.xmm[dest].reg = ctx.xmm[src].reg
#define X86_MOVLHPS(dest, src) \
    ctx.xmm[dest].reg = _mm_movelh_ps(ctx.xmm[dest].reg, ctx.xmm[src].reg)
#define X86_MOVHLPS(dest, src) \
    ctx.xmm[dest].reg = _mm_movehl_ps(ctx.xmm[dest].reg, ctx.xmm[src].reg)

// Arithmetic Operations
#define X86_ADDPS(dest, src) \
    ctx.xmm[dest].reg = _mm_add_ps(ctx.xmm[dest].reg, ctx.xmm[src].reg)
#define X86_MULPS(dest, src) \
    ctx.xmm[dest].reg = _mm_mul_ps(ctx.xmm[dest].reg, ctx.xmm[src].reg)
#define X86_SHUFPS(dest, src, imm) \
    ctx.xmm[dest].reg = _mm_shuffle_ps(ctx.xmm[dest].reg, ctx.xmm[src].reg, imm)

// MMX Load/Store
#define X86_MMX_LOAD(addr) (*reinterpret_cast<uint64_t*>(base + addr))
#define X86_MMX_STORE(addr, value) (*reinterpret_cast<uint64_t*>(base + addr) = value)
#define MMX_REG(reg) (ctx.fpu.mmx[(reg)]) 

// Clang 20+ compiles MMX intrinsics to SSE2, so host code never touches the
// x87 registers and the host emms can go. Older compilers still need it.
#if (defined(X86_HOST_X86) || defined(_M_IX86) || defined(_M_X64)) && \
    !(defined(__clang__) && __clang_major__ >= 20)
#define X86_EMMS() _mm_empty()
#else
#define X86_EMMS() ((void)0)
#endif

// On x86, xchg always locks memory, so relaxed is enough.
// volatile stops guest memory reads and writes moving around it.
// Other hosts keep seq_cst.
#if defined(X86_HOST_X86) && !X86_NONVOLATILE_MEMORY
#define X86_ATOMIC_XCHG_U8(addr, value)  std::atomic_exchange_explicit(reinterpret_cast<volatile std::atomic<uint8_t>*>(base + (addr)), static_cast<uint8_t>(value), std::memory_order_relaxed)
#define X86_ATOMIC_XCHG_U16(addr, value) std::atomic_exchange_explicit(reinterpret_cast<volatile std::atomic<uint16_t>*>(base + (addr)), static_cast<uint16_t>(value), std::memory_order_relaxed)
#define X86_ATOMIC_XCHG_U32(addr, value) std::atomic_exchange_explicit(reinterpret_cast<volatile std::atomic<uint32_t>*>(base + (addr)), static_cast<uint32_t>(value), std::memory_order_relaxed)
#else
#define X86_ATOMIC_XCHG_U8(addr, value)  std::atomic_exchange(reinterpret_cast<std::atomic<uint8_t>*>(base + (addr)), static_cast<uint8_t>(value))
#define X86_ATOMIC_XCHG_U16(addr, value) std::atomic_exchange(reinterpret_cast<std::atomic<uint16_t>*>(base + (addr)), static_cast<uint16_t>(value))
#define X86_ATOMIC_XCHG_U32(addr, value) std::atomic_exchange(reinterpret_cast<std::atomic<uint32_t>*>(base + (addr)), static_cast<uint32_t>(value))
#endif

// Same for lock xadd.
#if defined(X86_HOST_X86) && !X86_NONVOLATILE_MEMORY
#define X86_ATOMIC_XADD_U8(addr, value)  std::atomic_fetch_add_explicit(reinterpret_cast<volatile std::atomic<uint8_t>*>(base + (addr)), static_cast<uint8_t>(value), std::memory_order_relaxed)
#define X86_ATOMIC_XADD_U16(addr, value) std::atomic_fetch_add_explicit(reinterpret_cast<volatile std::atomic<uint16_t>*>(base + (addr)), static_cast<uint16_t>(value), std::memory_order_relaxed)
#define X86_ATOMIC_XADD_U32(addr, value) std::atomic_fetch_add_explicit(reinterpret_cast<volatile std::atomic<uint32_t>*>(base + (addr)), static_cast<uint32_t>(value), std::memory_order_relaxed)
#else
#define X86_ATOMIC_XADD_U8(addr, value)  std::atomic_fetch_add(reinterpret_cast<std::atomic<uint8_t>*>(base + (addr)), static_cast<uint8_t>(value))
#define X86_ATOMIC_XADD_U16(addr, value) std::atomic_fetch_add(reinterpret_cast<std::atomic<uint16_t>*>(base + (addr)), static_cast<uint16_t>(value))
#define X86_ATOMIC_XADD_U32(addr, value) std::atomic_fetch_add(reinterpret_cast<std::atomic<uint32_t>*>(base + (addr)), static_cast<uint32_t>(value))
#endif


#ifndef X86_CALL_FUNC
#define X86_CALL_FUNC(x) x(ctx, base)
#endif

#define X86_MEMORY_SIZE 0x100000000ull

#define X86_LOOKUP_FUNC(x, y) *(X86Func**)(x + X86_IMAGE_BASE + X86_IMAGE_SIZE + (uint64_t(uint32_t(y) - X86_CODE_BASE) * 2))

#ifndef X86_CALL_INDIRECT_FUNC
#define X86_CALL_INDIRECT_FUNC(x) (X86_LOOKUP_FUNC(base, x))(ctx, base)
#endif



//@context:declarations

typedef void X86Func(struct X86Context& __restrict ctx, uint8_t* base);

struct X86FuncMapping
{
    size_t guest;
    X86Func* host;
};

extern X86FuncMapping X86FuncMappings[];

struct X86Register {
    union {
        uint64_t u64;
        uint32_t u32;
        int32_t i32;
        struct {
            uint16_t u16;
            int16_t i16;
        };
        struct {
            uint8_t u8_lo;
            uint8_t u8_hi;
        };
    };
};

struct X86EFlags
{
    // Funny optimisation.
    //
    // Most instructions write only the `common` flags.
    // The rest (DF, IF and reserved bits) are in `rare`.
    //
    // Observed some speedups here.
    uint32_t common = 0;
    uint32_t rare = 0;

    // Flag bits (matches x86 architecture)
    enum : uint32_t {
        CF = 1 << 0,  // Carry
        PF = 1 << 2,  // Parity
        AF = 1 << 4,  // Adjust
        ZF = 1 << 6,  // Zero
        SF = 1 << 7,  // Sign
        IF = 1 << 9,  // Interrupt enable
        DF = (1 << 10),
        OF = 1 << 11  // Overflow
    };

    enum : uint32_t { ARITHMETIC_FLAGS = CF | PF | AF | ZF | SF | OF };

    X86_FORCEINLINE uint32_t packed() const
    {
        return common | rare;
    }

    X86_FORCEINLINE void set_packed(uint32_t flags)
    {
        common = flags & ARITHMETIC_FLAGS;
        rare = flags & ~ARITHMETIC_FLAGS;
    }

private:
    template<uint32_t Needed, uint32_t Changed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void write_flags(uint32_t flags)
    {
        static_assert((Needed & ~ARITHMETIC_FLAGS) == 0, "unsupported arithmetic flag mask");
        constexpr uint32_t written = Needed & Changed;

        if constexpr (written != 0) {
            constexpr uint32_t preserved = Needed & (ARITHMETIC_FLAGS & ~Changed);
            if constexpr (preserved == 0)
                common = flags;
            else
                common = (common & preserved) | flags;
        }
    }

    template<unsigned Bits, uint32_t Needed>
    static X86_FORCEINLINE uint32_t result_flags(uint32_t result)
    {
        static_assert(Bits == 8 || Bits == 16 || Bits == 32, "unsupported operand width");
        uint32_t flags = 0;

        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;

        if constexpr ((Needed & SF) != 0)
            flags |= (result >> (Bits - 8)) & SF;

        if constexpr ((Needed & PF) != 0) {
            flags |= uint32_t(__builtin_parity(result & 0xFF) == 0) * PF;
        }

        return flags;
    }

    template<unsigned Bits>
    static X86_FORCEINLINE uint32_t overflow_flag(uint32_t bits)
    {
        if constexpr (Bits >= 12)
            return (bits >> (Bits - 12)) & OF;
        else
            return (bits << (12 - Bits)) & OF;
    }

    template<unsigned Bits>
    static X86_FORCEINLINE uint32_t add_overflow_flag(uint32_t lhs, uint32_t rhs)
    {
        bool overflow;
        if constexpr (Bits == 8) {
            int8_t value;
            overflow = __builtin_add_overflow(int8_t(lhs), int8_t(rhs), &value);
        } else if constexpr (Bits == 16) {
            int16_t value;
            overflow = __builtin_add_overflow(int16_t(lhs), int16_t(rhs), &value);
        } else {
            int32_t value;
            overflow = __builtin_add_overflow(int32_t(lhs), int32_t(rhs), &value);
        }
        return uint32_t(overflow) * OF;
    }

    template<unsigned Bits>
    static X86_FORCEINLINE uint32_t sub_overflow_flag(uint32_t lhs, uint32_t rhs)
    {
        bool overflow;
        if constexpr (Bits == 8) {
            int8_t value;
            overflow = __builtin_sub_overflow(int8_t(lhs), int8_t(rhs), &value);
        } else if constexpr (Bits == 16) {
            int16_t value;
            overflow = __builtin_sub_overflow(int16_t(lhs), int16_t(rhs), &value);
        } else {
            int32_t value;
            overflow = __builtin_sub_overflow(int32_t(lhs), int32_t(rhs), &value);
        }
        return uint32_t(overflow) * OF;
    }

    template<unsigned Bits, uint32_t Needed>
    X86_FORCEINLINE void update_add(uint32_t lhs, uint32_t rhs, uint32_t result)
    {
        uint32_t flags = result_flags<Bits, Needed>(result);

        if constexpr ((Needed & CF) != 0) {
            if constexpr (Bits == 32) {
                uint32_t sum;
                flags |= uint32_t(__builtin_add_overflow(lhs, rhs, &sum)) * CF;
            } else
                flags |= ((lhs + rhs) >> Bits) & CF;
        }

        if constexpr ((Needed & OF) != 0)
            flags |= add_overflow_flag<Bits>(lhs, rhs);

        if constexpr ((Needed & AF) != 0) {
#if defined(__aarch64__)
            flags |= (__builtin_constant_p(rhs & 0xF) && (rhs & 0xF) == 0 ? 0u : lhs ^ rhs ^ result) & AF;
#else
            if (!__builtin_constant_p(rhs & 0xF) || (rhs & 0xF) != 0)
                flags |= (lhs ^ rhs ^ result) & AF;
#endif
        }

        write_flags<Needed>(flags);
    }

    template<unsigned Bits, uint32_t Needed>
    X86_FORCEINLINE void update_adc(uint32_t lhs, uint32_t rhs, uint32_t carry, uint32_t result,
                                    uint32_t carry_out)
    {
        uint32_t flags = result_flags<Bits, Needed>(result);

        if constexpr ((Needed & CF) != 0) {
#if defined(X86_HOST_SUBC)
            flags |= carry_out * CF;
#else
            (void)carry_out;
            if constexpr (Bits == 32)
                flags |= (((lhs & rhs) | ((lhs | rhs) & ~result)) >> 31) * CF;
            else
                flags |= ((lhs + rhs + carry) >> Bits) & CF;
#endif
        }
        if constexpr ((Needed & OF) != 0)
            flags |= overflow_flag<Bits>((lhs ^ result) & (rhs ^ result));
        if constexpr ((Needed & AF) != 0) {
            // Plain form:
            // flags |= uint32_t(((lhs & 0xF) + (rhs & 0xF) + carry) > 0xF) * AF;
            flags |= (lhs ^ rhs ^ result) & AF;
        }

        write_flags<Needed>(flags);
    }

    template<unsigned Bits, uint32_t Needed>
    X86_FORCEINLINE void update_sbb(uint32_t lhs, uint32_t rhs, uint32_t borrow, uint32_t result,
                                    uint32_t borrow_out)
    {
        if (__builtin_constant_p(lhs == rhs) && lhs == rhs) {
            write_flags<Needed>(borrow != 0 ? CF | PF | AF | SF : PF | ZF);
            return;
        }

        uint32_t flags = result_flags<Bits, Needed>(result);

        if constexpr ((Needed & CF) != 0) {
#if defined(X86_HOST_SUBC)
            flags |= borrow_out * CF;
#else
            (void)borrow_out;
            if constexpr (Bits != 32)
                flags |= ((lhs - rhs - borrow) >> Bits) & CF;
            else
                flags |= (((~lhs & rhs) | (~(lhs ^ rhs) & result)) >> 31) * CF;
#endif
        }
        if constexpr ((Needed & OF) != 0)
            flags |= overflow_flag<Bits>((lhs ^ rhs) & (lhs ^ result));

        if constexpr ((Needed & AF) != 0)
            flags |= (lhs ^ rhs ^ result) & AF;

        write_flags<Needed>(flags);
    }

    template<unsigned Bits, uint32_t Needed>
    X86_FORCEINLINE void update_sub(uint32_t lhs, uint32_t rhs, uint32_t result)
    {
        uint32_t flags = result_flags<Bits, Needed>(result);
        const uint32_t changed = lhs ^ result;
        uint32_t arithmetic = 0;

        if constexpr ((Needed & CF) != 0)
            arithmetic |= uint32_t(lhs < rhs) * CF;

        if constexpr ((Needed & OF) != 0) {
#if defined(X86_HOST_X86)
            if constexpr (Bits != 8)
                arithmetic |= sub_overflow_flag<Bits>(lhs, rhs);
            else
                arithmetic |= overflow_flag<Bits>((lhs ^ rhs) & changed);
#elif defined(__aarch64__)
            // arm was optimised for lowest instruction count under qemu. No idea.
            if (Bits == 32 && !__builtin_constant_p(rhs))
                arithmetic |= sub_overflow_flag<Bits>(lhs, rhs);
            else
                arithmetic |= overflow_flag<Bits>((lhs ^ rhs) & changed);
#elif defined(__arm__)
            if constexpr (Bits == 32)
                arithmetic |= sub_overflow_flag<Bits>(lhs, rhs);
            else
                arithmetic |= overflow_flag<Bits>((lhs ^ rhs) & changed);
#else
            arithmetic |= overflow_flag<Bits>((lhs ^ rhs) & changed);
#endif
        }

        if constexpr ((Needed & AF) != 0)
            arithmetic |= (changed ^ rhs) & AF;


        write_flags<Needed>(flags | arithmetic);
    }

    template<unsigned Bits, uint32_t Needed, bool Decrement>
    X86_FORCEINLINE void update_step(uint32_t old_val, uint32_t result)
    {
        uint32_t flags = result_flags<Bits, Needed>(result);

        if constexpr ((Needed & OF) != 0) {
#if defined(__aarch64__)
            flags |= overflow_flag<Bits>(Decrement ? (old_val & ~result) : (result & ~old_val));
#else
            constexpr uint32_t sign = uint32_t(1) << (Bits - 1);
            flags |= uint32_t(old_val == (Decrement ? sign : sign - 1)) * OF;
#endif
        }

        if constexpr ((Needed & AF) != 0) {
            // Plain form:
            // flags |= uint32_t((old_val & 0xF) == (Decrement ? 0u : 0xFu)) * AF;
            flags |= (old_val ^ result) & AF;
        }

        // Preserve Carry Flag
        write_flags<Needed, ARITHMETIC_FLAGS & ~CF>(flags);
    }

    template<unsigned Bits, uint32_t Needed>
    X86_FORCEINLINE void update_neg(uint32_t original, uint32_t result)
    {
        uint32_t flags = result_flags<Bits, Needed>(result);

        if constexpr ((Needed & CF) != 0)
            flags |= uint32_t(original != 0) * CF;
        if constexpr ((Needed & OF) != 0)
#if defined(__aarch64__)
            flags |= ((original & result) >> (Bits - 1)) * OF;
#else
            flags |= uint32_t(original == (uint32_t(1) << (Bits - 1))) * OF;
#endif

        if constexpr ((Needed & AF) != 0)
            flags |= (original ^ result) & AF;

        write_flags<Needed>(flags);
    }

public:
    X86_FORCEINLINE void clear_eflags()
    {
        common = 0;
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_compare_flags_U32(uint32_t lhs, uint32_t rhs)
    {
        if constexpr (Needed != 0)
            update_sub<32, Needed>(lhs, rhs, lhs - rhs);
    }


    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_compare_flags_U16(uint16_t lhs, uint16_t rhs)
    {
        if constexpr (Needed != 0)
            update_sub<16, Needed>(lhs, rhs, uint16_t(lhs - rhs));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_compare_flags_U8(uint8_t lhs, uint8_t rhs)
    {
        if constexpr (Needed != 0)
            update_sub<8, Needed>(lhs, rhs, uint8_t(lhs - rhs));
    }

    X86_FORCEINLINE void update_compare_flags_sse(float lhs, float rhs)
    {
#if defined(__aarch64__)
        uint32_t flags;
        __asm__ volatile("fcmp %s1, %s2\n\tcsel %w0, %w3, wzr, eq\n\tcsel %w0, %w4, %w0, vs\n\tcsinc %w0, %w0, wzr, pl"
                         : "=&r"(flags) : "w"(lhs), "w"(rhs), "r"(ZF), "r"(ZF | PF | CF) : "cc");
        common = flags;
#elif defined(__arm__) && defined(__ARM_FP)
        uint32_t flags;
        __asm__ volatile("vcmp.f32 %1, %2\n\tvmrs APSR_nzcv, fpscr\n\tmov %0, #0\n\t"
                         "it lt\n\tmovlt %0, %3\n\tit eq\n\tmoveq %0, %4\n\tit vs\n\tmovvs %0, %5"
                         : "=&r"(flags) : "t"(lhs), "t"(rhs), "I"(CF), "I"(ZF), "I"(ZF | PF | CF) : "cc");
        common = flags;
#else
        common = __builtin_isunordered(lhs, rhs) ? (ZF | PF | CF) :
                 !__builtin_islessgreater(lhs, rhs) ? ZF : (__builtin_isless(lhs, rhs) ? CF : 0);
#endif
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_test_flags_U8(uint8_t result)
    {
        write_flags<Needed>(result_flags<8, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_test_flags_U16(uint16_t result)
    {
        write_flags<Needed>(result_flags<16, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_test_flags_U32(uint32_t result)
    {
        write_flags<Needed>(result_flags<32, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_logic(uint32_t result)
    {
        write_flags<Needed>(result_flags<32, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_logic(uint16_t result)
    {
        write_flags<Needed>(result_flags<16, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_logic(uint8_t result)
    {
        write_flags<Needed>(result_flags<8, Needed>(result));
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_adc_U32(uint32_t lhs, uint32_t rhs, uint32_t carry, uint32_t result,
                                              uint32_t carry_out)
    {
        update_adc<32, Needed>(lhs, rhs, carry, result, carry_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_adc_U16(uint16_t lhs, uint16_t rhs, uint32_t carry, uint16_t result,
                                              uint32_t carry_out)
    {
        update_adc<16, Needed>(lhs, rhs, carry, result, carry_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_adc_U8(uint8_t lhs, uint8_t rhs, uint32_t carry, uint8_t result,
                                             uint32_t carry_out)
    {
        update_adc<8, Needed>(lhs, rhs, carry, result, carry_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_add_U32(uint32_t lhs, uint32_t rhs, uint32_t result)
    {
        update_add<32, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_add_U16(uint16_t lhs, uint16_t rhs, uint16_t result)
    {
        update_add<16, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_add_U8(uint8_t lhs, uint8_t rhs, uint8_t result)
    {
        update_add<8, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sub_U32(uint32_t lhs, uint32_t rhs, uint32_t result)
    {
        update_sub<32, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sub_U16(uint16_t lhs, uint16_t rhs, uint16_t result)
    {
        update_sub<16, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sub_U8(uint8_t lhs, uint8_t rhs, uint8_t result)
    {
        update_sub<8, Needed>(lhs, rhs, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sbb_U32(uint32_t lhs, uint32_t rhs, uint32_t borrow, uint32_t result,
                                              uint32_t borrow_out)
    {
        update_sbb<32, Needed>(lhs, rhs, borrow, result, borrow_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sbb_U16(uint16_t lhs, uint16_t rhs, uint32_t borrow, uint16_t result,
                                              uint32_t borrow_out)
    {
        update_sbb<16, Needed>(lhs, rhs, borrow, result, borrow_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sbb_U8(uint8_t lhs, uint8_t rhs, uint32_t borrow, uint8_t result,
                                             uint32_t borrow_out)
    {
        update_sbb<8, Needed>(lhs, rhs, borrow, result, borrow_out);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_mul(bool overflow)
    {
        uint32_t flags = 0;
        if constexpr ((Needed & (CF | OF)) != 0)
            flags = uint32_t(overflow) * (CF | OF);

        write_flags<Needed>(flags);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_inc_U32(uint32_t old_val, uint32_t result)
    {
        update_step<32, Needed, false>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_inc_U16(uint16_t old_val, uint16_t result)
    {
        update_step<16, Needed, false>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_inc_U8(uint8_t old_val, uint8_t result)
    {
        update_step<8, Needed, false>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_dec_U32(uint32_t old_val, uint32_t result)
    {
        update_step<32, Needed, true>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_dec_U16(uint16_t old_val, uint16_t result)
    {
        update_step<16, Needed, true>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_dec_U8(uint8_t old_val, uint8_t result)
    {
        update_step<8, Needed, true>(old_val, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_neg_U32(uint32_t original_value, uint32_t result)
    {
        update_neg<32, Needed>(original_value, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_neg_U16(uint16_t original_value, uint16_t result)
    {
        update_neg<16, Needed>(original_value, result);
    }


    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_neg_U8(uint8_t original_value, uint8_t result)
    {
        update_neg<8, Needed>(original_value, result);
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_shl(uint32_t value_before, uint32_t shift_count, uint32_t result, uint8_t operand_size)
    {
        const uint32_t bits = operand_size * 8;

        // count == 0 is a no-op
        if constexpr (Needed != ARITHMETIC_FLAGS) {
            if (shift_count == 0)
                return;
        }
        const uint32_t old = common;
        uint32_t flags = 0;

        // CF = last bit shifted out (MSB side)
        // Plain form:
        // flags |= (value_before >> (bits - shift_count)) & CF;
        if constexpr ((Needed & CF) != 0) {
            flags |= bits == 32 ? ((value_before >> 1) >> (31 - shift_count)) & CF
                                : ((value_before << shift_count) >> bits) & CF;
        }
        // OF = sign bit changed (1-bit shifts only)
        if constexpr ((Needed & OF) != 0) {
            if (shift_count == 1)
                flags |= (((value_before ^ result) >> (bits - 1)) & 1) * OF;
        }
        // PF
        if constexpr ((Needed & PF) != 0)
            flags |= result_flags<8, PF>(result);
        // SF = MSB of result
        if constexpr ((Needed & SF) != 0)
            flags |= ((result >> (bits - 1)) & 1) * SF;
        // ZF
        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;
        if constexpr (Needed != 0)
            common = shift_count != 0 ? flags : old;
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_shr(uint32_t value_before, uint32_t shift_count, uint32_t result, uint8_t operand_size)
    {
        const uint32_t bits = operand_size * 8;

        // count == 0 is a no-op
        if constexpr (Needed != ARITHMETIC_FLAGS) {
            if (shift_count == 0)
                return;
        }
        const uint32_t old = common;
        uint32_t flags = 0;

        // CF = last bit shifted out
        // Plain form:
        // flags |= (value_before >> (shift_count - 1)) & CF;
        if constexpr ((Needed & CF) != 0)
            flags |= ((value_before << 1) >> shift_count) & CF;
        // OF = MSB of original value, based on operand size
        if constexpr ((Needed & OF) != 0) {
            if (shift_count == 1)
                flags |= ((value_before >> (bits - 1)) & 1) * OF;
        }
        if constexpr ((Needed & PF) != 0)
            flags |= result_flags<8, PF>(result);
        // SF = MSB of result, based on operand size
        if constexpr ((Needed & SF) != 0)
            flags |= ((result >> (bits - 1)) & 1) * SF;
        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;
        if constexpr (Needed != 0)
            common = shift_count != 0 ? flags : old;
    }

    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_shld(uint32_t value_before, uint32_t shift_count, uint32_t result, uint32_t operand_size) {
        const uint32_t bits = operand_size;

        // count == 0 is a no-op
        if constexpr (Needed != ARITHMETIC_FLAGS) {
            if (shift_count == 0)
                return;
        }
        const uint32_t old = common;
        uint32_t flags = 0;

        // CF = Last bit shifted out
        // Plain form:
        // flags |= (value_before >> (bits - shift_count)) & CF;
        if constexpr ((Needed & CF) != 0) {
            const uint32_t cf = bits == 32 ? ((value_before >> 1) >> (31 - shift_count)) & CF
                                           : ((value_before << shift_count) >> 16) & CF;
            flags |= cf & uint32_t(shift_count <= bits);
        }
        // OF = Set if sign bit changed (only for 1-bit shifts)
        if constexpr ((Needed & OF) != 0) {
            if (shift_count == 1)
                flags |= (((value_before ^ result) >> (bits - 1)) & 1) * OF;
        }
        // PF = Parity of LSB
        if constexpr ((Needed & PF) != 0)
            flags |= result_flags<8, PF>(result);

        // SF = MSB of result
        if constexpr ((Needed & SF) != 0)
            flags |= ((result >> (bits - 1)) & 1) * SF;
        // ZF = Result is zero
        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;
        if constexpr (Needed != 0)
            common = shift_count != 0 ? flags : old;
    }

    // Updates flags after SHRD (Shift Right Double)
    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_shrd(uint32_t value_before, uint32_t shift_count, uint32_t result, uint32_t operand_size) {
        const uint32_t bits = operand_size;

        // count == 0 is a no-op
        if constexpr (Needed != ARITHMETIC_FLAGS) {
            if (shift_count == 0)
                return;
        }
        const uint32_t old = common;
        uint32_t flags = 0;

        // CF = Last bit shifted out
        // Plain form:
        // flags |= (value_before >> (shift_count - 1)) & CF;
        if constexpr ((Needed & CF) != 0)
            flags |= ((value_before << 1) >> shift_count) & CF & uint32_t(shift_count <= bits);
        // OF = Set if sign bit changed (only for 1-bit shifts)
        if constexpr ((Needed & OF) != 0) {
            if (shift_count == 1)
                flags |= (((value_before ^ result) >> (bits - 1)) & 1) * OF;
        }
        // PF = Parity of LSB
        if constexpr ((Needed & PF) != 0)
            flags |= result_flags<8, PF>(result);
        // SF = MSB of result
        if constexpr ((Needed & SF) != 0)
            flags |= ((result >> (bits - 1)) & 1) * SF;
        // ZF = Result is zero
        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;
        if constexpr (Needed != 0)
            common = shift_count != 0 ? flags : old;
    }

    // Updates flags after SAR (Shift Arithmetic Right)
    template<uint32_t Needed = ARITHMETIC_FLAGS>
    X86_FORCEINLINE void update_flags_sar(uint32_t value_before, uint32_t shift_count, uint32_t result, uint8_t operand_size)
    {
        const uint32_t bits = operand_size * 8;

        // count == 0 is a no-op
        if constexpr (Needed != ARITHMETIC_FLAGS) {
            if (shift_count == 0)
                return;
        }
        const uint32_t old = common;
        uint32_t flags = 0;

        // CF = last bit shifted out
        if constexpr ((Needed & CF) != 0) {
            const int32_t extended = int32_t(value_before << (32 - bits)) >> (32 - bits);
            flags |= ((uint32_t(extended) << 1) >> shift_count) & CF;
        }
        // OF = 0 (SAR keeps the sign)
        if constexpr ((Needed & PF) != 0)
            flags |= result_flags<8, PF>(result);
        if constexpr ((Needed & SF) != 0)
            flags |= ((result >> (bits - 1)) & 1) * SF;
        if constexpr ((Needed & ZF) != 0)
            flags |= uint32_t(result == 0) * ZF;
        if constexpr (Needed != 0)
            common = shift_count != 0 ? flags : old;
    }

    // Updates flags after ROL (Rotate Left)
    template<unsigned Bits>
    X86_FORCEINLINE void update_flags_rol(uint32_t masked_count, uint32_t result)
    {
        if (masked_count == 0)
            return;

        const uint32_t cf = result & 1;
        const uint32_t of = ((result >> (Bits - 1)) ^ cf) & 1;
        common = (common & ~(CF | OF)) | cf | (of << 11);
    }

    // Updates flags after ROR (Rotate Right)
    template<unsigned Bits>
    X86_FORCEINLINE void update_flags_ror(uint32_t masked_count, uint32_t result)
    {
        if (masked_count == 0)
            return;

        const uint32_t cf = (result >> (Bits - 1)) & 1;
        const uint32_t of = (cf ^ (result >> (Bits - 2))) & 1;
        common = (common & ~(CF | OF)) | cf | (of << 11);
    }

    // For STOS/MOVS instructions 
    void update_string_pointer(uint32_t& reg) {
        reg += 1u - ((rare & DF) >> 9);
    }

    void update_string_pointer_word(uint32_t& reg) {
        reg += 2u - ((rare & DF) >> 8);
    }

    // Direction Flag control
    void set_df(bool backward) {
        rare = (rare & ~DF) | (uint32_t(backward) * DF);
    }
};

// Returns true if a float compare would set any Taken flag.
//
// - Unordered: CF, ZF and PF.
// - Less: CF.
// - Equal: ZF.
template<uint32_t Taken, class Float>
X86_FORCEINLINE bool x86_float_branch(Float lhs, Float rhs)
{
    if constexpr (Taken == X86EFlags::CF)
        return !__builtin_isgreaterequal(lhs, rhs);
    else if constexpr (Taken == X86EFlags::ZF)
        return !__builtin_islessgreater(lhs, rhs);
    else if constexpr (Taken == (X86EFlags::CF | X86EFlags::ZF))
        return !__builtin_isgreater(lhs, rhs);
    else {
        static_assert(Taken == X86EFlags::PF, "float compares set only CF, ZF and PF");
        return __builtin_isunordered(lhs, rhs);
    }
}

// Returns the CF, ZF and PF a float compare sets.
template<class Float>
X86_FORCEINLINE uint32_t x86_float_flags(Float lhs, Float rhs)
{
    return __builtin_isunordered(lhs, rhs) ? (X86EFlags::ZF | X86EFlags::PF | X86EFlags::CF) :
           !__builtin_islessgreater(lhs, rhs) ? X86EFlags::ZF :
           (__builtin_isless(lhs, rhs) ? X86EFlags::CF : 0);
}

struct X87FPU
{
    union {
        x87_float st[8];    // 8-register FPU stack (ST0-ST7)
        uint64_t mmx[8];    // MMX registers (MM0-MM7) - share physical space with FPU
    };
    uint16_t top = 0; //Stack pointer (0-7)
    uint16_t cw = 0x037F; //Control word (default: round-to-nearest, 64-bit precision)
    uint16_t sw = 0;  //Status word tells what's currently happening inside the FPU
    enum : uint16_t {
        C0 = (1 << 8),  // Condition bit 0
        C1 = (1 << 9),  // Condition bit 1
        C2 = (1 << 10), // Condition bit 2 (for FCOS range reduction)
        C3 = (1 << 14)  // Condition bit 3
    };
    // One tag per physical register (0 = valid, 3 = empty).
    struct TagWord {
        uint8_t tag[8] = {3, 3, 3, 3, 3, 3, 3, 3};
        operator uint16_t() const
        {
            uint16_t word = 0;
            for (int i = 0; i < 8; ++i)
                word = uint16_t(word | ((tag[i] & 3) << (i * 2)));
            return word;
        }
        TagWord& operator=(uint16_t word)
        {
            for (int i = 0; i < 8; ++i)
                tag[i] = uint8_t((word >> (i * 2)) & 3);
            return *this;
        }
        TagWord& operator|=(uint16_t word) { return *this = uint16_t(uint16_t(*this) | word); }
        TagWord& operator&=(uint16_t word) { return *this = uint16_t(uint16_t(*this) & word); }
    };
    TagWord tw; // All empty by default.

    static constexpr x87_float ONE = 1.0L;
    static constexpr x87_float ZERO = 0.0L;
    static constexpr x87_float PI = 3.14159265358979323846264338327950288L;
    static constexpr x87_float L2E = 1.44269504088896340735992468100189214L;
    static constexpr x87_float LG2 = 0.30102999566398119521373889472449L;
    static constexpr x87_float LN2 = 0.693147180559945309417232121458176568L;

    // Checked = false is the fast_x87_stack speed hack.
    template<bool Checked = true>
    X86_FORCEINLINE void push(x87_float value)
    {
        top = (top - 1) & 7;
        uint16_t status = uint16_t((sw & ~0x3800) | (top << 11));
        const bool full = Checked && tw.tag[top] != 3;
#if defined(__aarch64__)
        // Fewer instructions, tested under QEMU.
        sw = uint16_t(status | (full ? 0x241 : 0));
        st[top] = full ? -std::numeric_limits<x87_float>::quiet_NaN() : value;
#else
        if (full) {
            status |= 0x241;
            value = -std::numeric_limits<x87_float>::quiet_NaN();
        }
        sw = status;

        st[top] = value;
#endif

        tw.tag[top] = 0;
    }

    template<bool Checked = true>
    X86_FORCEINLINE x87_float pop()
    {
        const bool empty = Checked && tw.tag[top] == 3;
        const x87_float val = empty ? -std::numeric_limits<x87_float>::quiet_NaN() : st[top];
        tw.tag[top] = 3;
        top = (top + 1) & 7;
        sw = uint16_t((sw & ~0x3800 & ~(uint16_t(empty) << 9)) | (top << 11) | (empty ? 0x41 : 0));
        return val;
    }

    X86_FORCEINLINE void ffree(int8_t i)
    {
        tw.tag[(top + i) & 7] = 3;
    }

    X86_FORCEINLINE x87_float& st0() { return st[top]; }
    X86_FORCEINLINE x87_float& st1() { return st[(top + 1) & 7]; }
    X86_FORCEINLINE x87_float& st2() { return st[(top + 2) & 7]; }
    X86_FORCEINLINE x87_float& st3() { return st[(top + 3) & 7]; }
    X86_FORCEINLINE x87_float& st4() { return st[(top + 4) & 7]; }
    X86_FORCEINLINE x87_float& st5() { return st[(top + 5) & 7]; }
    X86_FORCEINLINE x87_float& st6() { return st[(top + 6) & 7]; }
    X86_FORCEINLINE x87_float& st7() { return st[(top + 7) & 7]; }

    X86_FORCEINLINE x87_float& getST(uint8_t index)
    {
        const uint8_t offset = index < 8 ? index : 0;
        return st[(top + offset) & 7];
    }



    void init()
    {
        cw = 0x37F;
        tw = 0xFFFF;
        sw = 0;
        top = 0;
    }

    template<bool Volatile = true>
    void save(uint32_t addr, uint8_t* base, bool is32bits)
    {
        if (is32bits)
        {
            X86_STORE_AS(uint16_t, Volatile, addr, cw);
            X86_STORE_AS(uint16_t, Volatile, addr + 4, (sw & ~0x3800) | (top << 11));
            X86_STORE_AS(uint16_t, Volatile, addr + 8, uint16_t(tw));

            // Save registers in stack order (ST(0) first)
            for (int i = 0; i < 8; i++)
            {
                // Convert long double to 80-bit extended precision
                if constexpr (Volatile)
                    X86_STORE_F80(addr + 28 + i * 10, st[(top + i) & 7]);
                else
                    X86_NONVOLATILE_STORE_F80(addr + 28 + i * 10, st[(top + i) & 7]);
            }
        }
        else
        {
            X86_STORE_AS(uint16_t, Volatile, addr, cw);
            X86_STORE_AS(uint16_t, Volatile, addr + 2, (sw & ~0x3800) | (top << 11));
            X86_STORE_AS(uint16_t, Volatile, addr + 4, uint16_t(tw));

            // Save registers in stack order (ST(0) first)
            for (int i = 0; i < 8; i++)
            {
                // Convert long double to 80-bit extended precision
                if constexpr (Volatile)
                    X86_STORE_F80(addr + 14 + i * 10, st[(top + i) & 7]);
                else
                    X86_NONVOLATILE_STORE_F80(addr + 14 + i * 10, st[(top + i) & 7]);
            }
        }
    }

    template<bool Volatile = true>
    void load(uint32_t addr, uint8_t* base, bool is32bits)
    {
        if (is32bits)
        {
            cw = X86_LOAD_AS(uint16_t, Volatile, addr);
            sw = X86_LOAD_AS(uint16_t, Volatile, addr + 4);
            tw = X86_LOAD_AS(uint16_t, Volatile, addr + 8);
            top = (sw >> 11) & 7;

            // Load registers in stack order (ST(0) first)
            for (int i = 0; i < 8; i++)
            {
                // Convert long double to 80-bit extended precision
                if constexpr (Volatile)
                    st[(top + i) & 7] = X86_LOAD_F80(addr + 28 + i * 10);
                else
                    st[(top + i) & 7] = X86_NONVOLATILE_LOAD_F80(addr + 28 + i * 10);
            }
        }
        else
        {
            cw = X86_LOAD_AS(uint16_t, Volatile, addr);
            sw = X86_LOAD_AS(uint16_t, Volatile, addr + 2);
            tw = X86_LOAD_AS(uint16_t, Volatile, addr + 4);
            top = (sw >> 11) & 7;

            // Load registers in stack order (ST(0) first)
            for (int i = 0; i < 8; i++)
            {
                // Convert long double to 80-bit extended precision
                if constexpr (Volatile)
                    st[(top + i) & 7] = X86_LOAD_F80(addr + 14 + i * 10);
                else
                    st[(top + i) & 7] = X86_NONVOLATILE_LOAD_F80(addr + 14 + i * 10);
            }
        }
    }

    X86_FORCEINLINE void flag_invalid() { sw |= C0 | C2; }  // For NaN/Invalid ops
    X86_FORCEINLINE void flag_divzero() { sw |= C0 | C3; }  // For x/0 cases
    X86_FORCEINLINE void flag_stackfault() { sw |= C0 | C1; }  // Stack overflow/underflow

    X86_FORCEINLINE void clear_flags()
    {
        sw &= ~(C0 | C1 | C2 | C3);
    }

    X86_FORCEINLINE void set_unordered() {
        sw |= C0 | C2 | C3; // Set for NaN comparisons
    }

    X86_FORCEINLINE void set_compare_flags(bool less, bool equal)
    {
        sw = (sw & ~(C0 | C1 | C2 | C3)) | (uint16_t(less) * C0) |
            (uint16_t(!less & equal) * C3);
    }


    X86_FORCEINLINE void set_precision_flag()
    {
        sw |= (1 << 5); // PE
    }

    X86_FORCEINLINE void set_exception_flags(x87_float result)
    {
        // A finite result sets no flags here.
        // Xbox games mask all FPU exceptions, ES not set.
        if (__builtin_expect(std::isfinite(result), 1))
            return;
        constexpr uint16_t FLAGS_INVALID = 0x01;   // IE bit (bit 0)
        constexpr uint16_t FLAGS_OVERFLOW = 0x08;  // OE bit (bit 3)
        sw |= std::isnan(result) ? FLAGS_INVALID : FLAGS_OVERFLOW;

        // Set ES (Exception Summary) bit if any unmasked exceptions
        uint16_t unmasked = (sw & 0x3F) & ~(cw & 0x3F);
        if (unmasked) {
            sw |= 0x80;
        }
    }


    void handle_exceptions()
    {
        const uint16_t exceptions = sw & 0x3F; // Get exception flags

        if (exceptions & 0x01) { //Invalid operation
            sw |= C0; // Set condition code
#if X86_RECOMP_DEBUG
            if (cw & 0x01)
            {
                printf("FPU Invalid Operation\n");
            }
#endif
        }

        if (exceptions & 0x04) { // Division by zero
            sw |= C2; // Set condition code
#if X86_RECOMP_DEBUG
            if (cw & 0x04)
            {
                printf("FPU Divide by zero\n");
            }
#endif
        }
    }

    // Rounds with the x87 rounding mode. Out of range gives INT_MIN.
    template<typename T>
    X86_FORCEINLINE T to_int(x87_float value) const
    {
        return to_int_rounded<T>(apply_rounding(value));
    }

    // Converts an already rounded value to T. Out of range gives INT_MIN.
    template<typename T>
    static X86_FORCEINLINE T to_int_rounded(x87_float rounded)
    {
#if defined(X86_HOST_X86)
        if constexpr (sizeof(x87_float) == 8 && sizeof(T) <= 4) {
            const int32_t result = _mm_cvttsd_si32(_mm_set_sd(rounded));
            if constexpr (sizeof(T) == 4)
                return result;
            return T(result) == result ? T(result) : std::numeric_limits<T>::min();
        }
#if defined(__x86_64__) || defined(_M_X64)
        if constexpr (sizeof(x87_float) == 8 && sizeof(T) == 8)
            return _mm_cvttsd_si64(_mm_set_sd(rounded));
#endif
#endif
        constexpr x87_float limit = -static_cast<x87_float>(std::numeric_limits<T>::min());
        if (__builtin_expect(!(rounded >= -limit && rounded < limit), 0))
            return std::numeric_limits<T>::min();
        return static_cast<T>(rounded);
    }

    // FISTP: converts ST(0) to T.
    // Status = false is the fast_x87_status speed hack.
    template<typename T, bool Status = true>
    X86_FORCEINLINE T fist()
    {
        if constexpr (!Status)
            return to_int<T>(st0());

        const x87_float value = st0();
        const x87_float rounded = apply_rounding(value);
        const T result = to_int_rounded<T>(rounded);
        // INT_MIN itself is in range, so check the rounded value.
        if (__builtin_expect(result == std::numeric_limits<T>::min(), 0) &&
            rounded != static_cast<x87_float>(result)) {
            // Out of range: IE only, and C1 = 0.
            sw = uint16_t((sw & ~C1) | 0x01);
            return result;
        }
        set_rounding_status(value, rounded);
        return result;
    }

    // FRNDINT: rounds ST(0) to an integer.
    // Status = false is the fast_x87_status speed hack.
    template<bool Status = true>
    X86_FORCEINLINE void frndint()
    {
        const x87_float value = st0();
        const x87_float rounded = apply_rounding(value);
        st0() = rounded;
        if constexpr (Status) {
            set_rounding_status(value, rounded);
            if (__builtin_expect(__builtin_issignaling(value), 0))
                sw |= 0x01;
        }
    }

    X86_FORCEINLINE void set_rounding_status(x87_float value, x87_float rounded)
    {
        const bool up = __builtin_isgreater(std::fabs(rounded), std::fabs(value));
        const bool inexact = __builtin_islessgreater(rounded, value);
        sw = uint16_t((sw & ~C1) | (uint16_t(up) << 9) | (uint16_t(inexact) << 5));
    }

    X86_FORCEINLINE x87_float apply_rounding(x87_float value) const
    {
        const unsigned mode = (cw >> 10) & 0x3;
        // Round to nearest is the MSVC CRT's default. So apply 1st.
        if (__builtin_expect(mode == 0, 1))
            return round_to_even(value);

        switch (mode) {
        case 0x1: return std::floor(value); // Round down
        case 0x2: return std::ceil(value);  // Round up
        default: return std::trunc(value);  // Truncate
        }
    }

    // Nearest integer to 'arg'.
    static X86_FORCEINLINE x87_float round_to_even(x87_float value)
    {
#if defined(__SSE4_1__) || defined(__aarch64__)
        if constexpr (sizeof(x87_float) == 8)
            return __builtin_roundeven(value);
#endif
        return round_to_even_slow(value);
    }

    // Out of line: one call per site, like the libm call it replaces.
    static __attribute__((noinline)) x87_float round_to_even_slow(x87_float value)
    {
        const x87_float down = std::floor(value);
        const x87_float fraction = value - down;
        if (fraction > 0.5 || (fraction == 0.5 && std::fmod(down, 2.0) != 0))
            return std::copysign(down + 1, value);
        return std::copysign(down, value);
    }

    static X86_FORCEINLINE bool is_nan(x87_float value)
    {
#if defined(X86_HOST_X86) || defined(__aarch64__)
        // Fastest impl x86 and fewest on arm64.
        if constexpr (sizeof(x87_float) == 8) {
            uint64_t bits;
            memcpy(&bits, &value, sizeof(bits));
            return (bits << 1) > 0xFFE0000000000000ull;
        }
#endif
        return std::isnan(value);
    }

    template<bool Status>
    X86_FORCEINLINE void flag_unordered_invalid(x87_float lhs, x87_float rhs, bool ordered)
    {
        if constexpr (Status) {
            if (ordered || __builtin_issignaling(lhs) || __builtin_issignaling(rhs))
                sw |= 0x01; // IE flag
        }
    }

    template<bool Status = true, bool Checked = true>
    X86_FORCEINLINE void compare_and_pop(x87_float rhs, int pops = 0, bool ordered = true)
    {
        const x87_float lhs = st0();
        clear_flags();

        // Fewer insns on ARMv7
#if defined(__arm__)
        if (__builtin_expect(is_nan(lhs) | is_nan(rhs), 0)) {
#else
        if (__builtin_expect(is_nan(lhs) || is_nan(rhs), 0)) {
#endif
            sw |= C0 | C2 | C3;
            flag_unordered_invalid<Status>(lhs, rhs, ordered);
        }
        else if (__builtin_isless(lhs, rhs))
            sw |= C0;  // ST0 < rhs
        else if (!__builtin_isgreater(lhs, rhs))
            sw |= C3;  // ST0 == rhs
        // ST0 > rhs: no flags set (C3=0, C2=0, C0=0)

        while (pops--)
            pop<Checked>();
    }

    // OF, SF and AF are cleared too, so common is written whole.
    template<bool Status = true>
    X86_FORCEINLINE void compare_and_set_eflags(x87_float lhs, x87_float rhs, X86EFlags& eflags,
                                                bool ordered = true)
    {
#if defined(__arm__)
        if (__builtin_expect(is_nan(lhs) | is_nan(rhs), 0)) { // Unordered
#else
        if (__builtin_expect(is_nan(lhs) || is_nan(rhs), 0)) { // Unordered
#endif
            eflags.common = X86EFlags::ZF | X86EFlags::PF | X86EFlags::CF;
            flag_unordered_invalid<Status>(lhs, rhs, ordered);
            return;
        }
        eflags.common = __builtin_isless(lhs, rhs) ? X86EFlags::CF :
                        (__builtin_isgreater(lhs, rhs) ? 0 : X86EFlags::ZF);
    }

    template<bool Status, uint32_t Taken>
    X86_FORCEINLINE bool branch_fcomi(x87_float lhs, x87_float rhs, X86EFlags& eflags,
                                      bool ordered = true)
    {
        if constexpr (Status) {
            if (__builtin_expect(is_nan(lhs) || is_nan(rhs), 0)) {
                compare_and_set_eflags<Status>(lhs, rhs, eflags, ordered);
                return true; // <- unordered sets CF, ZF and PF
            }
        }
        return x86_float_branch<Taken>(lhs, rhs);
    }

};

// --- x87 optimization ---
//
// Intrinsics related to the `optimize_x87` config flag.
// See `x87.md`.

// --- x87 optimization: Assumptions ---

// X87_ASSUME(c): tells the compiler `c` holds.
// Define X87_ASSUME_CHECK to check it at runtime instead.
// Can be helpful for debugging.
#ifdef X87_ASSUME_CHECK
extern "C" [[noreturn]] void x87_assume_failed(const char* function, int line);

// Hides the value from the compiler.
// Prevent deletion of this check at runtime.
// Because typical compiler wouldn't nuke out asm.
//
// This is kinda similar to `black_box` function in Criterion (Rust).
template <typename T>
X86_FORCEINLINE T x87_opaque(T value)
{
    asm volatile("" : "+r"(value));
    return value;
}
#define X87_ASSUME(c) do { if (!x87_opaque(bool(c))) x87_assume_failed(__func__, __LINE__); } while (0)
#elif defined(__clang__)
#define X87_ASSUME(c) __builtin_assume(c)
#elif defined(_MSC_VER)
#define X87_ASSUME(c) __assume(c)
#else
#define X87_ASSUME(c) do { if (!(c)) X86_UNREACHABLE(); } while (0)
#endif

// Reads `sw` with a 16-bit load.
//
// Clang may otherwise use a 4-byte load, which hurts perf a bit due to read stalls.
// It cost about 2 ns per call in `sub_30ECB0`.
X86_FORCEINLINE uint16_t x87_load_u16(const uint16_t* field)
{
    uint16_t value;
    memcpy(&value, field, sizeof(value));
    asm("" : "+r"(value)); // <- keeps the load 16-bit
    return value;
}

// Builds the 8 x87 tag bytes for a stack holding `Live` values.
// Each byte is 0 if its slot holds a value, or 3 if the slot is empty.
//
// See X87FPU->tw.tag
template <int Live>
constexpr uint64_t x87_tags()
{
    constexpr int top = (8 - Live) & 7;
    uint64_t tags = 0;
    for (int k = 0; k < 8; ++k)
        tags |= uint64_t(((k - top) & 7) < Live ? 0 : 3) << (8 * k);

    return tags;
}

// Tells the compiler.
//
// Hey fella! We have `live` values on the stack right meow.
#define X87_ASSUME_DEPTH(fpu, live) do { \
    constexpr uint64_t x87_tags_ = x87_tags<live>(); \
    X87_ASSUME((fpu).top == ((8 - (live)) & 7)); \
    X87_ASSUME((fpu).tw.tag[0] == uint8_t(x87_tags_)); \
    X87_ASSUME((fpu).tw.tag[1] == uint8_t(x87_tags_ >> 8)); \
    X87_ASSUME((fpu).tw.tag[2] == uint8_t(x87_tags_ >> 16)); \
    X87_ASSUME((fpu).tw.tag[3] == uint8_t(x87_tags_ >> 24)); \
    X87_ASSUME((fpu).tw.tag[4] == uint8_t(x87_tags_ >> 32)); \
    X87_ASSUME((fpu).tw.tag[5] == uint8_t(x87_tags_ >> 40)); \
    X87_ASSUME((fpu).tw.tag[6] == uint8_t(x87_tags_ >> 48)); \
    X87_ASSUME((fpu).tw.tag[7] == uint8_t(x87_tags_ >> 56)); \
} while (0)

// Copy local `fpu` -> `ctx.fpu`.
// With `Live` values on the stack.
//
// This is used to 'sync' before a `call`, `return` or `hook`.
// Also writes `sw` while at it, in case status bits are read.
// `cw` is skipped, called function doesn't need it.
template <int Live>
X86_FORCEINLINE void x87_out(X87FPU& dst, const X87FPU& src)
{
    constexpr uint16_t top = (8 - Live) & 7;
    constexpr uint64_t tags = x87_tags<Live>();
    dst.top = top;
    dst.sw = uint16_t((src.sw & ~0x3800) | (top << 11));
    memcpy(dst.tw.tag, &tags, sizeof(tags));
    for (int k = 0; k < Live; ++k)
        dst.st[(top + k) & 7] = src.st[(top + k) & 7];
}

// Copy local `ctx.fpu` -> `fpu`.
// With `Live` values on the stack.
//
// Inverse of `x87_out`.
// Note: Compiler can (in theory) drop loads for `cw`, `sw` etc if they're
//       never read. I've not validated if this happens, however.
template <int Live>
X86_FORCEINLINE void x87_in(X87FPU& dst, const X87FPU& src)
{
    constexpr uint16_t top = (8 - Live) & 7;
    constexpr uint64_t tags = x87_tags<Live>();
    dst.top = top;
    memcpy(dst.tw.tag, &tags, sizeof(tags));
    dst.cw = src.cw;
    dst.sw = uint16_t((x87_load_u16(&src.sw) & ~0x3800) | (top << 11));
    for (int k = 0; k < Live; ++k)
        dst.st[(top + k) & 7] = src.st[(top + k) & 7];
}

// Copies `ctx.fpu` at function entry.
// `sw` is read with `x87_load_u16`.
X86_FORCEINLINE X87FPU x87_copy(const X87FPU& src)
{
    X87FPU dst = src;
    dst.sw = x87_load_u16(&src.sw);
    return dst;
}

// Variant of `x87_in` for `if (Hook(ctx, base))`; when you need to reload
// between the hook and branch.
template <int Live>
X86_FORCEINLINE bool x87_after_hook(bool taken, X87FPU& dst, const X87FPU& src)
{
    x87_in<Live>(dst, src);
    return taken;
}


union XmmRegister {
    __m128 reg;
    float f32[4];
    uint32_t u32[4];
    uint64_t u64[2];
};

struct X86Context
{

    X86Register eax; //Accumulator
    X86Register ebx; //Base
    X86Register ecx; //Counter
    X86Register edx; //Data
    X86Register esi; //Source Index
    X86Register edi; //Destination Index
    X86Register ebp; //Base Pointer
    X86Register esp; //Stack Pointer

    //Segment registers (16-bit)
    uint16_t es = 0;  //Extra Segment
    uint16_t cs = 0;  //Code Segment
    uint16_t ss = 0;  //Stack Segment
    uint16_t ds = 0;  //Data Segment
    uint16_t fs = 0;  //Additional Segment
    uint16_t gs = 0;  //Additional Segment

//@context:extra-state

    // Flags Register
    X86EFlags eflags;

    XmmRegister xmm[8];

    uint32_t mxcsr = 0x1F80;
    X87FPU fpu;

    void AddEsp(uint8_t count)
    {
        esp.u32 += (4 * count);
    }

    void SubEsp(uint8_t count)
    {
        esp.u32 -= (4 * count);
    }

    void set_mxcsr(uint32_t value)
    {
        mxcsr = value & 0xFFC0; // Mask reserved bits
        _mm_setcsr(mxcsr);
    }

    uint32_t get_mxcsr() const
    {
        return _mm_getcsr();
    }

    X86_FORCEINLINE __m128 sqrt_ss(__m128 src)
    {
        const __m128 root = _mm_sqrt_ss(src);
        if (__builtin_expect(__builtin_isunordered(_mm_cvtss_f32(root), _mm_cvtss_f32(root)), 0)) {
            const float input = _mm_cvtss_f32(src);
            uint32_t bits;
            memcpy(&bits, &input, sizeof(bits));
            mxcsr |= (bits & 0x7FC00000) != 0x7FC00000;
        }
        return root;
    }

    // COMISS/UCOMISS. Sets eflags.
    //
    // - Signalling: true for COMISS, false for UCOMISS.
    // - Status: false is the fast_sse_status speed hack.
    //
    // Guest exceptions set MXCSR flags.
    template<bool Signalling, bool Status = true>
    X86_FORCEINLINE void compare_ss(float lhs, float rhs)
    {
        if constexpr (!Status) {
#if !defined(X86_HOST_X86)
            // Non-x86 hosts may have different denormal handling. Need to standardize.
            uint32_t left, right;
            memcpy(&left, &lhs, sizeof(left));
            memcpy(&right, &rhs, sizeof(right));
            if (__builtin_expect(((left | right) & 0x7FFFFFFF) - 1 < 0x007FFFFF, false)) {
                compare_ss_special<Signalling, false>(left, right);
                return;
            }
#endif
            eflags.update_compare_flags_sse(lhs, rhs);
            return;
        }

        // Handle NaNs and nonzero denormals.
        uint32_t left, right;
        memcpy(&left, &lhs, sizeof(left));
        memcpy(&right, &rhs, sizeof(right));
        const uint32_t a = left & 0x7FFFFFFF, b = right & 0x7FFFFFFF;
        if (__builtin_expect(a > 0x7F800000 || b > 0x7F800000 ||
                             a - 1 < 0x007FFFFF || b - 1 < 0x007FFFFF, false)) {
            compare_ss_special<Signalling>(left, right);
            return;
        }

        // Now do the damn comparison.
        eflags.common = !__builtin_islessgreater(lhs, rhs) ? X86EFlags::ZF :
                        (__builtin_isless(lhs, rhs) ? X86EFlags::CF : 0);
    }

    // Merged COMISS/UCOMISS + jcc. Returns true if the jcc is taken.
    // e.g. `if (ctx.branch_ss<...>(lhs, rhs)) goto loc;`
    //
    // - Signalling: true for COMISS, false for UCOMISS.
    // - Status: false is the fast_sse_status speed hack
    // - Taken: mask of the flags the jcc reads: CF, ZF, PF or CF | ZF.
    template<bool Signalling, bool Status, uint32_t Taken>
    X86_FORCEINLINE bool branch_ss(float lhs, float rhs)
    {
        static_assert(Taken == X86EFlags::CF || Taken == X86EFlags::ZF || Taken == X86EFlags::PF ||
                      Taken == (X86EFlags::CF | X86EFlags::ZF), "COMISS jcc reads CF, ZF or PF");
        uint32_t left, right;
        memcpy(&left, &lhs, sizeof(left));
        memcpy(&right, &rhs, sizeof(right));
        bool special;
        if constexpr (!Status) {
#if defined(X86_HOST_X86)
            special = false;
#else
            special = ((left | right) & 0x7FFFFFFF) - 1 < 0x007FFFFF;
#endif
        } else {
            const uint32_t a = left & 0x7FFFFFFF, b = right & 0x7FFFFFFF;
            special = a > 0x7F800000 || b > 0x7F800000 || a - 1 < 0x007FFFFF || b - 1 < 0x007FFFFF;
        }
        if (__builtin_expect(special, false)) {
            compare_ss<Signalling, Status>(lhs, rhs);
            return (eflags.common & Taken) != 0;
        }
        return x86_float_branch<Taken>(lhs, rhs);
    }

    // Merged COMISS/UCOMISS + jccs that read different flags, with fast_sse_status.
    // Returns CF, ZF and PF. e.g. `if ((flags & X86EFlags::CF) != 0) goto loc;`
    //
    // - Signalling: true for COMISS, false for UCOMISS.
    template<bool Signalling>
    X86_FORCEINLINE uint32_t compare_flags_ss(float lhs, float rhs)
    {
#if defined(X86_HOST_X86)
        return x86_float_flags(lhs, rhs);
#else
        compare_ss<Signalling, false>(lhs, rhs);
        return eflags.common;
#endif
    }

private:
    template<bool Signalling, bool Status = true>
    __attribute__((noinline, cold)) void compare_ss_special(uint32_t left, uint32_t right)
    {
        const uint32_t a = left & 0x7FFFFFFF, b = right & 0x7FFFFFFF;
        const bool unordered = a > 0x7F800000 || b > 0x7F800000;
        if (unordered) {
            const bool invalid = Signalling ||
                (a > 0x7F800000 && !(a & 0x00400000)) ||
                (b > 0x7F800000 && !(b & 0x00400000));
            if (Status && invalid) {
                mxcsr |= 1;
#if defined(X86_HOST_X86)
                float lhs, rhs;
                memcpy(&lhs, &left, 4); memcpy(&rhs, &right, 4);
                if constexpr (Signalling)
                    __asm__ volatile("comiss %1, %0" :: "x"(lhs), "x"(rhs) : "cc");
                else
                    __asm__ volatile("ucomiss %1, %0" :: "x"(lhs), "x"(rhs) : "cc");
#elif defined(__aarch64__)
                uint64_t status;
                __asm__ volatile("mrs %0, fpsr" : "=r"(status) :: "memory");
                __asm__ volatile("msr fpsr, %0" :: "r"(status | 1) : "memory");
#elif defined(__arm__) && defined(__ARM_FP)
                uint32_t status;
                __asm__ volatile("vmrs %0, fpscr" : "=r"(status) :: "memory");
                __asm__ volatile("vmsr fpscr, %0" :: "r"(status | 1) : "memory");
#endif
            }
            eflags.common = X86EFlags::ZF | X86EFlags::PF | X86EFlags::CF;
            return;
        }

        const bool tiny_left = a - 1 < 0x007FFFFF, tiny_right = b - 1 < 0x007FFFFF;
        if (Status && !(mxcsr & 0x40))
            mxcsr |= (tiny_left || tiny_right) ? 2 : 0;
#if defined(X86_HOST_X86)
        float lhs, rhs;
        memcpy(&lhs, &left, 4); memcpy(&rhs, &right, 4);
        eflags.common = !__builtin_islessgreater(lhs, rhs) ? X86EFlags::ZF :
                        (__builtin_isless(lhs, rhs) ? X86EFlags::CF : 0);
#else
        if (mxcsr & 0x40) {
            if (tiny_left) left &= 0x80000000;
            if (tiny_right) right &= 0x80000000;
        }
        // Integer ordering avoids ARM FZ flushing inputs when only guest FTZ is set.
        const bool equal = left == right || ((left | right) & 0x7FFFFFFF) == 0;
        const bool less = (left ^ right) & 0x80000000 ? (left >> 31) != 0 :
                          (left & 0x80000000 ? left > right : left < right);
        eflags.common = equal ? X86EFlags::ZF : (less ? X86EFlags::CF : 0);
#endif
    }

public:
    uint8_t* base; // Base address for memory access

    struct { //technically only used by xbox kernel stuff should't be needed 
        uint16_t limit;  // GDT size - 1
        uint32_t base;   // Linear address of GDT
    } gdtr = { 0, 0 };     // Initialize to zero
};


template<bool Volatile = true>
X86_FORCEINLINE void X86_PUSH(X86Context& __restrict ctx, uint8_t* base, uint32_t value)
{
    uint32_t new_esp = ctx.esp.u32 - 4;
    X86_STORE_AS(uint32_t, Volatile, new_esp, value);
    ctx.esp.u32 = new_esp;
}

template<bool Volatile = true>
X86_FORCEINLINE uint32_t X86_POP(X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t value = X86_LOAD_AS(uint32_t, Volatile, ctx.esp.u32);
    ctx.esp.u32 += 4;
    return value;
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_PUSH16(X86Context& __restrict ctx, uint8_t* base, uint16_t value)
{
    uint32_t new_esp = ctx.esp.u32 - 2;
    X86_STORE_AS(uint16_t, Volatile, new_esp, value);
    ctx.esp.u32 = new_esp;
}

template<bool Volatile = true>
X86_FORCEINLINE uint16_t X86_POP16(X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t value = X86_LOAD_AS(uint16_t, Volatile, ctx.esp.u32);
    ctx.esp.u32 += 2;
    return value;
}

// CONDITION CODE HELPER
inline bool X86_CC_O(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::OF) != 0; }
inline bool X86_CC_NO(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::OF) == 0; }
inline bool X86_CC_B(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::CF) != 0; }
inline bool X86_CC_NB(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::CF) == 0; }
inline bool X86_CC_Z(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::ZF) != 0; }
inline bool X86_CC_NZ(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::ZF) == 0; }
inline bool X86_CC_BE(const X86Context& __restrict ctx) { return (ctx.eflags.common & (X86EFlags::CF | X86EFlags::ZF)) != 0; }
inline bool X86_CC_NBE(const X86Context& __restrict ctx) { return (ctx.eflags.common & (X86EFlags::CF | X86EFlags::ZF)) == 0; }
inline bool X86_CC_S(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::SF) != 0; }
inline bool X86_CC_NS(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::SF) == 0; }
inline bool X86_CC_P(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::PF) != 0; }
inline bool X86_CC_NP(const X86Context& __restrict ctx) { return (ctx.eflags.common & X86EFlags::PF) == 0; }
inline bool X86_CC_L(const X86Context& __restrict ctx)
{
    const uint32_t flags = ctx.eflags.common;
    return ((flags ^ (flags >> 4)) & X86EFlags::SF) != 0;
}
inline bool X86_CC_NL(const X86Context& __restrict ctx)
{
    return !X86_CC_L(ctx);
}
inline bool X86_CC_LE(const X86Context& __restrict ctx)
{
    const uint32_t flags = ctx.eflags.common;
    return ((flags & X86EFlags::ZF) | ((flags ^ (flags >> 4)) & X86EFlags::SF)) != 0;
}
inline bool X86_CC_NLE(const X86Context& __restrict ctx)
{
    return !X86_CC_LE(ctx);
}
// end CC HELPER

// Note: Tick rate on host.
// On Xbox you will want to hook calls that use this because a specific rate is assumed
inline void X86_RDTSC(X86Context& __restrict ctx)
{
#if defined(X86_HOST_X86)
    // __rdtsc() joins EDX:EAX into one value just to split it again.
    __asm__ volatile("rdtsc" : "=a"(ctx.eax.u32), "=d"(ctx.edx.u32));
#else
#if defined(__aarch64__)
    uint64_t ticks;
    __asm__ volatile("mrs %0, cntvct_el0" : "=r"(ticks));
#else
    // Nanoseconds; ARMv7 user mode has no readable cycle counter.
    const uint64_t ticks = static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count());
#endif
    ctx.eax.u32 = static_cast<uint32_t>(ticks);
    ctx.edx.u32 = static_cast<uint32_t>(ticks >> 32);
#endif
}

// Port reads we'll stub as 1.
#ifndef X86_IN8
X86_FORCEINLINE uint8_t X86_IN8(X86Context& __restrict, uint16_t) { return 0xFF; }
#endif
#ifndef X86_IN16
X86_FORCEINLINE uint16_t X86_IN16(X86Context& __restrict, uint16_t) { return 0xFFFF; }
#endif
#ifndef X86_IN32
X86_FORCEINLINE uint32_t X86_IN32(X86Context& __restrict, uint16_t) { return 0xFFFFFFFF; }
#endif

// Port writes go nowhere by default.
#ifndef X86_OUT8
X86_FORCEINLINE void X86_OUT8(X86Context& __restrict, uint16_t, uint8_t) {}
#endif
#ifndef X86_OUT16
X86_FORCEINLINE void X86_OUT16(X86Context& __restrict, uint16_t, uint16_t) {}
#endif
#ifndef X86_OUT32
X86_FORCEINLINE void X86_OUT32(X86Context& __restrict, uint16_t, uint32_t) {}
#endif

// On x86, lock cmpxchg is a full barrier, so 'relaxed' is enough.
template<typename T>
X86_FORCEINLINE bool X86_ATOMIC_CMPXCHG(uint8_t* base, uintptr_t addr, T& expected, T desired)
{
#if defined(X86_HOST_X86) && !X86_NONVOLATILE_MEMORY
    auto* ptr = reinterpret_cast<volatile std::atomic<T>*>(base + addr);
    return std::atomic_compare_exchange_strong_explicit(ptr, &expected, desired, std::memory_order_relaxed,
                                                        std::memory_order_relaxed);
#else
    auto* ptr = reinterpret_cast<std::atomic<T>*>(base + addr);
    return std::atomic_compare_exchange_strong(ptr, &expected, desired);
#endif
}

// Flag Macros
#define SET_FLAG(flag, condition) (ctx.RFLAGS = (condition) ? (ctx.RFLAGS | (flag)) : (ctx.RFLAGS & ~(flag)))
#define GET_FLAG(flag) (ctx.RFLAGS & (flag))

// x86 Flags in RFLAGS
#define CARRY_FLAG     (1 << 0)
#define ZERO_FLAG      (1 << 6)
#define OVERFLOW_FLAG  (1 << 11)

// SIMD Helpers (if needed)
#if defined(X86_HOST_X86)
inline __m128i X86_ADD_EPU32(__m128i a, __m128i b) {
    return _mm_add_epi32(a, _mm_min_epu32(_mm_xor_si128(a, _mm_cmpeq_epi32(a, a)), b));
}

inline __m128i X86_AVG_EPU8(__m128i a, __m128i b) {
    return _mm_avg_epu8(a, b);
}

inline __m128i X86_AVG_EPU16(__m128i a, __m128i b)
{
    return _mm_avg_epu16(a, b);
}
#endif


inline uint64_t X86_PSHUFW(uint64_t src, uint8_t control)
{
    const uint16_t w0 = (src >> (0 * 16)) & 0xFFFF;
    const uint16_t w1 = (src >> (1 * 16)) & 0xFFFF;
    const uint16_t w2 = (src >> (2 * 16)) & 0xFFFF;
    const uint16_t w3 = (src >> (3 * 16)) & 0xFFFF;


    const uint16_t ws[4] = { w0, w1, w2, w3 };

    return (uint64_t)ws[control & 3] |  // Word 0
        ((uint64_t)ws[(control >> 2) & 3] << 16) |  // Word 1
        ((uint64_t)ws[(control >> 4) & 3] << 32) |  // Word 2
        ((uint64_t)ws[(control >> 6) & 3] << 48);   // Word 3
}

template<uint8_t Control>
inline uint64_t X86_PSHUFW(uint64_t src)
{
#if defined(X86_PACKED_SSE2)
    // PSHUFW == _mm_shuffle_pi16 (MMX) == _mm_shufflelo_epi16 (SSE2)
    const __m128i words = _mm_cvtsi64_si128(static_cast<int64_t>(src));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_shufflelo_epi16(words, Control)));
#elif defined(X86_PACKED_NEON)
    const uint8_t indices[8] = {
        2 * (Control & 3), 2 * (Control & 3) + 1,
        2 * ((Control >> 2) & 3), 2 * ((Control >> 2) & 3) + 1,
        2 * ((Control >> 4) & 3), 2 * ((Control >> 4) & 3) + 1,
        2 * ((Control >> 6) & 3), 2 * ((Control >> 6) & 3) + 1,
    };
    const uint8x8_t bytes = vreinterpret_u8_u64(vcreate_u64(src));
    return vget_lane_u64(vreinterpret_u64_u8(vtbl1_u8(bytes, vld1_u8(indices))), 0);
#else
    return X86_PSHUFW(src, Control);
#endif
}

//todo check if all these functions and their instructions are correct
template<int Index>
X86_FORCEINLINE uint32_t X86_PEXTRW(uint64_t value)
{
#if defined(X86_PACKED_NEON) && defined(__arm__)
    return vget_lane_u16(vreinterpret_u16_u64(vcreate_u64(value)), Index);
#else
    return (value >> (Index * 16)) & 0xFFFF;
#endif
}

inline uint64_t X86_PADDW(uint64_t src_val, uint64_t dst_val)
{
#if defined(X86_PACKED_SSE2)
    // PADDW == _mm_add_pi16 (MMX) == _mm_add_epi16 (SSE2)
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_add_epi16(dst, src)));
#elif defined(X86_PACKED_NEON)
    const uint16x4_t src = vreinterpret_u16_u64(vcreate_u64(src_val));
    const uint16x4_t dst = vreinterpret_u16_u64(vcreate_u64(dst_val));
    return vget_lane_u64(vreinterpret_u64_u16(vadd_u16(dst, src)), 0);
#else
    constexpr uint64_t high_bits = 0x8000800080008000ULL;
    return ((src_val & ~high_bits) + (dst_val & ~high_bits)) ^
        ((src_val ^ dst_val) & high_bits);
#endif
}

inline uint64_t X86_PSRAW(uint64_t shift_val, uint64_t dst_val)
{
    // PSRAW == _mm_sra_pi16 (MMX) == _mm_sra_epi16 (SSE2)
#if defined(X86_PACKED_NEON)
    // A negative count shifts right; counts above 15 act as 15.
    const int16x4_t words = vreinterpret_s16_u64(vcreate_u64(dst_val));
    const int16x4_t count = vdup_n_s16(int16_t(-int16_t(shift_val > 15 ? 15 : shift_val)));
    return vget_lane_u64(vreinterpret_u64_s16(vshl_s16(words, count)), 0);
#else
    const __m128i words = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i count = _mm_cvtsi64_si128(static_cast<int64_t>(shift_val));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_sra_epi16(words, count)));
#endif
}

inline uint64_t X86_PACKUSWB(uint64_t dst_val, uint64_t src_val)
{
    // PACKUSWB == _mm_packs_pu16 (MMX) == _mm_packus_epi16 (SSE2)
#if defined(X86_PACKED_NEON)
    const int16x4_t dst = vreinterpret_s16_u64(vcreate_u64(dst_val));
    const int16x4_t src = vreinterpret_s16_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u8(vqmovun_s16(vcombine_s16(dst, src))), 0);
#else
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    const __m128i words = _mm_unpacklo_epi64(dst, src);
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_packus_epi16(words, words)));
#endif
}

inline uint64_t X86_PAVGB(uint64_t dst_val, uint64_t src_val)
{
#if defined(X86_PACKED_SSE2)
    // PAVGB == _mm_avg_pu8 (MMX) == _mm_avg_epu8 (SSE2)
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_avg_epu8(dst, src)));
#elif defined(X86_PACKED_NEON)
    const uint8x8_t dst = vreinterpret_u8_u64(vcreate_u64(dst_val));
    const uint8x8_t src = vreinterpret_u8_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u8(vrhadd_u8(dst, src)), 0);
#else
    return (dst_val | src_val) - (((dst_val ^ src_val) & 0xFEFEFEFEFEFEFEFEULL) >> 1);
#endif
}

// Counts above 63 give 0.
inline uint64_t X86_PSRLQ(uint64_t dst_val, uint64_t count)
{
#if defined(X86_PACKED_SSE2)
    // PSRLQ == _mm_srl_si64 (MMX) == _mm_srl_epi64 (SSE2)
    // One vector shift is fastest on x86.
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i n = _mm_cvtsi64_si128(static_cast<int64_t>(count));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_srl_epi64(dst, n)));
#elif defined(X86_PACKED_NEON)
    // A negative count shifts right; -64 gives 0.
    const int64x1_t shift = vdup_n_s64(-int64_t(count > 63 ? 64 : count));
    return vget_lane_u64(vshl_u64(vcreate_u64(dst_val), shift), 0);
#else
    return count > 63 ? 0 : dst_val >> count;
#endif
}

inline uint64_t X86_PUNPCKLWD(uint64_t dst_val, uint64_t src_val)
{
#if defined(X86_PACKED_SSE2)
    // PUNPCKLWD == _mm_unpacklo_pi16 (MMX) == _mm_unpacklo_epi16 (SSE2)
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_unpacklo_epi16(dst, src)));
#elif defined(X86_PACKED_NEON)
    const uint16x4_t dst = vreinterpret_u16_u64(vcreate_u64(dst_val));
    const uint16x4_t src = vreinterpret_u16_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u16(vzip_u16(dst, src).val[0]), 0);
#else
    return (dst_val & 0xFFFF) | ((src_val & 0xFFFF) << 16) |
        ((dst_val & 0xFFFF0000) << 16) | ((src_val & 0xFFFF0000) << 32);
#endif
}

inline uint64_t X86_PUNPCKHWD(uint64_t dst_val, uint64_t src_val)
{
#if defined(X86_PACKED_SSE2)
    // PUNPCKHWD == _mm_unpackhi_pi16 (MMX).
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    const __m128i words = _mm_unpacklo_epi16(dst, src);
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_unpackhi_epi64(words, words)));
#elif defined(X86_PACKED_NEON)
    const uint16x4_t dst = vreinterpret_u16_u64(vcreate_u64(dst_val));
    const uint16x4_t src = vreinterpret_u16_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u16(vzip_u16(dst, src).val[1]), 0);
#else
    return X86_PUNPCKLWD(dst_val >> 32, src_val >> 32);
#endif
}

inline uint64_t X86_PUNPCKLDQ(uint64_t dst_val, uint64_t src_val)
{
#if defined(X86_PACKED_SSE2)
    // PUNPCKLDQ == _mm_unpacklo_pi32 (MMX) == _mm_unpacklo_epi32 (SSE2)
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_unpacklo_epi32(dst, src)));
#elif defined(X86_PACKED_NEON)
    const uint32x2_t dst = vreinterpret_u32_u64(vcreate_u64(dst_val));
    const uint32x2_t src = vreinterpret_u32_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u32(vzip_u32(dst, src).val[0]), 0);
#else
    return (dst_val & 0xFFFFFFFF) | ((src_val & 0xFFFFFFFF) << 32);
#endif
}

inline uint64_t X86_PUNPCKHDQ(uint64_t dst_val, uint64_t src_val)
{
#if defined(X86_PACKED_SSE2)
    // PUNPCKHDQ == _mm_unpackhi_pi32 (MMX).
    const __m128i dst = _mm_cvtsi64_si128(static_cast<int64_t>(dst_val));
    const __m128i src = _mm_cvtsi64_si128(static_cast<int64_t>(src_val));
    const __m128i dwords = _mm_unpacklo_epi32(dst, src);
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_unpackhi_epi64(dwords, dwords)));
#elif defined(X86_PACKED_NEON)
    const uint32x2_t dst = vreinterpret_u32_u64(vcreate_u64(dst_val));
    const uint32x2_t src = vreinterpret_u32_u64(vcreate_u64(src_val));
    return vget_lane_u64(vreinterpret_u64_u32(vzip_u32(dst, src).val[1]), 0);
#else
    return (dst_val >> 32) | (src_val & 0xFFFFFFFF00000000);
#endif
}

// Rounds with MXCSR; NaN and out of range give INT32_MIN.
inline uint64_t X86_CVTPS2PI(__m128 src)
{
#if defined(X86_HOST_X86)
    // CVTPS2PI == _mm_cvtps_pi32 (MMX) == _mm_cvtps_epi32 (SSE2) on the low two floats.
    return static_cast<uint64_t>(_mm_cvtsi128_si64(_mm_cvtps_epi32(_mm_movelh_ps(src, src))));
#elif defined(X86_PACKED_NEON) && defined(__aarch64__)
    // Round with the current mode, then convert.
    const float32x2_t rounded = vrndx_f32(vget_low_f32((float32x4_t)src));
    const uint32x2_t fits = vcalt_f32(rounded, vdup_n_f32(2147483648.0f));
    const int32x2_t words = vbsl_s32(fits, vcvt_s32_f32(rounded), vdup_n_s32(INT32_MIN));
    return vget_lane_u64(vreinterpret_u64_s32(words), 0);
#else
    return static_cast<uint64_t>(_mm_cvtm64_si64(_mm_cvtps_pi32(src)));
#endif
}

X86_FORCEINLINE int x87_exponent(x87_float value)
{
    if constexpr (sizeof(x87_float) == 8) {
        uint64_t bits;
        memcpy(&bits, &value, sizeof(bits));
        const int biased = int((bits >> 52) & 0x7FF);
        if (__builtin_expect(biased != 0, 1))
            return biased - 1023;
    }
    return std::ilogb(value);
}

template<bool Ieee>
X86_FORCEINLINE void x87_fprem(X87FPU& fpu)
{
    const x87_float dividend = fpu.st0();
    const x87_float divisor = fpu.st1();
    constexpr uint16_t QUOTIENT = X87FPU::C0 | X87FPU::C3 | X87FPU::C1;

    if (std::isnan(dividend) || std::isnan(divisor) || std::isinf(dividend) || divisor == 0) {
        fpu.sw &= ~(X87FPU::C1 | X87FPU::C2);
        if (!std::isnan(dividend))
            fpu.st0() = std::isnan(divisor) ? divisor : std::numeric_limits<x87_float>::quiet_NaN();
        return;
    }
    if (std::isinf(divisor) || dividend == 0) {
        fpu.sw &= ~(QUOTIENT | X87FPU::C2);
        return;
    }

    const int diff = x87_exponent(dividend) - x87_exponent(divisor);
    const bool partial = diff > 63;
    const x87_float step = partial ? std::ldexp(divisor, diff - (32 + diff % 32)) : divisor;

    int quotient;
    x87_float remainder = std::remquo(dividend, step, &quotient);
    uint32_t q = static_cast<uint32_t>(quotient < 0 ? -quotient : quotient);
    if ((!Ieee || partial) && remainder != 0 && std::signbit(remainder) != std::signbit(dividend)) {
        remainder += std::copysign(step, dividend);
        --q;
    }

    if (remainder == 0)
        remainder = std::copysign((x87_float)0, dividend);
    fpu.st0() = remainder;

    q = partial ? 0 : q & 7;
    fpu.sw = (fpu.sw & ~(QUOTIENT | X87FPU::C2)) | (partial ? X87FPU::C2 : 0) |
             ((q & 4) ? X87FPU::C0 : 0) | ((q & 2) ? X87FPU::C3 : 0) | ((q & 1) ? X87FPU::C1 : 0);
}

X86_FORCEINLINE void X87_FPREM(X87FPU& fpu)
{
    x87_fprem<false>(fpu);
}

X86_FORCEINLINE void X87_FPREM1(X87FPU& fpu)
{
    x87_fprem<true>(fpu);
}

// Classifies from the bits, so a denormal can't raise a host exception.
inline void X87_FXAM(X87FPU& fpu, int32_t top)
{
    const x87_float v = fpu.st[top];
    uint32_t bits;

    //Get the 2-bit tag for the current register
    if (fpu.tw.tag[top & 7] == 3) {
        bits = X87FPU::C3 | X87FPU::C0;  //Empty
    } else if constexpr (sizeof(x87_float) == 8) {
        //Classify based on value
        uint64_t raw;
        memcpy(&raw, &v, sizeof(raw));
        const uint64_t magnitude = raw << 1;
        if (magnitude == 0)
            bits = X87FPU::C3;  //Zero
        else if (magnitude < 0xFFE0000000000000ull)
            bits = X87FPU::C2;  //Normal
        else if (magnitude == 0xFFE0000000000000ull)
            bits = X87FPU::C2 | X87FPU::C0;  //Infinity
        else
            bits = X87FPU::C0;  //NaN
    } else {
        //Classify based on value
        uint64_t mantissa;
        uint16_t sign_exponent;
        memcpy(&mantissa, &v, 8);
        memcpy(&sign_exponent, reinterpret_cast<const uint8_t*>(&v) + 8, 2);
        const uint32_t exponent = sign_exponent & 0x7FFF;
        if (exponent == 0)
            bits = mantissa == 0 ? X87FPU::C3 : X87FPU::C3 | X87FPU::C2;  //Zero or denormal
        else if (!(mantissa >> 63))
            bits = 0;  //Unsupported: no integer bit
        else if (exponent != 0x7FFF)
            bits = X87FPU::C2;  //Normal
        else
            bits = (mantissa << 1) == 0 ? X87FPU::C2 | X87FPU::C0 : X87FPU::C0;  //Infinity or NaN
    }

    //is negative (empty registers too)
    if (std::signbit(v))
        bits |= X87FPU::C1;
    fpu.sw = uint16_t((fpu.sw & ~(X87FPU::C0 | X87FPU::C1 | X87FPU::C2 | X87FPU::C3)) | bits);
}

// Status and Checked follow fast_x87_status and fast_x87_stack.
template<bool Status = true, bool Checked = true>
X86_FORCEINLINE void X87_FDIVP(X87FPU& fpu, int index, uint32_t addr)
{
    // FDIVP ST(i), ST(0): ST(i) = ST(i) / ST(0), then pop ST(0)
    x87_float& st1 = fpu.getST(index);
    x87_float& st0 = fpu.st0();

    if constexpr (!Status)
    {
        st1 = st1 / st0;
    }
    else if (st0 == X87FPU::ZERO)
    {
        if (st1 == X87FPU::ZERO) {
            fpu.sw |= 0x01;  // IE flag - 0/0
        }
        else {
            fpu.sw |= 0x04;  // ZE flag - non-zero/0
        }

        st1 = st1 / st0;
    }
    else
    {
        x87_float result = st1 / st0;

        // Check for overflow/underflow
        if constexpr (sizeof(x87_float) == sizeof(uint64_t)) {
            uint64_t magnitude;
            memcpy(&magnitude, &result, sizeof(magnitude));
            magnitude <<= 1;
            if (magnitude - 1 < 0x001FFFFFFFFFFFFFull) {
                fpu.sw |= 0x10;  // UE flag - Underflow
            }
            else if (magnitude == 0xFFE0000000000000ull) {
#if X86_RECOMP_DEBUG
                printf("WARNING, INF NUMBER !!!\n%x", addr);
#endif
                fpu.sw |= 0x08;  // OE flag - Overflow
            }
        }
        else if (__builtin_expect(!std::isnormal(result), 0)) {
            if (std::fpclassify(result) == FP_SUBNORMAL) {
                fpu.sw |= 0x10;  // UE flag - Underflow
            }
            else if (std::isinf(result)) {
#if X86_RECOMP_DEBUG
                printf("WARNING, INF NUMBER !!!\n%x", addr);
#endif
                fpu.sw |= 0x08;  // OE flag - Overflow
            }
        }

        st1 = result;
        fpu.set_precision_flag();
    }

    fpu.pop<Checked>();
}

#if X86_RECOMP_DEBUG
inline void X87_ISDIV_VALID(x87_float A, x87_float B, uint32_t addr)
{
    x87_float& st0 = A;
    x87_float& st1 = B;


    if (st0 == 0.0L || st1 == 0.0L)
    {
    }
    else
    {
        x87_float result = st0 / st1;

        // Check for overflow/underflow
        if (std::isinf(result))
        {
            printf("WARNING, DIVISION RESULT IS INF!!! (%Lf / %Lf) %x\n", A, B, addr);
        }

    }
}

#endif

inline x87_float X87_FDIVR(x87_float res, x87_float A, x87_float B, uint32_t addr)
{
#if X86_RECOMP_DEBUG
    X87_ISDIV_VALID(A, B, addr);
#endif

    res = A / B;

    return res;
}

#if defined(__clang__) && defined(X86_HOST_X86)
X86_FORCEINLINE x87_float X87_SQRT(x87_float value) { return __builtin_elementwise_sqrt(value); }
#elif defined(__clang__) && defined(__aarch64__)
// ARM's default NaN is positive (according to QEMU), negatives need a branch.
X86_FORCEINLINE x87_float X87_SQRT(x87_float value)
{
    return value < 0.0 ? -std::numeric_limits<x87_float>::quiet_NaN() : __builtin_elementwise_sqrt(value);
}
#elif defined(__clang__)
X86_FORCEINLINE x87_float X87_SQRT(x87_float value) { return std::copysign(__builtin_elementwise_sqrt(value), value); }
#else
X86_FORCEINLINE x87_float X87_SQRT(x87_float value) { return std::copysign(std::sqrt(value), value); }
#endif

// FSQRT. 
// Status = false -> fast_x87_status speed hack.
template<bool Status = true>
X86_FORCEINLINE void X87_FSQRT(X87FPU& fpu)
{
    const x87_float value = fpu.st0();
    fpu.st0() = X87_SQRT(value);
    if constexpr (Status) {
        if (value < 0.0 || __builtin_issignaling(value))
            fpu.sw |= 0x01;
        else
            fpu.set_precision_flag();
    }
}

// F2XM1 with fast_trig: computes 2^x - 1 in double.
// Accurate to 1 unit in last place (ulp) of x87.
//
// Ripped from EMULV in glibc's dla.h:
// https://github.com/bminor/glibc/blob/glibc-2.39/sysdeps/ieee754/dbl-64/dla.h#L63-L78
//
// Truth be told, I have no idea how this fully works.
// I only know a plain `value * ln(2)` was 2 ulp off, and this isn't.
//
// Is there really no math library for this?
X86_FORCEINLINE double X87_F2XM1(double value)
{
    constexpr double LN2_HI = 0x1.62e42fefa39efp-1, LN2_LO = 0x1.abc9e3b39803fp-56;
    const double high = value * LN2_HI;

    // Same steps as EMULV.
    constexpr double SPLIT = 134217729.0; // 2^27 + 1
    const double value_scaled = SPLIT * value;
    const double value_hi = value_scaled - (value_scaled - value), value_lo = value - value_hi;
    const double ln2_scaled = SPLIT * LN2_HI;
    const double ln2_hi = ln2_scaled - (ln2_scaled - LN2_HI), ln2_lo = LN2_HI - ln2_hi;

    const double rounding_error =
        ((value_hi * ln2_hi - high) + value_hi * ln2_lo + value_lo * ln2_hi) + value_lo * ln2_lo;
    const double low = rounding_error + value * LN2_LO;

    const double result = std::expm1(high);
    if (!(std::fabs(low) > 0))
        return result;

    return result + low * (result + 1.0);
}

// FSCALE when |ST(1)| >= 1023, or ST(1) is Inf or NaN.
template<bool Status>
__attribute__((noinline, cold)) void x87_fscale_special(X87FPU& fpu)
{
    x87_float& value = fpu.st0();
    const x87_float scale = std::trunc(fpu.st1());

    // A NaN operand gives that NaN, silenced mere mortal.
    if (scale != scale || value != value) {
        if constexpr (Status)
            if (__builtin_issignaling(value) || __builtin_issignaling(fpu.st1()))
                fpu.sw |= 0x01;
        if constexpr (sizeof(x87_float) == 8) {
            // With two NaNs x87 keeps the larger significand.
            if (value != value && scale != scale) {
                uint64_t a, b;
                memcpy(&a, &value, sizeof(a));
                memcpy(&b, &scale, sizeof(b));
                const uint64_t a_mantissa = a & 0x7FFFFFFFFFFFFull, b_mantissa = b & 0x7FFFFFFFFFFFFull;
                a = a_mantissa != b_mantissa ? (a_mantissa > b_mantissa ? a : b) : (a & b);
                a |= 0x7FF8000000000000ull;
                memcpy(&value, &a, sizeof(a));
                return;
            }
        }
        value = value + scale;
        return;
    }

    // 0 * 2^+Inf and Inf * 2^-Inf are invalid and give x87's default NaN.
    if (std::isinf(scale) && (scale > 0 ? value == 0 : std::isinf(value))) {
        if constexpr (Status)
            fpu.sw |= 0x01;
        value = -std::numeric_limits<x87_float>::quiet_NaN();
        return;
    }

    // 2^20000 overflows any format.
    value = std::ldexp(value, static_cast<int>(std::fmax(std::fmin(scale, 20000.0), -20000.0)));
    if constexpr (Status)
        fpu.set_precision_flag();
}

// FSCALE: ST(0) * 2^ST(1), with ST(1) truncated toward zero.
//
// Status = false is the fast_x87_status speed hack. 
// Exact = false the fast_fscale one.
template<bool Status = true, bool Exact = true>
X86_FORCEINLINE void X87_FSCALE(X87FPU& fpu)
{
    if constexpr (sizeof(x87_float) == 8) {
        const x87_float scale = fpu.st1();
        if constexpr (!Exact) {
            const x87_float clamped = scale < 1023.0 ? (scale > -1022.0 ? scale : -1022.0) : 1023.0;
            const uint64_t bits = uint64_t(static_cast<int>(clamped) + 1023) << 52;
            double power;
            memcpy(&power, &bits, sizeof(power));
            fpu.st0() = fpu.st0() * power;
            if constexpr (Status)
                fpu.set_precision_flag();
            return;
        }
        if (__builtin_expect(std::fabs(scale) < 1023.0 && (!Status || fpu.st0() == fpu.st0()), 1)) {
            const uint64_t bits = uint64_t(static_cast<int>(scale) + 1023) << 52;
            double power;
            memcpy(&power, &bits, sizeof(power));
            fpu.st0() = fpu.st0() * power;
            if constexpr (Status)
                fpu.set_precision_flag();
            return;
        }
    }
    x87_fscale_special<Status>(fpu);
}

X86_FORCEINLINE bool X87_TRIG_OUT_OF_RANGE(x87_float value)
{
    if constexpr (sizeof(x87_float) == 8) {
        // TODO: Clean this up later.
        uint64_t bits;
        memcpy(&bits, &value, sizeof(bits));
        return ((bits >> 52) & 0x7FF) - 0x43Eu < 0x7FFu - 0x43Eu;
    }
    return std::fabs(value) >= 0x1p63L && !std::isinf(value);
}

template<bool Checked = true, typename T>
X86_FORCEINLINE void X87_FSINCOS(X87FPU& fpu, T value)
{
#if defined(__GLIBC__)
    // glibc's sincos gives the same bits as sin and cos, in one call.
    if constexpr (sizeof(T) == sizeof(double) && sizeof(x87_float) == sizeof(double)) {
        double sine, cosine;
        __builtin_sincos(value, &sine, &cosine);
        fpu.st0() = sine;
        fpu.push<Checked>(cosine);
        return;
    }
#endif
    const x87_float sine = std::sin(value);
    const x87_float cosine = std::cos(value);
    fpu.st0() = sine;
    fpu.push<Checked>(cosine);
}

inline x87_float X87_FIDIV_16(x87_float st0, int16_t B, uint32_t addr)
{
    return (st0 / (x87_float)B);
}

inline x87_float X87_FIDIV_32(x87_float st0, int32_t B, uint32_t addr)
{
    return (st0 / (x87_float)B);
}

inline x87_float X87_FIDIV_REG(x87_float st0, x87_float B, uint32_t addr)
{
    return (st0 / B);
}

// Rotates the (Bits + 1)-bit CF:val right.
template<unsigned Bits>
X86_FORCEINLINE uint32_t x86_rcr(uint32_t val, uint32_t count, X86EFlags& eflags)
{
    const uint32_t masked = count & 0x1F;
    if (masked == 0) return val;
    const uint32_t shift = masked % (Bits + 1);

    const uint32_t old_cf = eflags.common & X86EFlags::CF;

    // The 64-bit form (uint64_t) is fastest on x86.
    uint32_t result;
    uint32_t new_cf;
    if constexpr (Bits == 32) {
        uint64_t wide = ((uint64_t)old_cf << 32) | val;
        wide = (wide >> shift) | (wide << (33 - shift));
        result = static_cast<uint32_t>(wide);
        new_cf = (wide >> 32) & 1;
    } else {
        uint32_t wide = (old_cf << Bits) | val;
        wide = (wide >> shift) | (wide << (Bits + 1 - shift));
        result = wide & ((1u << Bits) - 1);
        new_cf = (wide >> Bits) & 1;
    }

    // Update CF
    if (new_cf)
        eflags.common |= X86EFlags::CF;
    else
        eflags.common &= ~X86EFlags::CF;

    // OF uses val and CF from before the rotate.
    uint32_t of = ((val >> (Bits - 1)) & 1) ^ old_cf;
    eflags.common = (eflags.common & ~X86EFlags::OF) | (of << 11);

    return result;
}

X86_FORCEINLINE uint32_t X86_RCR32(uint32_t val, uint8_t shift, X86EFlags& eflags)
{
    return x86_rcr<32>(val, shift, eflags);
}

X86_FORCEINLINE uint16_t X86_RCR16(uint16_t val, uint8_t shift, X86EFlags& eflags)
{
    return (uint16_t)x86_rcr<16>(val, shift, eflags);
}

X86_FORCEINLINE uint8_t X86_RCR8(uint8_t val, uint8_t shift, X86EFlags& eflags)
{
    return (uint8_t)x86_rcr<8>(val, shift, eflags);
}

inline void X86_DIV8(uint32_t divisor, X86Context& __restrict ctx)
{
    // Plain form:
    // if (divisor_ == 0 || dividend_ / divisor_ > UINT8_MAX)
    //     return;
    const uint8_t divisor_ = static_cast<uint8_t>(divisor);
    if (__builtin_expect(ctx.eax.u8_hi >= divisor_, 0))
        return;

    const uint16_t dividend_ = ctx.eax.u16;
    ctx.eax.u8_lo = static_cast<uint8_t>(dividend_ / divisor_);
    ctx.eax.u8_hi = static_cast<uint8_t>(dividend_ % divisor_);
}

inline void X86_DIV16(uint32_t divisor, X86Context& __restrict ctx)
{
    // Plain form:
    // if (divisor_ == 0 || dividend_ / divisor_ > UINT16_MAX)
    //     return;
    const uint16_t divisor_ = static_cast<uint16_t>(divisor);
    if (__builtin_expect(ctx.edx.u16 >= divisor_, 0))
        return;

    const uint32_t dividend_ = (static_cast<uint32_t>(ctx.edx.u16) << 16) | ctx.eax.u16;
    ctx.eax.u16 = static_cast<uint16_t>(dividend_ / divisor_);
    ctx.edx.u16 = static_cast<uint16_t>(dividend_ % divisor_);
}

inline void X86_DIV32(uint32_t divisor, X86Context& __restrict ctx)
{
    // Plain form:
    // if (divisor == 0 || dividend_ / divisor > UINT32_MAX)
    //     return;
    uint32_t high = ctx.edx.u32;
    if (__builtin_expect(high >= divisor, 0))
        return;

    // Found a faster way on x86, but it'll limit your compiler choice a bit. soz
#if defined(X86_HOST_X86)
    uint32_t low = ctx.eax.u32;
    __asm__("divl %[divisor]" : "+a"(low), "+d"(high) : [divisor] "r"(divisor) : "cc");
    ctx.eax.u32 = low;
    ctx.edx.u32 = high;
#else
    const uint64_t dividend_ = (static_cast<uint64_t>(high) << 32) | ctx.eax.u32;
    ctx.eax.u32 = static_cast<uint32_t>(dividend_ / divisor);
    ctx.edx.u32 = static_cast<uint32_t>(dividend_ % divisor);
#endif
}

inline void X86_IDIV8(uint32_t divisor, X86Context& __restrict ctx)
{
    if (__builtin_expect(static_cast<int8_t>(divisor) == 0, 0))
        return;

    int16_t dividend = static_cast<int16_t>(ctx.eax.u16);
    int8_t  divisor_ = static_cast<int8_t>(divisor);
    int16_t quotient = dividend / divisor_;

    if (__builtin_expect(quotient > INT8_MAX || quotient < INT8_MIN, 0))
        return;
    ctx.eax.u8_lo = static_cast<uint8_t>(static_cast<int8_t>(quotient));
    ctx.eax.u8_hi = static_cast<uint8_t>(static_cast<int8_t>(dividend % divisor_));
}

X86_FORCEINLINE void X86_IDIV16(uint32_t divisor, X86Context& __restrict ctx)
{
    if (__builtin_expect(static_cast<int16_t>(divisor) == 0, 0))
        return;

    // DX = 0x8000 always overflows, and C++ can't divide INT32_MIN by -1.
    if (__builtin_expect(ctx.edx.u16 == 0x8000, 0))
        return;

    // DX -> high wowowowd :3
    // AX -> low word
    int32_t dividend = (static_cast<int32_t>(static_cast<int16_t>(ctx.edx.u16)) << 16) | static_cast<uint32_t>(ctx.eax.u16);
    int16_t divisor_ = static_cast<int16_t>(divisor);
    int32_t quotient = dividend / divisor_;
    int32_t remainder = dividend % divisor_;

    if (__builtin_expect(quotient > INT16_MAX || quotient < INT16_MIN, 0))
        return;

    ctx.eax.u16 = static_cast<uint16_t>(static_cast<int16_t>(quotient));
    ctx.edx.u16 = static_cast<uint16_t>(static_cast<int16_t>(remainder));
}

X86_FORCEINLINE void X86_IDIV32(uint32_t divisor, X86Context& __restrict ctx)
{
    if (__builtin_expect(divisor == 0, 0))
        return;

    // EDX = 0x80000000 always overflows, and C++ can't divide INT64_MIN by -1.
    if (__builtin_expect(ctx.edx.u32 == 0x80000000u, 0))
        return;

#if defined(__arm__)
    // CDQ's sign extension needs only a 32-bit divide, not the 64-bit library call.
    const int32_t low = ctx.eax.i32, den = static_cast<int32_t>(divisor);
    if (ctx.edx.i32 == (low >> 31)) {
        if (__builtin_expect(low == INT32_MIN && den == -1, 0))
            return;
        ctx.eax.i32 = low / den;
        ctx.edx.i32 = low % den;
        return;
    }
#endif

    // EDX -> high dword
    // EAX -> low dword
    int64_t dividend = (static_cast<int64_t>(static_cast<int32_t>(ctx.edx.u32)) << 32) | static_cast<uint64_t>(ctx.eax.u32);
    int32_t divisor_ = static_cast<int32_t>(divisor);
    int64_t quotient = dividend / divisor_;
    int64_t remainder = dividend % divisor_;

    if (__builtin_expect(quotient != static_cast<int32_t>(quotient), 0))
        return;

    ctx.eax.u32 = static_cast<uint32_t>(static_cast<int32_t>(quotient));
    ctx.edx.u32 = static_cast<uint32_t>(static_cast<int32_t>(remainder));
}

// IDIV r/m32 after CDQ, so we can assume EDX is just EAX's sign.
X86_FORCEINLINE void X86_IDIV32_CDQ(uint32_t divisor, X86Context& __restrict ctx)
{
    const int32_t dividend = ctx.eax.i32;
    const int32_t divisor_ = static_cast<int32_t>(divisor);
    if (__builtin_expect(divisor_ == 0, 0))
        return;
    if (__builtin_expect(dividend == INT32_MIN && divisor_ == -1, 0))
        return;

    ctx.eax.i32 = dividend / divisor_;
    ctx.edx.i32 = dividend % divisor_;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR8(uint8_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint8_t orig = dst;
    dst = static_cast<uint8_t>(static_cast<int8_t>(dst) >> n);
    ctx.eflags.update_flags_sar<Needed>(orig, n, dst, 1);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR16(uint16_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint16_t orig = dst;
    dst = static_cast<uint16_t>(static_cast<int16_t>(dst) >> n);
    ctx.eflags.update_flags_sar<Needed>(orig, n, dst, 2);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR32(uint32_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint32_t orig = dst;
    dst = static_cast<uint32_t>(static_cast<int32_t>(dst) >> n);
    ctx.eflags.update_flags_sar<Needed>(orig, n, dst, 4);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR8_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = static_cast<uint8_t>(static_cast<int8_t>(orig) >> n);
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_sar<Needed>(orig, n, res, 1);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR16_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = static_cast<uint16_t>(static_cast<int16_t>(orig) >> n);
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_sar<Needed>(orig, n, res, 2);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SAR32_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = static_cast<uint32_t>(static_cast<int32_t>(orig) >> n);
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_sar<Needed>(orig, n, res, 4);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR8(uint8_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint8_t temp = dst;
    dst >>= n;
    ctx.eflags.update_flags_shr<Needed>(temp, n, dst, 1);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR8_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig >> n;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_shr<Needed>(orig, n, res, 1);
}


template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR16(uint16_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint16_t temp = dst;
    dst >>= n;
    ctx.eflags.update_flags_shr<Needed>(temp, n, dst, 2);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR16_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig >> n;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_shr<Needed>(orig, n, res, 2);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR32(uint32_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint32_t temp = dst;
    dst >>= n;
    ctx.eflags.update_flags_shr<Needed>(temp, n, dst, 4);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHR32_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig >> n;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_shr<Needed>(orig, n, res, 4);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL8(uint8_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint8_t temp = dst;
    dst <<= n;
    ctx.eflags.update_flags_shl<Needed>(temp, n, dst, 1);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL8_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig << n;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_shl<Needed>(orig, n, res, 1);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL16(uint16_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint16_t temp = dst;
    dst <<= n;
    ctx.eflags.update_flags_shl<Needed>(temp, n, dst, 2);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL16_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig << n;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_shl<Needed>(orig, n, res, 2);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL32(uint32_t& dst, const uint8_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    uint32_t temp = dst;
    dst <<= n;
    ctx.eflags.update_flags_shl<Needed>(temp, n, dst, 4);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHL32_MEM(uintptr_t addr, const uint8_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig << n;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_shl<Needed>(orig, n, res, 4);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB32(uint32_t& dst, uint32_t src, X86Context& ctx)
{
    uint32_t orig = dst;
    dst = orig - src;
    ctx.eflags.update_flags_sub_U32<Needed>(orig, src, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig - src;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_sub_U32<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_SUB16_VAL(uint16_t orig, uint16_t src, X86Context& __restrict ctx)
{
    uint16_t res = orig - src;
    ctx.eflags.update_flags_sub_U16<Needed>(orig, src, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB16(uint16_t& dst, uint16_t src, X86Context& ctx)
{
    uint16_t orig = dst;
    dst = orig - src;
    ctx.eflags.update_flags_sub_U16<Needed>(orig, src, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig - src;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_sub_U16<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_SUB8_VAL(uint8_t orig, uint8_t src, X86Context& __restrict ctx)
{
    uint8_t res = orig - src;
    ctx.eflags.update_flags_sub_U8<Needed>(orig, src, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB8(uint8_t& dst, uint8_t src, X86Context& ctx)
{
    uint8_t orig = dst;
    dst = orig - src;
    ctx.eflags.update_flags_sub_U8<Needed>(orig, src, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SUB8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig - src;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_sub_U8<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD32(uint32_t& dst, uint32_t src, X86Context& ctx)
{
    uint32_t orig = dst;
    dst = orig + src;
    ctx.eflags.update_flags_add_U32<Needed>(orig, src, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig + src;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_add_U32<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_ADD16_VAL(uint16_t orig, uint16_t src, X86Context& __restrict ctx)
{
    uint16_t res = orig + src;
    ctx.eflags.update_flags_add_U16<Needed>(orig, src, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD16(uint16_t& dst, uint16_t src, X86Context& ctx)
{
    uint16_t orig = dst;
    dst = orig + src;
    ctx.eflags.update_flags_add_U16<Needed>(orig, src, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig + src;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_add_U16<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_ADD8_VAL(uint8_t orig, uint8_t src, X86Context& __restrict ctx)
{
    uint8_t res = orig + src;
    ctx.eflags.update_flags_add_U8<Needed>(orig, src, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD8(uint8_t& dst, uint8_t src, X86Context& ctx)
{
    uint8_t orig = dst;
    dst = orig + src;
    ctx.eflags.update_flags_add_U8<Needed>(orig, src, dst);
}

// mem destination, need base parameter for store macro.
template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADD8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig + src;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_add_U8<Needed>(orig, src, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC32(uint32_t& dst, X86Context& ctx)
{
    uint32_t orig = dst;
    dst = orig + 1;
    ctx.eflags.update_flags_inc_U32<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC32_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig + 1;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_inc_U32<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_INC16_VAL(uint16_t orig, X86Context& __restrict ctx)
{
    uint16_t res = orig + 1;
    ctx.eflags.update_flags_inc_U16<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC16(uint16_t& dst, X86Context& ctx)
{
    uint16_t orig = dst;
    dst = orig + 1;
    ctx.eflags.update_flags_inc_U16<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC16_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig + 1;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_inc_U16<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_INC8_VAL(uint8_t orig, X86Context& __restrict ctx)
{
    uint8_t res = orig + 1;
    ctx.eflags.update_flags_inc_U8<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC8(uint8_t& dst, X86Context& ctx)
{
    uint8_t orig = dst;
    dst = orig + 1;
    ctx.eflags.update_flags_inc_U8<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_INC8_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig + 1;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_inc_U8<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC32(uint32_t& dst, X86Context& ctx)
{
    uint32_t orig = dst;
    dst = orig - 1;
    ctx.eflags.update_flags_dec_U32<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC32_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig - 1;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_dec_U32<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_DEC16_VAL(uint16_t orig, X86Context& __restrict ctx)
{
    uint16_t res = orig - 1;
    ctx.eflags.update_flags_dec_U16<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC16(uint16_t& dst, X86Context& ctx)
{
    uint16_t orig = dst;
    dst = orig - 1;
    ctx.eflags.update_flags_dec_U16<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC16_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = orig - 1;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_dec_U16<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_DEC8_VAL(uint8_t orig, X86Context& __restrict ctx)
{
    uint8_t res = orig - 1;
    ctx.eflags.update_flags_dec_U8<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC8(uint8_t& dst, X86Context& ctx)
{
    uint8_t orig = dst;
    dst = orig - 1;
    ctx.eflags.update_flags_dec_U8<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_DEC8_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = orig - 1;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_dec_U8<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U32(uint32_t& dst, X86Context& ctx)
{
    uint32_t orig = dst;
    dst = -orig;
    ctx.eflags.update_flags_neg_U32<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U32_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = -orig;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    ctx.eflags.update_flags_neg_U32<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_NEG_U16_VAL(uint16_t orig, X86Context& __restrict ctx)
{
    uint16_t res = -orig;
    ctx.eflags.update_flags_neg_U16<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U16(uint16_t &dst, X86Context& ctx)
{
    uint16_t orig = dst;
    dst = -orig;
    ctx.eflags.update_flags_neg_U16<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U16_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = -orig;
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    ctx.eflags.update_flags_neg_U16<Needed>(orig, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_NEG_U8_VAL(uint8_t orig, X86Context& __restrict ctx)
{
    uint8_t res = -orig;
    ctx.eflags.update_flags_neg_U8<Needed>(orig, res);
    return res;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U8(uint8_t& dst, X86Context& ctx)
{
    uint8_t orig = dst;
    dst = -orig;
    ctx.eflags.update_flags_neg_U8<Needed>(orig, dst);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_NEG_U8_MEM(uintptr_t addr, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = -orig;
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    ctx.eflags.update_flags_neg_U8<Needed>(orig, res);
}

X86_FORCEINLINE uint32_t x86_imul_flags(bool overflow)
{
#if defined(X86_HOST_X86)
    return overflow ? (X86EFlags::CF | X86EFlags::OF) : 0;
#else
    // CF is bit 0 and OF is bit 11.
    return uint32_t(overflow) | (uint32_t(overflow) << 11);
#endif
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_EAX_U8(uint8_t src, X86Context& __restrict ctx)
{
    int16_t result = static_cast<int16_t>(static_cast<int8_t>(ctx.eax.u8_lo))
        * static_cast<int16_t>(static_cast<int8_t>(src));
    ctx.eax.u32 = (ctx.eax.u32 & 0xFFFF0000u) | static_cast<uint16_t>(result);
    // OF/CF set if result doesn't fit in 8 bits
    const bool overflow = (result < INT8_MIN || result > INT8_MAX);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_EAX_U16(uint16_t src, X86Context& __restrict ctx)
{
    int32_t result = static_cast<int32_t>(static_cast<int16_t>(ctx.eax.u16))
        * static_cast<int32_t>(static_cast<int16_t>(src));
    ctx.eax.u32 = (ctx.eax.u32 & 0xFFFF0000u) | static_cast<uint16_t>(result);
    ctx.edx.u32 = (ctx.edx.u32 & 0xFFFF0000u) | (static_cast<uint32_t>(result) >> 16);
    const bool overflow = result != static_cast<int16_t>(result);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_EAX_U32(uint32_t src, X86Context& __restrict ctx)
{
    int64_t result = static_cast<int64_t>(static_cast<int32_t>(ctx.eax.u32))
        * static_cast<int64_t>(static_cast<int32_t>(src));
    ctx.eax.u32 = static_cast<uint32_t>(result);
    ctx.edx.u32 = static_cast<uint32_t>(static_cast<uint64_t>(result) >> 32);
    const bool overflow = result != static_cast<int32_t>(result);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
}


template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_IMUL_U8_VAL(uint8_t lhs, uint8_t rhs, X86Context& __restrict ctx)
{
    int8_t result;
    const bool overflow = __builtin_mul_overflow(
        static_cast<int8_t>(lhs), static_cast<int8_t>(rhs), &result);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
    return static_cast<uint8_t>(result);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_U8(uint8_t& dst, uint8_t lhs, uint8_t rhs, X86Context& ctx)
{
    dst = X86_IMUL_U8_VAL<Needed>(lhs, rhs, ctx);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_IMUL_U16_VAL(uint16_t lhs, uint16_t rhs, X86Context& __restrict ctx)
{
    int16_t result;
    const bool overflow = __builtin_mul_overflow(
        static_cast<int16_t>(lhs), static_cast<int16_t>(rhs), &result);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
    return static_cast<uint16_t>(result);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_U16(uint16_t& dst, uint16_t lhs, uint16_t rhs, X86Context& ctx)
{
    dst = X86_IMUL_U16_VAL<Needed>(lhs, rhs, ctx);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_IMUL_U32(uint32_t& dst, uint32_t lhs, uint32_t rhs, X86Context& ctx)
{
    int32_t result;
    const bool overflow = __builtin_mul_overflow(
        static_cast<int32_t>(lhs), static_cast<int32_t>(rhs), &result);
    dst = static_cast<uint32_t>(result);
    // Intel's CPU manual says SF, ZF, AF and PF are undefined after IMUL, so we write 0.
    if constexpr (Needed != 0)
        ctx.eflags.common = (Needed & (X86EFlags::CF | X86EFlags::OF)) != 0 ? x86_imul_flags(overflow) : 0;
}

// Macros, so x86 can fold a memory source into the instruction.
#define X86_MINPS(dst, src) ((dst).reg = _mm_min_ps((dst).reg, (src)))
#define X86_MAXPS(dst, src) ((dst).reg = _mm_max_ps((dst).reg, (src)))

template<bool Volatile = true>
inline void X86_CVTSS2SI_MEM(uintptr_t addr, uint32_t &dst, uint8_t* base, bool mm_cvttss_si32)
{
    const __m128 value = _mm_set_ss(X86_LOAD_AS(float, Volatile, addr));
    if (mm_cvttss_si32)
        dst = static_cast<uint32_t>(static_cast<int32_t>(_mm_cvttss_si32(value)));
    else
        dst = static_cast<uint32_t>(static_cast<int32_t>(_mm_cvtss_si32(value)));
}

// --- Bitwise MEM helpers ---

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_AND32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t temp = X86_LOAD_AS(uint32_t, Volatile, addr);
    temp &= src;
    X86_STORE_AS(uint32_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_AND16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t temp = X86_LOAD_AS(uint16_t, Volatile, addr);
    temp &= src;
    X86_STORE_AS(uint16_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_AND8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t temp = X86_LOAD_AS(uint8_t, Volatile, addr);
    temp &= src;
    X86_STORE_AS(uint8_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_OR32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t temp = X86_LOAD_AS(uint32_t, Volatile, addr);
    temp |= src;
    X86_STORE_AS(uint32_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_OR16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t temp = X86_LOAD_AS(uint16_t, Volatile, addr);
    temp |= src;
    X86_STORE_AS(uint16_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_OR8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t temp = X86_LOAD_AS(uint8_t, Volatile, addr);
    temp |= src;
    X86_STORE_AS(uint8_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XOR32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t temp = X86_LOAD_AS(uint32_t, Volatile, addr);
    temp ^= src;
    X86_STORE_AS(uint32_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XOR16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t temp = X86_LOAD_AS(uint16_t, Volatile, addr);
    temp ^= src;
    X86_STORE_AS(uint16_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XOR8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t temp = X86_LOAD_AS(uint8_t, Volatile, addr);
    temp ^= src;
    X86_STORE_AS(uint8_t, Volatile, addr, temp);
    ctx.eflags.update_flags_logic<Needed>(temp);
}

// --- ADC helpers ---

// carry_out is only set with X86_HOST_SUBC; otherwise it is 0.
template<typename T>
X86_FORCEINLINE T x86_adc(T lhs, T rhs, uint32_t carry, uint32_t& carry_out)
{
#if defined(X86_HOST_SUBC)
    T out;
    T result;
    if constexpr (sizeof(T) == 1)
        result = __builtin_addcb(lhs, rhs, T(carry), &out);
    else if constexpr (sizeof(T) == 2)
        result = __builtin_addcs(lhs, rhs, T(carry), &out);
    else
        result = __builtin_addc(lhs, rhs, carry, &out);
    carry_out = out;
    return result;
#else
    carry_out = 0;
    return T(lhs + rhs + carry);
#endif
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC32(uint32_t& dst, uint32_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t orig = dst;
    uint32_t carry_out;
    uint32_t result = x86_adc(orig, src, carry, carry_out);
    dst = result;
    ctx.eflags.update_flags_adc_U32<Needed>(orig, src, carry, result, carry_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t carry_out;
    uint32_t result = x86_adc(orig, src, carry, carry_out);
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_adc_U32<Needed>(orig, src, carry, result, carry_out);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_ADC16_VAL(uint16_t orig, uint16_t src, X86Context& __restrict ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t carry_out;
    uint16_t result = x86_adc(orig, src, carry, carry_out);
    ctx.eflags.update_flags_adc_U16<Needed>(orig, src, carry, result, carry_out);
    return result;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC16(uint16_t& dst, uint16_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint16_t orig = dst;
    uint32_t carry_out;
    uint16_t result = x86_adc(orig, src, carry, carry_out);
    dst = result;
    ctx.eflags.update_flags_adc_U16<Needed>(orig, src, carry, result, carry_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint32_t carry_out;
    uint16_t result = x86_adc(orig, src, carry, carry_out);
    X86_STORE_AS(uint16_t, Volatile, addr, result);
    ctx.eflags.update_flags_adc_U16<Needed>(orig, src, carry, result, carry_out);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_ADC8_VAL(uint8_t orig, uint8_t src, X86Context& __restrict ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t carry_out;
    uint8_t result = x86_adc(orig, src, carry, carry_out);
    ctx.eflags.update_flags_adc_U8<Needed>(orig, src, carry, result, carry_out);
    return result;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC8(uint8_t& dst, uint8_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint8_t orig = dst;
    uint32_t carry_out;
    uint8_t result = x86_adc(orig, src, carry, carry_out);
    dst = result;
    ctx.eflags.update_flags_adc_U8<Needed>(orig, src, carry, result, carry_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_ADC8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint32_t carry_out;
    uint8_t result = x86_adc(orig, src, carry, carry_out);
    X86_STORE_AS(uint8_t, Volatile, addr, result);
    ctx.eflags.update_flags_adc_U8<Needed>(orig, src, carry, result, carry_out);
}

// --- SBB helpers ---

template<typename T>
X86_FORCEINLINE T x86_sbb(T lhs, T rhs, uint32_t borrow, uint32_t& borrow_out)
{
#if defined(X86_HOST_SUBC)
    T out;
    T result;
    if constexpr (sizeof(T) == 1)
        result = __builtin_subcb(lhs, rhs, T(borrow), &out);
    else if constexpr (sizeof(T) == 2)
        result = __builtin_subcs(lhs, rhs, T(borrow), &out);
    else
        result = __builtin_subc(lhs, rhs, borrow, &out);
    borrow_out = out;
    return result;
#else
    borrow_out = 0;
    return T(lhs - rhs - borrow);
#endif
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB32(uint32_t& dst, uint32_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t orig = dst;
    uint32_t borrow_out;
    uint32_t result = x86_sbb(orig, src, carry, borrow_out);
    dst = result;
    ctx.eflags.update_flags_sbb_U32<Needed>(orig, src, carry, result, borrow_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB32_MEM(uintptr_t addr, uint32_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t borrow_out;
    uint32_t result = x86_sbb(orig, src, carry, borrow_out);
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_sbb_U32<Needed>(orig, src, carry, result, borrow_out);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint16_t X86_SBB16_VAL(uint16_t orig, uint16_t src, X86Context& __restrict ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t borrow_out;
    uint16_t result = x86_sbb(orig, src, carry, borrow_out);
    ctx.eflags.update_flags_sbb_U16<Needed>(orig, src, carry, result, borrow_out);
    return result;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB16(uint16_t& dst, uint16_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint16_t orig = dst;
    uint32_t borrow_out;
    uint16_t result = x86_sbb(orig, src, carry, borrow_out);
    dst = result;
    ctx.eflags.update_flags_sbb_U16<Needed>(orig, src, carry, result, borrow_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB16_MEM(uintptr_t addr, uint16_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint32_t borrow_out;
    uint16_t result = x86_sbb(orig, src, carry, borrow_out);
    X86_STORE_AS(uint16_t, Volatile, addr, result);
    ctx.eflags.update_flags_sbb_U16<Needed>(orig, src, carry, result, borrow_out);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE uint8_t X86_SBB8_VAL(uint8_t orig, uint8_t src, X86Context& __restrict ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint32_t borrow_out;
    uint8_t result = x86_sbb(orig, src, carry, borrow_out);
    ctx.eflags.update_flags_sbb_U8<Needed>(orig, src, carry, result, borrow_out);
    return result;
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB8(uint8_t& dst, uint8_t src, X86Context& ctx)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint8_t orig = dst;
    uint32_t borrow_out;
    uint8_t result = x86_sbb(orig, src, carry, borrow_out);
    dst = result;
    ctx.eflags.update_flags_sbb_U8<Needed>(orig, src, carry, result, borrow_out);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SBB8_MEM(uintptr_t addr, uint8_t src, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t carry = (ctx.eflags.common & X86EFlags::CF) ? 1 : 0;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint32_t borrow_out;
    uint8_t result = x86_sbb(orig, src, carry, borrow_out);
    X86_STORE_AS(uint8_t, Volatile, addr, result);
    ctx.eflags.update_flags_sbb_U8<Needed>(orig, src, carry, result, borrow_out);
}

// --- BSF helpers ---

// Intel's CPU manual says CF, OF, SF, AF and PF are undefined after BSF, so we write 0.
template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_BSF32(uint32_t& dst, uint32_t src, X86Context& ctx)
{
    if (src == 0) {
        if constexpr (Needed != 0)
            ctx.eflags.common = X86EFlags::ZF;
    } else {
        if constexpr (Needed != 0)
            ctx.eflags.common = 0;
        dst = __builtin_ctz(src);
    }
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_BSF16(uint16_t& dst, uint16_t src, X86Context& ctx)
{
    if (src == 0) {
        if constexpr (Needed != 0)
            ctx.eflags.common = X86EFlags::ZF;
    } else {
        if constexpr (Needed != 0)
            ctx.eflags.common = 0;
        dst = (uint16_t)__builtin_ctz(src);
    }
}

// --- FPATAN helper ---

template<bool Status = true, bool Checked = true>
X86_FORCEINLINE void X86_FPATAN(X87FPU& fpu)
{
    x87_float st1Temp = fpu.st1();
    x87_float st0Temp = fpu.st0();
    if constexpr (Status)
        fpu.set_precision_flag();
    fpu.pop<Checked>();
    fpu.st0() = std::atan2(st1Temp, st0Temp);
}

// --- FXCH helper ---

X86_FORCEINLINE void X86_FXCH(X87FPU& fpu, int index = 1)
{
    x87_float& st0 = fpu.st0();
    x87_float& other = fpu.getST(index);
    x87_float tmp = st0;
    st0 = other;
    other = tmp;
}

// --- XCHG reg-reg helper ---

template<typename T>
inline void X86_XCHG_REG(T& a, T& b)
{
    T tmp = a;
    a = b;
    b = tmp;
}

// --- RCR MEM helpers ---

template<bool Volatile = true>
X86_FORCEINLINE void X86_RCR32_MEM(uintptr_t addr, uint8_t shift, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t temp = X86_LOAD_AS(uint32_t, Volatile, addr);
    X86_STORE_AS(uint32_t, Volatile, addr, X86_RCR32(temp, shift, ctx.eflags));
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_RCR16_MEM(uintptr_t addr, uint8_t shift, X86Context& __restrict ctx, uint8_t* base)
{
    uint16_t temp = X86_LOAD_AS(uint16_t, Volatile, addr);
    X86_STORE_AS(uint16_t, Volatile, addr, X86_RCR16(temp, shift, ctx.eflags));
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_RCR8_MEM(uintptr_t addr, uint8_t shift, X86Context& __restrict ctx, uint8_t* base)
{
    uint8_t temp = X86_LOAD_AS(uint8_t, Volatile, addr);
    X86_STORE_AS(uint8_t, Volatile, addr, X86_RCR8(temp, shift, ctx.eflags));
}

// --- ROL helpers ---

template<typename T>
X86_FORCEINLINE T x86_rotl(T value, uint32_t count)
{
    constexpr uint32_t bits = sizeof(T) * 8;
    // & (bits - n) avoids a shift by the full width. (e.g. on `rol eax, 0`)
    const uint32_t n = count & (bits - 1);
    return T((uint32_t(value) << n) | (uint32_t(value) >> ((bits - n) & (bits - 1))));
}

template<typename T>
X86_FORCEINLINE T x86_rotr(T value, uint32_t count)
{
    constexpr uint32_t bits = sizeof(T) * 8;
    const uint32_t n = count & (bits - 1);
    return T((uint32_t(value) >> n) | (uint32_t(value) << ((bits - n) & (bits - 1))));
}

X86_FORCEINLINE void X86_ROL32(uint32_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotl(dst, n);
    ctx.eflags.update_flags_rol<32>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROL32_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint32_t result = x86_rotl<uint32_t>(X86_LOAD_AS(uint32_t, Volatile, addr), n);
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_rol<32>(n, result);
}

X86_FORCEINLINE void X86_ROL16(uint16_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotl(dst, n);
    ctx.eflags.update_flags_rol<16>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROL16_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint16_t result = x86_rotl<uint16_t>(X86_LOAD_AS(uint16_t, Volatile, addr), n);
    X86_STORE_AS(uint16_t, Volatile, addr, result);
    ctx.eflags.update_flags_rol<16>(n, result);
}

X86_FORCEINLINE void X86_ROL8(uint8_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotl(dst, n);
    ctx.eflags.update_flags_rol<8>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROL8_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint8_t result = x86_rotl<uint8_t>(X86_LOAD_AS(uint8_t, Volatile, addr), n);
    X86_STORE_AS(uint8_t, Volatile, addr, result);
    ctx.eflags.update_flags_rol<8>(n, result);
}

// --- ROR helpers ---

X86_FORCEINLINE void X86_ROR32(uint32_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotr(dst, n);
    ctx.eflags.update_flags_ror<32>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROR32_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint32_t result = x86_rotr<uint32_t>(X86_LOAD_AS(uint32_t, Volatile, addr), n);
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_ror<32>(n, result);
}

X86_FORCEINLINE void X86_ROR16(uint16_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotr(dst, n);
    ctx.eflags.update_flags_ror<16>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROR16_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint16_t result = x86_rotr<uint16_t>(X86_LOAD_AS(uint16_t, Volatile, addr), n);
    X86_STORE_AS(uint16_t, Volatile, addr, result);
    ctx.eflags.update_flags_ror<16>(n, result);
}

X86_FORCEINLINE void X86_ROR8(uint8_t& dst, uint32_t count, X86Context& ctx)
{
    const uint32_t n = count & 0x1F;
    dst = x86_rotr(dst, n);
    ctx.eflags.update_flags_ror<8>(n, dst);
}

template<bool Volatile = true>
X86_FORCEINLINE void X86_ROR8_MEM(uintptr_t addr, uint32_t count, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t n = count & 0x1F;
    const uint8_t result = x86_rotr<uint8_t>(X86_LOAD_AS(uint8_t, Volatile, addr), n);
    X86_STORE_AS(uint8_t, Volatile, addr, result);
    ctx.eflags.update_flags_ror<8>(n, result);
}

// --- SHLD helpers ---

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHLD32(uint32_t& dst, uint32_t src, uint32_t raw_count, X86Context& ctx)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = dst;
    // (src >> 1) >> (31 - count) avoids a shift by 32 at count 0.
    uint32_t result = (orig << count) | ((src >> 1) >> (31 - count));
    dst = result;
    ctx.eflags.update_flags_shld<Needed>(orig, count, result, 32);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHLD32_MEM(uintptr_t addr, uint32_t src, uint32_t raw_count, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t result = (orig << count) | ((src >> 1) >> (31 - count));
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_shld<Needed>(orig, count, result, 32);
}

// Counts 17..31 are undefined on x86 and differ between tested CPUs.
template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHLD16(uint16_t& dst, uint16_t src, uint32_t raw_count, X86Context& ctx)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = dst;
    uint32_t result = (((orig << 16) | src) << count) >> 16;
    dst = (uint16_t)result;
    ctx.eflags.update_flags_shld<Needed>(orig, count, result, 16);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHLD16_MEM(uintptr_t addr, uint16_t src, uint32_t raw_count, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint32_t result = (((orig << 16) | src) << count) >> 16;
    X86_STORE_AS(uint16_t, Volatile, addr, (uint16_t)result);
    ctx.eflags.update_flags_shld<Needed>(orig, count, result, 16);
}

// --- SHRD helpers ---

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHRD32(uint32_t& dst, uint32_t src, uint32_t raw_count, X86Context& ctx)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = dst;
    // (src << 1) << (31 - count) avoids a shift by 32 at count 0.
    uint32_t result = (orig >> count) | ((src << 1) << (31 - count));
    dst = result;
    ctx.eflags.update_flags_shrd<Needed>(orig, count, result, 32);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHRD32_MEM(uintptr_t addr, uint32_t src, uint32_t raw_count, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t result = (orig >> count) | ((src << 1) << (31 - count));
    X86_STORE_AS(uint32_t, Volatile, addr, result);
    ctx.eflags.update_flags_shrd<Needed>(orig, count, result, 32);
}

// Counts 17..31 are undefined on x86 and differ between tested CPUs.
template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHRD16(uint16_t& dst, uint16_t src, uint32_t raw_count, X86Context& ctx)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = dst;
    uint32_t result = (((uint32_t)src << 16) | orig) >> count & 0xFFFF;
    dst = (uint16_t)result;
    ctx.eflags.update_flags_shrd<Needed>(orig, count, result, 16);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_SHRD16_MEM(uintptr_t addr, uint16_t src, uint32_t raw_count, X86Context& __restrict ctx, uint8_t* base)
{
    uint32_t count = raw_count & 0x1F;
    uint32_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint32_t result = (((uint32_t)src << 16) | orig) >> count & 0xFFFF;
    X86_STORE_AS(uint16_t, Volatile, addr, (uint16_t)result);
    ctx.eflags.update_flags_shrd<Needed>(orig, count, result, 16);
}

// --- CMPXCHG helpers ---
template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG32_MEM(uintptr_t addr, uint32_t src_val, X86Context& __restrict ctx, uint8_t* base)
{
    const uint32_t acc = ctx.eax.u32;
    uint32_t dest = acc;
    X86_ATOMIC_CMPXCHG<uint32_t>(base, addr, dest, src_val);
    ctx.eax.u32 = dest;
    ctx.eflags.update_compare_flags_U32<Needed>(acc, dest);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG32(uint32_t& current, uint32_t src_val, X86Context& ctx)
{
    const uint32_t acc = ctx.eax.u32;
    const uint32_t dest = current;
    if (acc == dest)
        current = src_val;
    else
        ctx.eax.u32 = dest;
    ctx.eflags.update_compare_flags_U32<Needed>(acc, dest);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG16_MEM(uintptr_t addr, uint16_t src_val, X86Context& __restrict ctx, uint8_t* base)
{
    const uint16_t acc = ctx.eax.u16;
    uint16_t dest = acc;
    X86_ATOMIC_CMPXCHG<uint16_t>(base, addr, dest, src_val);
    ctx.eax.u16 = dest;
    ctx.eflags.update_compare_flags_U16<Needed>(acc, dest);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG16(uint16_t& current, uint16_t src_val, X86Context& ctx)
{
    const uint16_t acc = ctx.eax.u16;
    const uint16_t dest = current;
    if (acc == dest)
        current = src_val;
    else
        ctx.eax.u16 = dest;
    ctx.eflags.update_compare_flags_U16<Needed>(acc, dest);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG8_MEM(uintptr_t addr, uint8_t src_val, X86Context& __restrict ctx, uint8_t* base)
{
    const uint8_t acc = ctx.eax.u8_lo;
    uint8_t dest = acc;
    X86_ATOMIC_CMPXCHG<uint8_t>(base, addr, dest, src_val);
    ctx.eax.u8_lo = dest;
    ctx.eflags.update_compare_flags_U8<Needed>(acc, dest);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_CMPXCHG8(uint8_t& current, uint8_t src_val, X86Context& ctx)
{
    const uint8_t acc = ctx.eax.u8_lo;
    const uint8_t dest = current;
    if (acc == dest)
        current = src_val;
    else
        ctx.eax.u8_lo = dest;
    ctx.eflags.update_compare_flags_U8<Needed>(acc, dest);
}

// --- XADD helpers ---
template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD32_ATOMIC(uintptr_t addr, uint32_t& src, X86Context& ctx, uint8_t* base)
{
    uint32_t add = src;
    uint32_t orig = X86_ATOMIC_XADD_U32(addr, add);
    src = orig;
    ctx.eflags.update_flags_add_U32<Needed>(orig, add, orig + add);
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD32_MEM(uintptr_t addr, uint32_t& src, X86Context& ctx, uint8_t* base)
{
    uint32_t add = src;
    uint32_t orig = X86_LOAD_AS(uint32_t, Volatile, addr);
    uint32_t res = orig + add;
    X86_STORE_AS(uint32_t, Volatile, addr, res);
    src = orig;
    ctx.eflags.update_flags_add_U32<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD16_ATOMIC(uintptr_t addr, uint16_t& src, X86Context& ctx, uint8_t* base)
{
    uint16_t add = src;
    uint16_t orig = X86_ATOMIC_XADD_U16(addr, add);
    src = orig;
    ctx.eflags.update_flags_add_U16<Needed>(orig, add, uint16_t(orig + add));
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD16_MEM(uintptr_t addr, uint16_t& src, X86Context& ctx, uint8_t* base)
{
    uint16_t add = src;
    uint16_t orig = X86_LOAD_AS(uint16_t, Volatile, addr);
    uint16_t res = uint16_t(orig + add);
    X86_STORE_AS(uint16_t, Volatile, addr, res);
    src = orig;
    ctx.eflags.update_flags_add_U16<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD8_ATOMIC(uintptr_t addr, uint8_t& src, X86Context& ctx, uint8_t* base)
{
    uint8_t add = src;
    uint8_t orig = X86_ATOMIC_XADD_U8(addr, add);
    src = orig;
    ctx.eflags.update_flags_add_U8<Needed>(orig, add, uint8_t(orig + add));
}

template<bool Volatile = true, uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD8_MEM(uintptr_t addr, uint8_t& src, X86Context& ctx, uint8_t* base)
{
    uint8_t add = src;
    uint8_t orig = X86_LOAD_AS(uint8_t, Volatile, addr);
    uint8_t res = uint8_t(orig + add);
    X86_STORE_AS(uint8_t, Volatile, addr, res);
    src = orig;
    ctx.eflags.update_flags_add_U8<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD32(uint32_t& dst, uint32_t& src, X86Context& ctx)
{
    uint32_t orig = dst;
    uint32_t add = src;
    uint32_t res = orig + add;
    src = orig;
    dst = res;
    ctx.eflags.update_flags_add_U32<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD16(uint16_t& dst, uint16_t& src, X86Context& ctx)
{
    uint16_t orig = dst;
    uint16_t add = src;
    uint16_t res = uint16_t(orig + add);
    src = orig;
    dst = res;
    ctx.eflags.update_flags_add_U16<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_XADD8(uint8_t& dst, uint8_t& src, X86Context& ctx)
{
    uint8_t orig = dst;
    uint8_t add = src;
    uint8_t res = uint8_t(orig + add);
    src = orig;
    dst = res;
    ctx.eflags.update_flags_add_U8<Needed>(orig, add, res);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_MUL8(uint8_t rhs, X86Context& __restrict ctx)
{
    uint16_t result = uint16_t(ctx.eax.u8_lo * rhs);
    ctx.eax.u16 = result;
    ctx.eflags.update_flags_mul<Needed>(result > 0xFF);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_MUL16(uint16_t rhs, X86Context& __restrict ctx)
{
    uint32_t result = uint32_t(ctx.eax.u16) * rhs;
    ctx.eax.u16 = uint16_t(result);
    ctx.edx.u16 = uint16_t(result >> 16);
    ctx.eflags.update_flags_mul<Needed>(result > 0xFFFF);
}

template<uint32_t Needed = X86EFlags::ARITHMETIC_FLAGS>
X86_FORCEINLINE void X86_MUL32(uint32_t rhs, X86Context& __restrict ctx)
{
    uint64_t result = static_cast<uint64_t>(ctx.eax.u32) * static_cast<uint64_t>(rhs);
    ctx.eax.u32 = static_cast<uint32_t>(result & 0xFFFFFFFF);
    ctx.edx.u32 = static_cast<uint32_t>(result >> 32);
    ctx.eflags.update_flags_mul<Needed>(ctx.edx.u32 != 0);
}

#endif // X86_CONTEXT_H_INCLUDED
