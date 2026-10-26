/* ============================================================================
 * rupix_decoder_kit.h - single-header C kit for writing RuPix decoder decoders.
 *
 * WHY THIS EXISTS
 *   The ABI contract lives in RuPix-Decoder/abi/src/lib.rs (Rust, the single
 *   source of truth). Rust decoder authors get it as a path dependency for free;
 *   a C/C++ decoder author cannot. This header is the C translation so that
 *   "decoders may be written in any language" (strategy charter) is not a slogan.
 *
 *   Hand-copying a struct layout is normally forbidden (a silently drifting
 *   layout means the host allocates `bytes_needed` wrong and the decoder writes
 *   out of bounds). Therefore this header carries the SAME defense as the Rust
 *   side: C11 `_Static_assert` on every struct size. If abi/src/lib.rs ever
 *   changes a field, the sizes here stop matching and the C build fails
 *   immediately - it cannot drift silently. When you change the ABI you must
 *   update this header in the same commit AND re-run abi/c/abi_probe.c
 *   (it prints the sizes + version; compare with the Rust asserts).
 *
 * WHAT IS DIFFERENT FROM THE RUST KIT
 *   - C has no panic/unwind. The equivalent of the Rust kit's panic gate is the
 *     MSVC structured-exception guard (RUPIX_GUARD_*): access violations in your
 *     decoder become RUPIX_ERR_PANIC instead of killing the host process.
 *   - No allocation helpers, no traits: the host owns the pixel buffer and the
 *     decoder only writes into it (who-allocates-frees rule).
 *
 * BUILD REQUIREMENTS
 *   MSVC only (charter V4 / K06: mixing MSVC and MinGW toolchains breaks the
 *   static CRT). Non-MSVC compilers are rejected below on purpose.
 *
 * SPDX-License-Identifier: AGPL-3.0
 * ============================================================================ */

#ifndef RUPIX_DECODER_KIT_H
#define RUPIX_DECODER_KIT_H

#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* ---------------------------------------------------------------------------
 * 1. Version + error codes + pixel formats (values frozen; see abi/src/lib.rs)
 * ------------------------------------------------------------------------ */

#define RUPIX_ABI_VERSION 5u

/* Error codes - the host maps each class to a different UI message; never
 * collapse "I cannot do this" into "the file is corrupt" (fallback chain). */
#define RUPIX_ERR_OK               0
#define RUPIX_ERR_UNRECOGNIZED     1  /* magic mismatch -> "unknown format" */
#define RUPIX_ERR_CORRUPT          2  /* broken data -> "file is damaged" */
#define RUPIX_ERR_UNSUPPORTED      3  /* recognized, feature out of scope -> "next decoder, please" */
#define RUPIX_ERR_BUFFER_TOO_SMALL 4
#define RUPIX_ERR_OOM              5
#define RUPIX_ERR_PANIC            6  /* caught fault -> "decoder isolated" */
#define RUPIX_ERR_TOO_LARGE        7  /* budget / dimension limit */

/* pixel_format values (transmission format is always RUPIX_PF_RGBA8 for the
 * bitmap path; the YUV/BGRA ones are inputs of the optional iv_render_raw). */
#define RUPIX_PF_GRAY8    0u
#define RUPIX_PF_RGB8     1u
#define RUPIX_PF_RGBA8    2u
#define RUPIX_PF_GRAY16   3u
#define RUPIX_PF_RGB16    4u
#define RUPIX_PF_RGBA16   5u
#define RUPIX_PF_GRAY_F32 6u
#define RUPIX_PF_RGBA_F32 7u
#define RUPIX_PF_BGRA8    8u
#define RUPIX_PF_YUY2     9u
#define RUPIX_PF_UYVY     10u
#define RUPIX_PF_I420     11u
#define RUPIX_PF_YV12     12u
#define RUPIX_PF_NV12     13u

/* Decode-capability bitmap - the value returned by the OPTIONAL export iv_caps().
 * v4 addition, retained in v5, does NOT bump RUPIX_ABI_VERSION (rule: never change published
 * exports or structs; new capability goes through a new optional export).
 *
 * bit set = supported. Only ever ADD bits, never renumber published ones (the
 * host routes by these bits, so a renumber makes old decoders lie).
 *
 * If you omit iv_caps entirely the host derives caps from what it already has
 * (is_vector, and whether iv_render_raw is exported). CAP_SCALED is NEVER derived
 * - there is no legacy evidence for it, and inventing it would send scaled requests
 * to decoders that can only do full decodes. */
#define RUPIX_CAP_FULL        1u
#define RUPIX_CAP_SCALED      2u
#define RUPIX_CAP_RAW_PARAM   4u
#define RUPIX_CAP_MULTIFRAME  8u
#define RUPIX_CAP_MULTIPAGE   16u
#define RUPIX_CAP_VECTOR      32u

/* Request intent bit (IvRenderReq.flags). The capability bitmap says whether a
 * decoder CAN decode scaled; this bit says whether THIS request WANTS it.
 * The host sends a non-zero target for the main view too (viewport size) and
 * that must stay pixel-honest, so a decoder must never scale on target alone.
 * Unset = decode the whole image (what every shipped decoder does today). */
#define RUPIX_REQ_ALLOW_SCALED 4u

/* ---------------------------------------------------------------------------
 * 2. Platform gate + export macro
 * ------------------------------------------------------------------------ */

#if !defined(_MSC_VER)
#error "RuPix decoders are built with MSVC only (mixed MSVC/MinGW breaks the static CRT: K06)."
#endif

/* C mode needs C11 for _Static_assert (MSVC: pass /std:c11; CMake: C_STANDARD 11). */
#if !defined(__cplusplus) && (!defined(__STDC_VERSION__) || __STDC_VERSION__ < 201112L)
#error "Compile C sources with /std:c11 (the kit uses _Static_assert for layout pins)."
#endif

#if defined(__cplusplus)
#  define RUPIX_STATIC_ASSERT(cond, msg) static_assert(cond, msg)
#else
#  define RUPIX_STATIC_ASSERT(cond, msg) _Static_assert(cond, msg)
#endif

#define RUPIX_EXPORT __declspec(dllexport)

/* ---------------------------------------------------------------------------
 * 3. Cross-boundary structs - field-by-field mirror of abi/src/lib.rs
 * ------------------------------------------------------------------------ */

/* Decoder self-description (iv_decoder_info). All pointers are to static
 * read-only NUL-terminated byte strings owned by the decoder. */
typedef struct RupixDecoderInfo {
    const char* id;           /* unique id: lowercase letters/digits/hyphen, e.g. "stb" */
    const char* display_name; /* e.g. "STB Image" */
    uint32_t    abi_version;  /* must equal RUPIX_ABI_VERSION */
    const char* extensions;   /* semicolon-separated lowercase suffixes, no dot: "bmp;tga" */
    int32_t     is_vector;    /* 0 = bitmap */
    int32_t     has_alpha;
    const char* description;  /* one line for the settings page */
} RupixDecoderInfo;

/* Extensible metadata table (optional export iv_metadata). New keys are appended to
 * `fields`; IvDecoderInfo stays frozen and RUPIX_ABI_VERSION does not change. */
typedef struct RupixMetadataField {
    const char* key;   /* NUL-terminated UTF-8 key, e.g. "license" */
    const char* value; /* NUL-terminated UTF-8 value */
} RupixMetadataField;

typedef struct RupixMetadata {
    uint32_t size;          /* sizeof(RupixMetadata); host checks this before reading */
    uint32_t version;       /* metadata key semantics, independent of ABI */
    uint32_t field_count;
    uint32_t reserved;      /* must be 0 */
    const RupixMetadataField* fields; /* static array owned by the decoder */
} RupixMetadata;

#define RUPIX_METADATA_VERSION 1u
#define RUPIX_METADATA_FIELD(KEY, VALUE) { (KEY), (VALUE) }

/* Render request (host -> decoder). size 24. */
typedef struct RupixRenderReq {
    uint32_t target_w;      /* 0 = natural size */
    uint32_t target_h;
    uint32_t budget_bytes;  /* pixel budget cap, 0 = unlimited */
    uint32_t flags;         /* bit 2 = RUPIX_REQ_ALLOW_SCALED; bits 1/2 reserved */
    uint32_t frame_index;   /* animation frame, 0-based (static: always 0) */
    uint32_t page_index;    /* page/sub-image, 0-based */
} RupixRenderReq;

/* Render result (decoder -> host). size 48. Query phase fills metadata only. */
typedef struct RupixRenderOut {
    uint32_t actual_w;        /* may differ from target_w (DCT scaling) */
    uint32_t actual_h;
    uint64_t bytes_needed;    /* query phase: host allocates exactly this */
    uint32_t pixel_format;    /* RUPIX_PF_* */
    uint32_t bit_depth;
    int32_t  orientation;     /* EXIF 1..8 */
    int32_t  already_oriented;/* 1 = orientation already applied */
    uint32_t frame_count;     /* static must be 1; 0 = not yet known (animation) */
    uint32_t frame_delay_ms;  /* 0 = static */
    uint32_t page_count;      /* single-page still images: 1 */
    uint32_t icc_len;         /* ICC bytes after the pixels, 0 = none */
} RupixRenderOut;

/* Layout assertions: the whole point of this header. If these fail, do NOT
 * "fix" them by editing the numbers - compare against abi/src/lib.rs. */
RUPIX_STATIC_ASSERT(sizeof(RupixDecoderInfo) == 48, "RupixDecoderInfo layout drift (see abi/src/lib.rs)");
RUPIX_STATIC_ASSERT(sizeof(RupixMetadata) == 24, "RupixMetadata layout drift (see abi/src/lib.rs)");
RUPIX_STATIC_ASSERT(sizeof(RupixMetadataField) == 16, "RupixMetadataField layout drift (see abi/src/lib.rs)");
RUPIX_STATIC_ASSERT(sizeof(RupixRenderReq) == 24, "RupixRenderReq layout drift (see abi/src/lib.rs)");
RUPIX_STATIC_ASSERT(sizeof(RupixRenderOut) == 48, "RupixRenderOut layout drift (see abi/src/lib.rs)");

/* ---------------------------------------------------------------------------
 * 4. Last-error plumbing (thread-local; single decode thread per contract)
 * ------------------------------------------------------------------------ */

/* Implemented by RUPIX_DECLARE_DECODER in exactly one translation unit;
 * other TUs just call it after declaring the prototype. */
void rupix_set_last_error(const char* msg); /* NULL clears */

/* ---------------------------------------------------------------------------
 * 5. Export block: info + abi_version + last_error + thread-local storage
 *
 * Put this ONCE in one .c file of your decoder. It implements the shared
 * thread-local buffer so every translation unit sees the same message.
 * ------------------------------------------------------------------------ */

#define RUPIX_DECLARE_DECODER(ID, DISPLAY_NAME, EXTENSIONS, DESC, IS_VECTOR, HAS_ALPHA)          \
    static const char rupix__id[]    = ID;                                                      \
    static const char rupix__name[]  = DISPLAY_NAME;                                            \
    static const char rupix__exts[]  = EXTENSIONS;                                              \
    static const char rupix__desc[]  = DESC;                                                    \
    static const RupixDecoderInfo rupix__info = {                                                \
        rupix__id, rupix__name, RUPIX_ABI_VERSION, rupix__exts, (IS_VECTOR), (HAS_ALPHA),       \
        rupix__desc                                                                             \
    };                                                                                          \
    static __declspec(thread) char rupix__last_error[512];                                      \
    void rupix_set_last_error(const char* msg) {                                                \
        if (!msg) { rupix__last_error[0] = '\0'; return; }                                      \
        {                                                                                       \
            size_t n_ = strlen(msg);                                                            \
            if (n_ >= sizeof(rupix__last_error)) { n_ = sizeof(rupix__last_error) - 1; }        \
            memcpy(rupix__last_error, msg, n_);                                                 \
            rupix__last_error[n_] = '\0';                                                       \
        }                                                                                       \
    }                                                                                           \
    RUPIX_EXPORT uint32_t iv_abi_version(void) { return RUPIX_ABI_VERSION; }                     \
    RUPIX_EXPORT const RupixDecoderInfo* iv_decoder_info(void) { return &rupix__info; }            \
    RUPIX_EXPORT int iv_last_error(char* buf, size_t len) {                                     \
        size_t n_ = strlen(rupix__last_error) + 1; /* include NUL, like the Rust kit */          \
        if (buf && len > 0) {                                                                   \
            size_t c_ = n_ < len ? n_ : len;                                                    \
            memcpy(buf, rupix__last_error, c_);                                                 \
            buf[c_ - 1] = '\0';                                                                 \
        }                                                                                       \
        return (int)n_;                                                                         \
    }

/* Optional: expose extensible key/value metadata. Add new fields by appending pairs;
 * this does not change any existing struct or RUPIX_ABI_VERSION.
 *
 *   RUPIX_EXPORT_METADATA(
 *       RUPIX_METADATA_FIELD("author", "Your Name"),
 *       RUPIX_METADATA_FIELD("license", "AGPL-3.0"),
 *       RUPIX_METADATA_FIELD("source", "https://example.invalid/source"))
 */
#define RUPIX_EXPORT_METADATA(...)                                                               \
    static const RupixMetadataField rupix__meta_fields[] = { __VA_ARGS__ };                      \
    static const RupixMetadata rupix__meta = {                                                   \
        (uint32_t)sizeof(RupixMetadata), RUPIX_METADATA_VERSION,                                \
        (uint32_t)(sizeof(rupix__meta_fields) / sizeof(rupix__meta_fields[0])), 0,               \
        rupix__meta_fields                                                                       \
    };                                                                                            \
    RUPIX_EXPORT const RupixMetadata* iv_metadata(void) { return &rupix__meta; }

/* Optional: declare which decode kinds this decoder answers for. Put it right after
 * RUPIX_DECLARE_DECODER, in the same translation unit:
 *
 *   RUPIX_DECLARE_DECODER("stb", "STB Image", "psd;hdr", "...", 0, 1)
 *   RUPIX_EXPORT_CAPS(RUPIX_CAP_FULL | RUPIX_CAP_MULTIPAGE)
 *
 * Omit it if you only do full decoding - the host derives caps (see RUPIX_CAP_*). */
#define RUPIX_EXPORT_CAPS(bits) RUPIX_EXPORT uint32_t iv_caps(void) { return (bits); }

/* ---------------------------------------------------------------------------
 * 6. Fault guard: the C counterpart of the Rust kit's catch_unwind gate.
 *
 * MSVC cannot `return` from inside __except, so the pattern is:
 *
 *   int iv_render(...) {
 *       int rc = RUPIX_ERR_OK;
 *       RUPIX_GUARD_ENTER
 *           rc = my_decode(...);        // may fault on hostile input
 *       RUPIX_GUARD_LEAVE(rc)           // faults -> rc = RUPIX_ERR_PANIC
 *       return rc;
 *   }
 *
 * This turns access violations in your decoder into an isolated failure
 * instead of taking down the host process. It does NOT protect against
 * non-terminating loops - that is rule 10 (work must be linear in input size).
 * ------------------------------------------------------------------------ */

#define RUPIX_GUARD_ENTER __try {

#define RUPIX_GUARD_LEAVE(code)                                                                 \
    }                                                                                           \
    __except (1 /* EXCEPTION_EXECUTE_HANDLER */) {                                              \
        rupix_set_last_error("access violation caught by SEH guard");                           \
        (code) = RUPIX_ERR_PANIC;                                                               \
    }

/* ---------------------------------------------------------------------------
 * 7. Entry-point prototypes (what the host resolves from your DLL)
 * ------------------------------------------------------------------------ */

/* iv_probe: 100 = this format's measured champion; 50 = core fallback;
 * 0 = not mine. ext may be NULL - then you may only claim by magic bytes.
 * Returning 0 for a recognized-but-refused subset is how the fallback chain
 * works; never return ERR_* from a probe (it returns a confidence score). */
typedef int32_t (*RupixProbeFn)(const uint8_t* data, size_t len, const char* ext);

/* iv_render: two phases. out == NULL -> query: fill info only
 * (actual_w/actual_h/bytes_needed/pixel_format/...); then the host allocates
 * exactly bytes_needed and calls again with out != NULL. Return RUPIX_ERR_*. */
typedef int32_t (*RupixRenderFn)(const uint8_t* data, size_t len,
                                 const RupixRenderReq* req,
                                 uint8_t* out, size_t out_cap,
                                 RupixRenderOut* info, const char* ext);

#endif /* RUPIX_DECODER_KIT_H */
