// SSE/MMX intrinsics for non-x86 hosts.
//
// SIMDe (MIT, github.com/simd-everywhere/simde) was the reference.
// No code is taken from it.
#if !defined(__GNUC__) && !defined(__clang__)
#error "Non-x86 hosts need GCC or Clang vector extensions"
#endif

typedef float __m128 __attribute__((vector_size(16), aligned(16)));
typedef long long __m128i __attribute__((vector_size(16), aligned(16)));
typedef long long __m64 __attribute__((vector_size(8), aligned(8)));
typedef float x86_m128_u __attribute__((vector_size(16), aligned(1), may_alias));
typedef long long x86_m128i_u __attribute__((vector_size(16), aligned(1), may_alias));
typedef long long x86_i64_u __attribute__((aligned(1), may_alias));

#if defined(__ARM_NEON) || defined(__ARM_NEON__)
#include <arm_neon.h>
#endif

typedef uint64_t x86_v2u64 __attribute__((vector_size(16)));
typedef uint32_t x86_v4u32 __attribute__((vector_size(16)));
typedef int32_t x86_v4i32 __attribute__((vector_size(16)));
typedef int16_t x86_v8i16 __attribute__((vector_size(16)));
typedef uint16_t x86_v8u16 __attribute__((vector_size(16)));
typedef uint8_t x86_v16u8 __attribute__((vector_size(16)));
typedef int32_t x86_v2i32 __attribute__((vector_size(8)));
typedef int16_t x86_v4i16 __attribute__((vector_size(8)));

// Loads, stores and sets.
X86_FORCEINLINE __m128 _mm_set_ss(float a) { return __m128{a, 0.0f, 0.0f, 0.0f}; }
X86_FORCEINLINE __m128 _mm_set_ps1(float a) { return __m128{a, a, a, a}; }
X86_FORCEINLINE __m128 _mm_set_ps(float e3, float e2, float e1, float e0) { return __m128{e0, e1, e2, e3}; }
X86_FORCEINLINE __m128 _mm_load_ss(const float* p) { return _mm_set_ss(*p); }
X86_FORCEINLINE __m128 _mm_loadu_ps(const float* p) { return *reinterpret_cast<const x86_m128_u*>(p); }
X86_FORCEINLINE __m128 _mm_load_ps(const float* p) { return *reinterpret_cast<const __m128*>(p); }
X86_FORCEINLINE void _mm_storeu_ps(float* p, __m128 a) { *reinterpret_cast<x86_m128_u*>(p) = a; }
X86_FORCEINLINE void _mm_store_ps(float* p, __m128 a) { *reinterpret_cast<__m128*>(p) = a; }
X86_FORCEINLINE void _mm_stream_ps(float* p, __m128 a) { *reinterpret_cast<__m128*>(p) = a; }
X86_FORCEINLINE void _mm_stream_si64(long long* p, long long a) { *reinterpret_cast<x86_i64_u*>(p) = a; }
X86_FORCEINLINE float _mm_cvtss_f32(__m128 a) { return a[0]; }

// Scalar math on lane 0.
X86_FORCEINLINE __m128 _mm_move_ss(__m128 a, __m128 b) { return __builtin_shufflevector(a, b, 4, 1, 2, 3); }
X86_FORCEINLINE __m128 _mm_add_ss(__m128 a, __m128 b) { a[0] += b[0]; return a; }
X86_FORCEINLINE __m128 _mm_sub_ss(__m128 a, __m128 b) { a[0] -= b[0]; return a; }
X86_FORCEINLINE __m128 _mm_mul_ss(__m128 a, __m128 b) { a[0] *= b[0]; return a; }
X86_FORCEINLINE __m128 _mm_div_ss(__m128 a, __m128 b) { a[0] /= b[0]; return a; }
// Doesn't touch errno, so it inlines.
#if defined(__clang__)
X86_FORCEINLINE float x86_sqrtf(float a) { return __builtin_elementwise_sqrt(a); }
#else
X86_FORCEINLINE float x86_sqrtf(float a) { return __builtin_sqrtf(a); }
#endif
X86_FORCEINLINE __m128 _mm_sqrt_ss(__m128 a) { a[0] = x86_sqrtf(a[0]); return a; }
// Exact, not x86's 12-bit estimate.
X86_FORCEINLINE __m128 _mm_rsqrt_ss(__m128 a) { a[0] = 1.0f / x86_sqrtf(a[0]); return a; }

// Packed math and logic.
// ARMv7 needs scalar VFP, not flushing NEON; assembly stops Clang vectorising
// these operations after volatile loads.
#if defined(__arm__)
X86_FORCEINLINE __m128 _mm_add_ps(__m128 a, __m128 b)
{
    for (int i = 0; i < 4; ++i)
        __asm__ volatile("vadd.f32 %0, %0, %1" : "+t"(a[i]) : "t"(b[i]) : "memory");
    return a;
}
X86_FORCEINLINE __m128 _mm_sub_ps(__m128 a, __m128 b)
{
    for (int i = 0; i < 4; ++i)
        __asm__ volatile("vsub.f32 %0, %0, %1" : "+t"(a[i]) : "t"(b[i]) : "memory");
    return a;
}
X86_FORCEINLINE __m128 _mm_mul_ps(__m128 a, __m128 b)
{
    for (int i = 0; i < 4; ++i)
        __asm__ volatile("vmul.f32 %0, %0, %1" : "+t"(a[i]) : "t"(b[i]) : "memory");
    return a;
}
#else
X86_FORCEINLINE __m128 _mm_add_ps(__m128 a, __m128 b) { return a + b; }
X86_FORCEINLINE __m128 _mm_sub_ps(__m128 a, __m128 b) { return a - b; }
X86_FORCEINLINE __m128 _mm_mul_ps(__m128 a, __m128 b) { return a * b; }
#endif
X86_FORCEINLINE __m128 _mm_and_ps(__m128 a, __m128 b) { return (__m128)((x86_v4u32)a & (x86_v4u32)b); }
X86_FORCEINLINE __m128 _mm_xor_ps(__m128 a, __m128 b) { return (__m128)((x86_v4u32)a ^ (x86_v4u32)b); }

// x86 returns b on NaN or when both are zero.
#if defined(__arm__)
// asm so Clang doesn't turn this back into NEON.
X86_FORCEINLINE float x86_max_ss(float a, float b)
{
    __asm__("vcmp.f32 %1, %2\n\tvmrs APSR_nzcv, fpscr\n\tvmovgt.f32 %0, %1" : "+t"(b) : "t"(a), "t"(b) : "cc");
    return b;
}
X86_FORCEINLINE float x86_min_ss(float a, float b)
{
    __asm__("vcmp.f32 %1, %2\n\tvmrs APSR_nzcv, fpscr\n\tvmovmi.f32 %0, %1" : "+t"(b) : "t"(a), "t"(b) : "cc");
    return b;
}
X86_FORCEINLINE __m128 _mm_max_ps(__m128 a, __m128 b)
{
    return __m128{x86_max_ss(a[0], b[0]), x86_max_ss(a[1], b[1]), x86_max_ss(a[2], b[2]), x86_max_ss(a[3], b[3])};
}
X86_FORCEINLINE __m128 _mm_min_ps(__m128 a, __m128 b)
{
    return __m128{x86_min_ss(a[0], b[0]), x86_min_ss(a[1], b[1]), x86_min_ss(a[2], b[2]), x86_min_ss(a[3], b[3])};
}
#elif defined(__aarch64__)
// Strict FP splits a vector compare into four; asm keeps one fcmgt.
X86_FORCEINLINE __m128 _mm_max_ps(__m128 a, __m128 b)
{
    uint32x4_t take_a;
    __asm__("fcmgt %0.4s, %1.4s, %2.4s" : "=w"(take_a) : "w"(a), "w"(b));
    return (__m128)vbslq_f32(take_a, (float32x4_t)a, (float32x4_t)b);
}
X86_FORCEINLINE __m128 _mm_min_ps(__m128 a, __m128 b)
{
    uint32x4_t take_a;
    __asm__("fcmgt %0.4s, %1.4s, %2.4s" : "=w"(take_a) : "w"(b), "w"(a));
    return (__m128)vbslq_f32(take_a, (float32x4_t)a, (float32x4_t)b);
}
#else
X86_FORCEINLINE __m128 _mm_max_ps(__m128 a, __m128 b)
{
    const x86_v4u32 take_a = (x86_v4u32)(a > b);
    return (__m128)((take_a & (x86_v4u32)a) | (~take_a & (x86_v4u32)b));
}
X86_FORCEINLINE __m128 _mm_min_ps(__m128 a, __m128 b)
{
    const x86_v4u32 take_a = (x86_v4u32)(a < b);
    return (__m128)((take_a & (x86_v4u32)a) | (~take_a & (x86_v4u32)b));
}
#endif

// Lane moves.
#if defined(__aarch64__)
// On arm64, moving 64-bit halves needs fewer instructions.
X86_FORCEINLINE __m128 _mm_movelh_ps(__m128 a, __m128 b) { return (__m128)__builtin_shufflevector((x86_v2u64)a, (x86_v2u64)b, 0, 2); }
X86_FORCEINLINE __m128 _mm_movehl_ps(__m128 a, __m128 b) { return (__m128)__builtin_shufflevector((x86_v2u64)a, (x86_v2u64)b, 3, 1); }
#else
X86_FORCEINLINE __m128 _mm_movelh_ps(__m128 a, __m128 b) { return __builtin_shufflevector(a, b, 0, 1, 4, 5); }
X86_FORCEINLINE __m128 _mm_movehl_ps(__m128 a, __m128 b) { return __builtin_shufflevector(a, b, 6, 7, 2, 3); }
#endif
X86_FORCEINLINE __m128 _mm_unpacklo_ps(__m128 a, __m128 b) { return __builtin_shufflevector(a, b, 0, 4, 1, 5); }
X86_FORCEINLINE __m128 _mm_unpackhi_ps(__m128 a, __m128 b) { return __builtin_shufflevector(a, b, 2, 6, 3, 7); }
#define _mm_shuffle_ps(a, b, imm) \
    __builtin_shufflevector((__m128)(a), (__m128)(b), (imm) & 3, ((imm) >> 2) & 3, \
                            4 + (((imm) >> 4) & 3), 4 + (((imm) >> 6) & 3))

// Compares.
X86_FORCEINLINE __m128 x86_mask_ss(__m128 a, bool set)
{
    x86_v4u32 bits = (x86_v4u32)a;
    bits[0] = set ? ~0u : 0u;
    return (__m128)bits;
}
X86_FORCEINLINE __m128 _mm_cmpeq_ss(__m128 a, __m128 b) { return x86_mask_ss(a, a[0] == b[0]); }
X86_FORCEINLINE __m128 _mm_cmplt_ss(__m128 a, __m128 b) { return x86_mask_ss(a, a[0] < b[0]); }
X86_FORCEINLINE __m128 _mm_cmple_ss(__m128 a, __m128 b) { return x86_mask_ss(a, a[0] <= b[0]); }
X86_FORCEINLINE __m128 _mm_cmpneq_ss(__m128 a, __m128 b) { return x86_mask_ss(a, !(a[0] == b[0])); }
X86_FORCEINLINE __m128 _mm_cmpnlt_ss(__m128 a, __m128 b) { return x86_mask_ss(a, !(a[0] < b[0])); }
X86_FORCEINLINE __m128 _mm_cmpnle_ss(__m128 a, __m128 b) { return x86_mask_ss(a, !(a[0] <= b[0])); }
X86_FORCEINLINE __m128 _mm_cmpord_ss(__m128 a, __m128 b) { return x86_mask_ss(a, !__builtin_isunordered(a[0], b[0])); }
X86_FORCEINLINE __m128 _mm_cmpunord_ss(__m128 a, __m128 b) { return x86_mask_ss(a, __builtin_isunordered(a[0], b[0])); }

// Conversions.
// NaN and out of range give INT32_MIN, like x86.
X86_FORCEINLINE int32_t x86_f32_to_i32(float truncated)
{
    return __builtin_fabsf(truncated) < 2147483648.0f ? int32_t(truncated) : INT32_MIN;
}
// Uses the current rounding mode.
X86_FORCEINLINE int32_t x86_f32_to_i32_current(float a)
{
#if defined(__arm__) && defined(__ARM_FP)
    float converted;
    __asm__ volatile("vcvtr.s32.f32 %0, %1" : "=t"(converted) : "t"(a));
    int32_t result;
    memcpy(&result, &converted, 4);
    return __builtin_fabsf(a) < 2147483648.0f ? result : INT32_MIN;
#else
    return x86_f32_to_i32(__builtin_rintf(a));
#endif
}
X86_FORCEINLINE int _mm_cvttss_si32(__m128 a) { return x86_f32_to_i32(a[0]); }
X86_FORCEINLINE int _mm_cvtss_si32(__m128 a) { return x86_f32_to_i32_current(a[0]); }
X86_FORCEINLINE __m128 _mm_cvtsi32_ss(__m128 a, int b) { a[0] = float(b); return a; }
X86_FORCEINLINE __m64 _mm_cvtps_pi32(__m128 a)
{
    return (__m64)x86_v2i32{x86_f32_to_i32_current(a[0]), x86_f32_to_i32_current(a[1])};
}

// MMX.
X86_FORCEINLINE __m64 _mm_cvtsi64_m64(long long a) { return __m64{a}; }
X86_FORCEINLINE long long _mm_cvtm64_si64(__m64 a) { return a[0]; }
X86_FORCEINLINE __m64 _mm_cvtsi32_si64(int a) { return __m64{(long long)(uint32_t)a}; }
X86_FORCEINLINE void _mm_empty() {}

typedef int32_t x86_v8i32 __attribute__((vector_size(32)));
typedef int16_t x86_v16i16 __attribute__((vector_size(32)));

X86_FORCEINLINE __m64 _mm_packs_pi32(__m64 a, __m64 b)
{
#if defined(__ARM_NEON) || defined(__ARM_NEON__)
    return (__m64)vqmovn_s32(vcombine_s32((int32x2_t)a, (int32x2_t)b));
#endif
    x86_v4i32 v = __builtin_shufflevector((x86_v2i32)a, (x86_v2i32)b, 0, 1, 2, 3);
    v = v < -32768 ? -32768 : v;
    v = v > 32767 ? 32767 : v;
    return (__m64)__builtin_convertvector(v, x86_v4i16);
}

// SSE2 integer.
X86_FORCEINLINE __m128i _mm_castps_si128(__m128 a) { return (__m128i)a; }
X86_FORCEINLINE __m128 _mm_castsi128_ps(__m128i a) { return (__m128)a; }
X86_FORCEINLINE __m128i _mm_cvtsi64_si128(long long a) { return __m128i{a, 0}; }
X86_FORCEINLINE long long _mm_cvtsi128_si64(__m128i a) { return a[0]; }
X86_FORCEINLINE __m128i _mm_set_epi64x(long long hi, long long lo) { return __m128i{lo, hi}; }
X86_FORCEINLINE __m128i _mm_set_epi32(int e3, int e2, int e1, int e0) { return (__m128i)x86_v4i32{e0, e1, e2, e3}; }
X86_FORCEINLINE __m128i _mm_loadu_si128(const __m128i* p) { return *reinterpret_cast<const x86_m128i_u*>(p); }
X86_FORCEINLINE __m128i _mm_loadl_epi64(const __m128i* p) { return __m128i{*reinterpret_cast<const x86_i64_u*>(p), 0}; }
X86_FORCEINLINE __m128i _mm_unpacklo_epi64(__m128i a, __m128i b) { return __m128i{a[0], b[0]}; }
X86_FORCEINLINE __m128i _mm_unpacklo_epi16(__m128i a, __m128i b)
{
    return (__m128i)__builtin_shufflevector((x86_v8u16)a, (x86_v8u16)b, 0, 8, 1, 9, 2, 10, 3, 11);
}
X86_FORCEINLINE __m128i _mm_unpackhi_epi16(__m128i a, __m128i b)
{
    return (__m128i)__builtin_shufflevector((x86_v8u16)a, (x86_v8u16)b, 4, 12, 5, 13, 6, 14, 7, 15);
}
X86_FORCEINLINE __m128i _mm_add_epi16(__m128i a, __m128i b) { return (__m128i)((x86_v8u16)a + (x86_v8u16)b); }
// Rounding average without overflow, from Hacker's Delight:
// https://web.archive.org/web/20180831033349/http://hackersdelight.org/basics2.pdf#G525596
X86_FORCEINLINE __m128i _mm_avg_epu8(__m128i a, __m128i b)
{
    const x86_v16u8 x = (x86_v16u8)a, y = (x86_v16u8)b;
    return (__m128i)((x | y) - ((x ^ y) >> 1));
}

X86_FORCEINLINE __m128i _mm_sra_epi16(__m128i a, __m128i count)
{
    const uint64_t n = uint64_t(count[0]) > 15 ? 15 : uint64_t(count[0]);
    return (__m128i)((x86_v8i16)a >> int16_t(n));
}

// Counts above 63 give 0.
X86_FORCEINLINE __m128i _mm_srl_epi64(__m128i a, __m128i count)
{
    const uint64_t n = uint64_t(count[0]);
    return n > 63 ? __m128i{0, 0} : (__m128i)((x86_v2u64)a >> n);
}
X86_FORCEINLINE __m128i _mm_srli_epi64(__m128i a, int imm)
{
    return _mm_srl_epi64(a, __m128i{(long long)(unsigned)imm, 0});
}

X86_FORCEINLINE __m128i _mm_packs_epi32(__m128i a, __m128i b)
{
#if defined(__ARM_NEON) || defined(__ARM_NEON__)
    return (__m128i)vcombine_s16(vqmovn_s32((int32x4_t)a), vqmovn_s32((int32x4_t)b));
#endif
    x86_v8i32 v = __builtin_shufflevector((x86_v4i32)a, (x86_v4i32)b, 0, 1, 2, 3, 4, 5, 6, 7);
    v = v < -32768 ? -32768 : v;
    v = v > 32767 ? 32767 : v;
    return (__m128i)__builtin_convertvector(v, x86_v8i16);
}

X86_FORCEINLINE __m128i _mm_packus_epi16(__m128i a, __m128i b)
{
#if defined(__ARM_NEON) || defined(__ARM_NEON__)
    return (__m128i)vcombine_u8(vqmovun_s16((int16x8_t)a), vqmovun_s16((int16x8_t)b));
#endif
    x86_v16i16 v = __builtin_shufflevector((x86_v8i16)a, (x86_v8i16)b,
                                           0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
    v = v < 0 ? 0 : v;
    v = v > 255 ? 255 : v;
    return (__m128i)__builtin_convertvector(v, x86_v16u8);
}

// MXCSR, mapped onto FPCR/FPSCR. ARM's FZ also flushes inputs.
#if defined(__aarch64__)
X86_FORCEINLINE uint64_t x86_host_fpcr() { uint64_t v; __asm__ volatile("mrs %0, fpcr" : "=r"(v) :: "memory"); return v; }
X86_FORCEINLINE void x86_set_host_fpcr(uint64_t v) { __asm__ volatile("msr fpcr, %0" :: "r"(v) : "memory"); }
#elif defined(__arm__) && defined(__ARM_FP)
X86_FORCEINLINE uint32_t x86_host_fpcr() { uint32_t v; __asm__ volatile("vmrs %0, fpscr" : "=r"(v) :: "memory"); return v; }
X86_FORCEINLINE void x86_set_host_fpcr(uint32_t v) { __asm__ volatile("vmsr fpscr, %0" :: "r"(v) : "memory"); }
#else
X86_FORCEINLINE uint32_t x86_host_fpcr() { return 0; }
X86_FORCEINLINE void x86_set_host_fpcr(uint32_t) {}
#endif

// noinline so float ops don't get moved across it.
__attribute__((noinline)) inline void _mm_setcsr(uint32_t csr)
{
    // Up and down are swapped between MXCSR and ARM.
    const uint32_t rc = (csr >> 13) & 3;
    const uint32_t mode = ((rc & 1) << 1) | (rc >> 1);
    const auto fpcr = x86_host_fpcr();
    x86_set_host_fpcr((fpcr & ~decltype(fpcr)(0x1C00000)) | (mode << 22) | ((csr & 0x8000) ? 1u << 24 : 0));
}

__attribute__((noinline)) inline uint32_t _mm_getcsr()
{
    const uint32_t fpcr = uint32_t(x86_host_fpcr());
    const uint32_t mode = (fpcr >> 22) & 3;
    const uint32_t rc = ((mode & 1) << 1) | (mode >> 1);
    return 0x1F80 | (rc << 13) | ((fpcr & (1u << 24)) ? 0x8000 : 0);
}
