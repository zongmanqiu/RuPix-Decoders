"""Generate random-noise ("snow") test images at 1 MiB / 10 MiB / 100 MiB, one per SUFFIX.

This folder holds only the generator. No image ever enters the repo: the samples are
written outside the source tree, to `<repo>/../.temp/testdata/<tier>/noise<ext>` plus a
`manifest.json` (override with --out or RUPIX_SAMPLE_DIR). From the repo root:

    python testdata/make_samples.py                  # every suffix, all three tiers
    python testdata/make_samples.py --only .jpg,.png # a subset (suffixes, or decoder ids)
    python testdata/make_samples.py --tier 10M
    python testdata/make_samples.py --list           # print the plan, write nothing
    python testdata/make_samples.py --check          # verify claims coverage only

WHY THE SAMPLES ARE KEYED BY SUFFIX AND NOT BY DECODER
  Many libraries can decode one format and one suffix has exactly one claiming decoder
  (tools/check_deps.py enforces ext -> single decoder), so "the JPEG test images" are the
  same bytes no matter who reads them. Keeping a copy per decoder meant the same 100 MiB
  file existed up to twelve times and drifted: two decoders could be tested against two
  different "10 MB png". Here a suffix is generated once and shared; the manifest still
  records `claimed_by` so you can see who is supposed to read it.

Four choices worth knowing before editing:

* Noise is the point. It is the worst case for every entropy coder, which is what lets a
  100 MiB file exist inside the size gate at all: the viewer refuses anything over 41 MP or
  164 MB of RGBA (decoders themselves cap at 64 MP, see MAX_PIXELS below), so a bigger sample
  would be a sample the host won't open and we could not measure.
* A tier is a **floor**, not an exact size. Dimensions grow until the encoded file clears
  it. When a format cannot clear a tier inside the pixel bound or its own ceiling (1-bit PBM,
  256 px ICO, 1 B/px GRAY8, BC1 .dds at 0.5 B/px), the row reports
  `capped` with the size actually achieved rather than pretending.
* Alias suffixes keep the same pixels on purpose: `.jpeg .jpe .jfif .jif .jfi .exif` are
  JPEG under another suffix, `.dib .rle` are BMP, `.icon` is ICO, `.pnm` is PPM, `.tiff` is
  TIFF, `.j2k .j2c .jpc .jpx .jpf .jpt` are JP2, `.pcc` is PCX, `.mrcs .ccp4 .map` are MRC
  and `.dicom` is DICOM - that is how the host routes them and the decoder never sees the
  suffix as a hint. They start as hard links, then a final pass gives every file in a tier a
  unique byte length (format-safe trailers; `.heif` gets a `free` box, `.dicom` a trailing
  padding element). This is deliberate: sorting by size must not make two distinct files
  look identical. Caveat: `.rle` gets an uncompressed BMP, so the BMP RLE4/RLE8 code paths
  stay untested.

Formats with no encoder reachable from here are backed by real seeds in `<seed-dir>/raw/<ext>/`
and `<seed-dir>/real/<ext>/` (default `.temp/corpus`): camera RAW, HEIC/AVIF/JXL, EXR/JXR,
XCF/WMF/ASE/DjVu/KTX2 and `.ff`. Seeds are reused at every tier; when a seed is smaller
than a tier the manifest records `capped(SEED ...)`, and a missing seed makes `--strict`
fail. Nothing is fabricated.

Dependency: Pillow, for the containers it can save. QOI, PAM/P7, Radiance .hdr, .psd, DXT1
.dds, DCX, WBMP, KTX1, NIfTI, NRRD, MRC and DICOM are hand-encoded below against the specs
our DLLs read. The .hdr writer emits literal-only scanlines, so the ">128 repeat" branch is
exercised by the crate's own unit tests, not by these samples. .dds is written as BC1 because
that is all core can decode: image-rs' DDS decoder accepts only DXT1/DXT3/DXT5 (plus DX10
headers in the BC1-BC3 range), so uncompressed RGB/RGBA DDS and BC4-BC7 stay
ERR_UNSUPPORTED - Pillow opens them, the DLL does not, and the compressed sample is the one
that tests the real path. That path also insists on dimensions divisible by 4, hence ROUND
below.
"""

import argparse
import io
import shutil
import json
import math
import os
import random
import struct
import sys
from pathlib import Path

Image = None   # Pillow, loaded on first use - see pil()


def pil():
    """Import Pillow lazily. `--list`, the plan table and the hand-written encoders need no
    third-party package, so a bare interpreter can still tell you what the corpus would be
    (and what is skipped) before you install anything."""
    global Image
    if Image is None:
        try:
            from PIL import Image as _pil_image
        except ImportError:
            sys.exit("Pillow is required for the container formats: python -m pip install pillow")
        Image = _pil_image
    return Image

ROOT = Path(__file__).resolve().parent.parent   # this file lives in <repo>/testdata/
DEFAULT_TIERS = {"1M": 1 << 20, "10M": 10 * (1 << 20), "100M": 100 * (1 << 20)}
CONFIG_FILE = ROOT / "testdata" / "samples.json"


def load_config(path):
    """Read the one-switch corpus config. Relative paths are rooted at the
    decoder repository, so the file stays portable."""
    cfg = {
        "output": str(ROOT.parent / ".temp" / "testdata"),
        "seed_dir": str(ROOT.parent / ".temp" / "corpus"),
        "tiers": dict(DEFAULT_TIERS),
        "strict": False,
    }
    p = Path(path)
    if p.is_file():
        cfg.update(json.loads(p.read_text(encoding="utf-8")))
    for key in ("output", "seed_dir"):
        q = Path(cfg[key])
        cfg[key] = q if q.is_absolute() else ROOT / q
    cfg["tiers"] = {str(k): int(v) for k, v in cfg["tiers"].items()}
    return cfg
# Two gates exist and the host's is the tighter one, so samples obey the tighter one:
# decoders answer ERR_TOO_LARGE above 64 MP (the per-decoder `const MAX_PIXELS` in
# each decoder crate - kept equal by tools/check_caps.py, which also
# checks the mirror value below against main/src/core/pixels.rs), while the viewer refuses
# anything over 41 MP or 164 MB of RGBA (main/src/core/pixels.rs MAX_PIXELS). A sample above
# the host bound is only good for testing the refusal, so we stay under it and every row in
# the manifest is a file the viewer can actually open. To probe the 64 MP decoder gate, call
# the ABI directly with budget_bytes = 0 (the decoder then applies only its own cap).
MAX_PIXELS = 41_000_000
MAX_SIDE = int(math.isqrt(MAX_PIXELS))  # 6324
GROW = 1.4
GROW_ATTEMPTS = 24

# Ceilings that come from the container, not from our gates.
DIM_CAP = {".jpg": 65500, ".png": 65500, ".apng": 65500, ".gif": 65500, ".bmp": 65500,
           ".tif": 65500, ".psd": 30000, ".tga": 65535, ".qoi": 65500, ".pnm": 65500,
           ".pgm": 65500, ".pbm": 65500, ".pam": 65500, ".dds": 16384,
           ".webp": 16383, ".ico": 256, ".jp2": 65535, ".pcx": 65535,
           ".icns": 1024, ".xbm": 65535, ".wbmp": 65535, ".dcx": 65535,
           ".ktx": 16384, ".nii": 65535, ".nrrd": 65535, ".mrc": 65535,
           ".dcm": 65535}
MODE = {".pgm": "L", ".pbm": "1", ".webp": "RGBA", ".png": "RGBA", ".apng": "RGBA",
        ".ico": "RGBA", ".dds": "RGBA", ".tif": "RGB", ".gif": "RGB",
        ".jp2": "RGBA", ".pcx": "RGB", ".icns": "RGBA", ".xbm": "1"}
PIL_FORMAT = {".jp2": "JPEG2000", ".pcx": "PCX", ".icns": "ICNS", ".xbm": "XBM"}
# image-rs' DXT path (the only one core's .dds can take) rejects any header whose width or
# height is not a multiple of 4 - Parameter(DimensionMismatch), which core reports as
# ERR_CORRUPT. Real odd-sized BC1 files exist and stay unopenable; the sample snaps to 4 so
# it tests the path that is supposed to work.
ROUND = {".dds": 4, ".dcm": 2}
# Pillow 11 cannot reopen these from the bytes alone (no sniff hook), so "verify by
# reopening" stays quiet about them - the DLL itself is the judge for those formats.
NO_READBACK = {".pam", ".hdr", ".dcm", ".ktx", ".nii", ".nrrd", ".mrc",
                ".mrcs", ".ccp4", ".map", ".dcx", ".wbmp"}
# Suffixes whose file name is the only thing distinguishing them from the source row.
ALIAS_NOTE = {".rle": "无压缩 BMP，BMP 的 RLE4/RLE8 分支仍未覆盖",
              ".exif": "JPEG 字节，EXIF 段未写入"}

RAWPX = {".gray": ("GRAY8", 1), ".gry": ("GRAY8", 1),
         ".rgb": ("RGB8", 3), ".rgba": ("RGBA8", 4), ".bgra": ("BGRA8", 4),
         ".yuy2": ("YUY2", 2), ".uyvy": ("UYVY", 2),
         ".yuv": ("I420", 1.5), ".i420": ("I420", 1.5), ".yv12": ("YV12", 1.5),
         ".nv12": ("NV12", 1.5)}

# Distinct picture shape and pixel budget per rawpx suffix: same-byte layouts
# (`.bgra`/`.rgba`, planar 4:2:0 variants) must not end up with equal lengths.
RAWPX_ASPECT = {".bgra": 4 / 3, ".rgba": 3 / 2, ".gray": 4 / 3, ".gry": 5 / 4,
                ".rgb": 4 / 3, ".i420": 16 / 9, ".yuv": 3 / 2, ".yv12": 4 / 3,
                ".nv12": 16 / 10, ".uyvy": 3 / 2, ".yuy2": 16 / 9}

# Suffixes that cannot be synthesised honestly. They must be backed by a real
# sample in <seed-dir>/raw/<ext>/ or <seed-dir>/real/<ext>/.
RAW_SEED_EXT = (
    ".3fr", ".ari", ".arw", ".cr2", ".cr3", ".crm", ".crw", ".dcr", ".dcs",
    ".dng", ".erf", ".fff", ".iiq", ".kdc", ".mef", ".mos", ".mrw", ".nef",
    ".nrw", ".orf", ".ori", ".pef", ".qtk", ".raf", ".raw", ".rw2", ".rwl",
    ".srw", ".x3f",
)
REAL_SEED_EXT = (
    ".heic", ".heif", ".hif", ".avif", ".avifs", ".jxl", ".exr", ".jxr",
    ".wdp", ".hdp", ".xcf", ".wmf", ".ase", ".aseprite", ".djvu", ".djv",
    ".ktx2", ".ff",
)
SEED_EXT = RAW_SEED_EXT + REAL_SEED_EXT

# (claimed_by, ext, kind, options). The first field is METADATA, not an output path: one
# sample per suffix is generated once and shared by whoever claims that suffix. kind:
# pil | apng | qoi | pam | hdr | psd | rawpx | alias | wbmp | dcx | ktx | nii | nrrd | mrc | dicom
PLAN = [
    ("png", ".png", "pil", {}),
    ("jpeg", ".jpg", "pil", {"quality": 100, "subsampling": 0}),
    ("gif", ".gif", "pil", {}),
    ("tiff", ".tif", "pil", {}),
    ("qoi", ".qoi", "qoi", {}),
    ("common", ".psd", "psd", {}),
    ("common", ".hdr", "hdr", {}),
    ("core", ".apng", "apng", {}),
    ("core", ".webp", "pil", {"lossless": True}),
    ("core", ".bmp", "pil", {}),
    ("core", ".ico", "pil", {}),
    ("core", ".ppm", "pil", {}),
    ("core", ".pgm", "pil", {}),
    ("core", ".pbm", "pil", {}),
    ("core", ".pam", "pam", {}),
    ("core", ".tga", "pil", {}),
    ("core", ".dds", "dds", {}),
]
PLAN += [("jpeg", e, "alias", {"src": ".jpg"})
         for e in (".jpeg", ".jpe", ".jfif", ".jif", ".jfi", ".exif")]
PLAN += [("tiff", ".tiff", "alias", {"src": ".tif"})]
PLAN += [("core", e, "alias", {"src": s})
         for e, s in ((".dib", ".bmp"), (".rle", ".bmp"), (".icon", ".ico"), (".pnm", ".ppm"))]
PLAN += [("rawpx", e, "rawpx", {}) for e in sorted(RAWPX)]

# pro tier. Encodable formats are generated; the rest are resolved from real seeds.
PLAN += [
    ("pro-jp2", ".jp2", "pil", {"format": "JPEG2000"}),
    ("pro-small", ".pcx", "pil", {"format": "PCX"}),
    ("pro-small", ".icns", "pil", {"format": "ICNS"}),
    ("pro-small", ".xbm", "pil", {"format": "XBM"}),
    ("pro-small", ".wbmp", "wbmp", {}),
    ("pro-small", ".dcx", "dcx", {}),
    ("pro-ktx2", ".ktx", "ktx", {}),
    ("pro-sci", ".nii", "nii", {}),
    ("pro-sci", ".nrrd", "nrrd", {}),
    ("pro-sci", ".mrc", "mrc", {}),
    ("pro-dicom", ".dcm", "dicom", {}),
]
PLAN += [("pro-jp2", e, "alias", {"src": ".jp2"})
         for e in (".j2k", ".j2c", ".jpc", ".jpx", ".jpf", ".jpt")]
PLAN += [("pro-small", ".pcc", "alias", {"src": ".pcx"})]
PLAN += [("pro-sci", e, "alias", {"src": ".mrc"})
         for e in (".mrcs", ".ccp4", ".map")]
PLAN += [("pro-dicom", ".dicom", "alias", {"src": ".dcm"})]

# No silent skips: every non-encodable suffix must have a real seed.
SEED_MEMBER = {ext: "raw" for ext in RAW_SEED_EXT}
SEED_MEMBER.update({ext: "real" for ext in REAL_SEED_EXT})
# Every suffix in the claim ledger must be either generated or explicitly skipped.
CLAIMS = ROOT / "testdata" / "claims.tsv"


def coverage_errors():
    planned = {ext for _, ext, _, _ in PLAN}
    seeded = set(SEED_EXT)
    if not CLAIMS.is_file():
        return ["claims ledger missing: %s" % CLAIMS]
    claimed = {}
    for line in CLAIMS.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        tier, member, exts, status, evidence, note = line.split("\t")
        for ext in exts.split(";"):
            claimed["." + ext] = member
    missing = sorted(set(claimed) - planned - seeded)
    extra = sorted((planned | seeded) - set(claimed))
    out = []
    if missing:
        out.append("claimed but not generated/seeded: %s" % " ".join(missing))
    if extra:
        out.append("generated/seeded but not claimed: %s" % " ".join(extra))
    return out


# The four published DLL ids map to their members. `--only common` therefore means every
# suffix the common DLL can read, not just the one member literally named "common".
TIER_MEMBERS = {
    "common": ["jpeg", "png", "gif", "tiff", "qoi", "common", "core"],
    "modern": ["av", "jxl"],
    "raw": ["rawpx", "raw"],
    "pro": ["pro-jp2", "pro-jxr", "pro-exr", "pro-ktx2", "pro-dicom",
            "pro-sci", "pro-small", "pro-xcf", "pro-ase", "pro-djvu", "pro-wmf"],
}
DECODER_IDS = sorted({p for p, *_ in PLAN} | set(SEED_MEMBER.values()))


# ---------------------------------------------------------------------------
# pixel sources
# ---------------------------------------------------------------------------


def noise_bytes(n, rng):
    """n random bytes. Drawn as a 8 MiB block, then repeated to length n: one getrandbits per
    bit across 10^8 pixels is too slow to use. The repeat is invisible to every coder here
    because it is far beyond deflate's 32 KiB window and beyond RGBE's per-scanline RLE - but
    it is a repeat, so two rows 8 MiB apart in a huge file are identical."""
    buf = bytearray(rng.randbytes(min(n, 1 << 23)))
    while len(buf) < n:
        buf.extend(buf[: n - len(buf)])  # slice first: `buf += buf` on the same object raises BufferError
    return bytes(buf[:n])


def snow(mode, side, rng):
    bpp = {"1": 1, "L": 1, "RGB": 3, "RGBA": 4}[mode]
    return pil().frombytes(mode, (side, side), noise_bytes(side * side * bpp, rng))


# ---------------------------------------------------------------------------
# hand-written encoders
# ---------------------------------------------------------------------------


def encode_qoi(side, rng, channels=3):
    """QOI level 30, literal-only. Our decoder implements the whole tag set, but on noise
    the index / diff / luma / run tags buy nothing, so a stream of OP_RGB is both valid and
    the honest worst case."""
    tag = 0xFE if channels == 3 else 0xFF
    rgb = noise_bytes(side * side * 3, rng)
    out = bytearray(b"qoif") + struct.pack(">II", side, side) + bytes([channels, 0])
    for i in range(side * side):
        out.append(tag)
        out += rgb[i * 3:i * 3 + 3]
        if channels == 4:
            out.append(0xFF)
    out += b"\x00" * 7 + b"\x01"
    return bytes(out)


def encode_pam(side, rng):
    """P7 / PAM, depth 4 RGB_ALPHA. image-rs reads it; Pillow will not write it."""
    head = ("P7\nWIDTH %d\nHEIGHT %d\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
            % (side, side)).encode()
    rgb = noise_bytes(side * side * 3, rng)
    out = bytearray(head)
    for i in range(side * side):
        out += rgb[i * 3:i * 3 + 3] + b"\xff"
    return bytes(out)


def encode_psd(side, rng):
    """Uncompressed RGB PSD, 8 bit, three separate planes: what the PSD decoder walks and
    what our psd_expected_len truncation guard measures."""
    rgb = noise_bytes(side * side * 3, rng)
    out = bytearray(b"8BPS") + struct.pack(">H", 1) + bytes(6)
    out += struct.pack(">HIIHH", 3, side, side, 8, 3)
    out += struct.pack(">III", 0, 0, 0) + struct.pack(">H", 0)  # three section lengths + compression
    for c in range(3):
        out += rgb[c::3][: side * side]
    return bytes(out)


def encode_hdr(side, rng):
    """Radiance RGBE, new-style scanlines (required once 8 <= width < 32768). The header is
    **four** bytes - 2, 2, width>>8, width&0xFF - which is what the image crate's HDR
    reader checks (`c1 != 2 || c2 != 2`); writing the usual three bytes makes it fall back
    to reading flat RGBE and report corrupt. Each plane then goes out as literal chunks of
    up to 128 bytes: noise has no runs worth the >128 repeat form, which the RLE branch
    supports and the crate's own unit tests cover."""
    rgb = noise_bytes(side * side * 3, rng)
    body = bytearray()
    for y in range(side):
        row = rgb[y * side * 3:(y + 1) * side * 3]
        planes = [bytearray(row[c::3]) for c in range(3)]
        exp = bytearray(side)
        for x in range(side):
            m = max(planes[0][x], planes[1][x], planes[2][x])
            if m == 0:
                planes[0][x] = planes[1][x] = planes[2][x] = 0
                exp[x] = 0
            else:
                e = m.bit_length() - 1
                shift = 8 - e
                for p in planes:
                    p[x] = min(255, p[x] << shift)
                exp[x] = e + 128
        body += bytes([2, 2, (side >> 8) & 0x7F, side & 0xFF])
        for plane in planes + [exp]:
            for i in range(0, side, 128):
                chunk = plane[i:i + 128]
                body.append(len(chunk))
                body += chunk
    return b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y %d +X %d\n" % (side, side) + bytes(body)


def encode_dds(side, rng):
    """DX9 **DXT1 (BC1)** DDS. core routes .dds to image-rs, and its `dds` feature is a bare
    gate: `DdsDecoder::new` accepts only `pixel_format.flags & DDPF_FOURCC` with FourCC
    DXT1/DXT3/DXT5 (or a DX10 header in the BC1/BC2/BC3 range). An uncompressed RGBA header -
    what Pillow writes and what I wrote first - opens fine in Pillow but answers
    ERR_UNSUPPORTED from the DLL, so the compressed form is the only shape worth sampling.
    Block bytes are random: every 565 endpoint pair and every 2-bit index is legal, so noise
    is a valid BC1 stream and the maximally incompressible one. Layout is 4-byte magic + a
    124-byte header (image-rs requires CAPS|HEIGHT|WIDTH|PIXELFORMAT and rejects any bit
    outside its own valid list) + ceil(side/4)^2 blocks of 8 bytes."""
    blocks = ((side + 3) // 4) ** 2
    dwords = [124, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000, side, side, blocks * 8, 1, 1]
    dwords += [0] * 11  # CAPS|HEIGHT|WIDTH|LINEARSIZE|PIXELFORMAT, then reserved1[11]
    head = b"DDS " + struct.pack("<%dI" % len(dwords), *dwords)
    head += struct.pack("<2I", 32, 0x4) + b"DXT1" + struct.pack("<5I", 0, 0, 0, 0, 0)
    head += struct.pack("<5I", 0x1000, 0, 0, 0, 0)                               # caps1..4 + reserved2
    assert len(head) == 128, len(head)
    return head + noise_bytes(blocks * 8, rng)


def encode_dcx(side, rng):
    """DCX is a tiny wrapper around the first PCX page. pro-small's detector only
    needs the magic, the first-page offset and a valid PCX page at that offset."""
    buf = io.BytesIO()
    snow("RGB", side, rng).save(buf, format="PCX")
    return struct.pack("<II", 0x3ADE68B1, 8) + buf.getvalue()


def _mbf(n):
    """WBMP multi-byte integer (7-bit groups, high bit = more bytes)."""
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    return bytes(reversed(out))


def encode_wbmp(side, rng):
    """WBMP type 0, 1 bit per pixel; header is type/fix + MBF width/height."""
    row = (side + 7) // 8
    bits = bytearray(noise_bytes(row * side, rng))
    if side % 8:
        mask = (0xFF << (8 - side % 8)) & 0xFF
        for y in range(side):
            bits[y * row + row - 1] &= mask
    return b"\x00\x00" + _mbf(side) + _mbf(side) + bytes(bits)


def encode_ktx1(side, rng):
    """KTX1 uncompressed R8G8B8A8_UNORM; enough for pro-ktx2's KTX1 path."""
    ident = b"\xABKTX 11\xBB\r\n\x1A\n"
    header = struct.pack("<13I", 0x04030201, 0x1401, 1, 0x1908, 0x8058, 0x1908,
                         side, side, 0, 0, 1, 1, 0)
    pixels = noise_bytes(side * side * 4, rng)
    return ident + header + struct.pack("<I", len(pixels)) + pixels


def encode_nii(side, rng):
    """NIfTI-1 single-file, uint8 2D slice (pro-sci's minimal path)."""
    h = bytearray(348)
    h[0:4] = struct.pack("<i", 348)
    for off, value in ((40, 2), (42, side), (44, side), (46, 1), (70, 2), (72, 8)):
        h[off:off + 2] = struct.pack("<h", value)
    h[76:80] = struct.pack("<f", 1.0)
    h[108:112] = struct.pack("<f", 352.0)
    h[112:116] = struct.pack("<f", 1.0)
    h[344:348] = b"n+1\0"
    return bytes(h) + b"\0\0\0\0" + noise_bytes(side * side, rng)


def encode_nrrd(side, rng):
    head = ("NRRD0004\ntype: uchar\ndimension: 2\nsizes: %d %d\nencoding: raw\n\n"
            % (side, side)).encode()
    return head + noise_bytes(side * side, rng)


def encode_mrc(side, rng):
    """MRC mode 0 (signed int8), one 2D slice; pro-sci reads the first 1024-byte header."""
    h = bytearray(1024)
    for off, value in ((0, side), (4, side), (8, 1)):
        h[off:off + 4] = struct.pack("<i", value)
    h[208:212] = b"MAP "
    return bytes(h) + noise_bytes(side * side, rng)


def _dcm_elem(group, elem, vr, value):
    value = bytes(value)
    if len(value) % 2:
        value += b"\0" if vr in (b"UI", b"OB") else b" "
    vrvr = vr.encode() if isinstance(vr, str) else vr
    if vrvr in (b"OB", b"OW", b"SQ", b"UN", b"UT"):
        return struct.pack("<HH", group, elem) + vrvr + b"\0\0" + struct.pack("<I", len(value)) + value
    return struct.pack("<HH", group, elem) + vrvr + struct.pack("<H", len(value)) + value


def encode_dicom(side, rng):
    """Minimal explicit-VR little-endian Secondary Capture: RGB8 pixel data."""
    sop = b"1.2.840.10008.5.1.4.1.1.7"
    uid = b"1.2.826.0.1.3680043.8.498.1.1"
    meta = b"".join([
        _dcm_elem(0x0002, 0x0001, b"OB", b"\0\1"),
        _dcm_elem(0x0002, 0x0002, b"UI", sop),
        _dcm_elem(0x0002, 0x0003, b"UI", uid),
        _dcm_elem(0x0002, 0x0010, b"UI", b"1.2.840.10008.1.2.1"),
        _dcm_elem(0x0002, 0x0012, b"UI", uid),
    ])
    meta = _dcm_elem(0x0002, 0x0000, b"UL", struct.pack("<I", len(meta))) + meta
    pixels = noise_bytes(side * side * 3, rng)
    dataset = b"".join([
        _dcm_elem(0x0008, 0x0016, b"UI", sop),
        _dcm_elem(0x0008, 0x0018, b"UI", uid),
        _dcm_elem(0x0008, 0x0060, b"CS", b"OT"),
        _dcm_elem(0x0028, 0x0002, b"US", struct.pack("<H", 3)),
        _dcm_elem(0x0028, 0x0004, b"CS", b"RGB"),
        _dcm_elem(0x0028, 0x0010, b"US", struct.pack("<H", side)),
        _dcm_elem(0x0028, 0x0011, b"US", struct.pack("<H", side)),
        _dcm_elem(0x0028, 0x0100, b"US", struct.pack("<H", 8)),
        _dcm_elem(0x0028, 0x0101, b"US", struct.pack("<H", 8)),
        _dcm_elem(0x0028, 0x0102, b"US", struct.pack("<H", 7)),
        _dcm_elem(0x0028, 0x0103, b"US", struct.pack("<H", 0)),
        _dcm_elem(0x7FE0, 0x0010, b"OB", pixels),
    ])
    return b"\0" * 128 + b"DICM" + meta + dataset


def encode_rawpx(layout, w, h, rng):
    if layout in ("I420", "YV12", "NV12"):
        y = noise_bytes(w * h, rng)
        cb = noise_bytes((w // 2) * (h // 2), rng)
        cr = noise_bytes((w // 2) * (h // 2), rng)
        if layout == "NV12":
            uv = bytearray(len(cb) * 2)
            uv[0::2] = cb
            uv[1::2] = cr
            return y + bytes(uv)
        return y + (cb + cr if layout == "I420" else cr + cb)
    bpp = {"GRAY8": 1, "RGB8": 3, "RGBA8": 4, "BGRA8": 4, "YUY2": 2, "UYVY": 2}[layout]
    px = noise_bytes(w * h * bpp, rng)
    if layout == "BGRA8":
        swapped = bytearray(px)
        swapped[0::4] = px[2::4]
        swapped[2::4] = px[0::4]
        return bytes(swapped)
    return px


# ---------------------------------------------------------------------------
# one file at a time
# ---------------------------------------------------------------------------

HAND = {".qoi": encode_qoi, ".pam": encode_pam, ".psd": encode_psd, ".hdr": encode_hdr,
        ".dds": encode_dds, ".dcx": encode_dcx, ".wbmp": encode_wbmp,
        ".ktx": encode_ktx1, ".nii": encode_nii, ".nrrd": encode_nrrd,
        ".mrc": encode_mrc, ".dcm": encode_dicom}


def write_one(path, kind, ext, side, rng, opts):
    if ext in HAND:
        path.write_bytes(HAND[ext](side, rng))
    elif kind == "apng":
        snow("RGBA", side, rng).save(path, save_all=True,
                                     append_images=[snow("RGBA", side, rng)],
                                     duration=100, loop=0)
    else:
        kw = dict(opts)
        kw["mode"] = MODE.get(ext, "RGB")
        img = snow(kw.pop("mode"), side, rng)
        if ext in (".jpg", ".jpeg", ".jpe", ".jfif", ".jif", ".jfi", ".exif"):
            kw["format"] = "JPEG"
        elif ext == ".ico":
            kw["format"] = "ICO"
        elif ext in PIL_FORMAT:
            kw["format"] = PIL_FORMAT[ext]
        img.save(path, **kw)


def grow(path, kind, ext, opts, tier_bytes, seed):
    """Square dimensions until the encoded file clears the floor, or a cap says stop."""
    rng = random.Random(seed)
    cap = min(DIM_CAP.get(ext, 65535), MAX_SIDE)
    side = 256
    for attempt in range(GROW_ATTEMPTS):
        write_one(path, kind, ext, side, rng, opts)
        size = path.stat().st_size
        if size >= tier_bytes:
            return side, size, ""
        if side >= cap:
            return side, size, "capped(%s)" % ("PIXELS" if ext != ".ico" else "DIM 256")
        per_px = max(size, 1) / float(side * side)
        want = int(math.sqrt(tier_bytes * 1.05 / per_px))
        side = min(cap, max(want, int(side * GROW) + 1))
        r = ROUND.get(ext, 1)
        side += (-side) % r
        if side * side > MAX_PIXELS:
            # DDS rounds 6403 -> 6404, which is just over the 41 MP cap.
            side = (MAX_SIDE // r) * r
    return side, path.stat().st_size, "capped(ATTEMPTS)"


def grow_rawpx(path, ext, tier_bytes, seed):
    """Dimension-first: these dumps have no header, so the geometry is what we choose and
    the manifest is what the host dialog has to be typed with."""
    layout, bpp = RAWPX[ext]
    # Layout is part of the seed: `.rgb` and `.rgba` can share a byte length at
    # different geometry. Before this they were byte-identical and looked like
    # accidental duplicate files.
    rng = random.Random(seed ^ (sum(ext.encode("ascii")) << 8) ^ int(bpp * 1000) * 131)
    # A believable non-strip shape plus a per-suffix pixel budget. The budget must
    # differ when bpp is equal, otherwise `.bgra`/`.rgba` remain byte-length twins.
    idx = list(RAWPX).index(ext)
    aspect = RAWPX_ASPECT[ext]
    cap = MAX_PIXELS - (idx + 1) * 1000
    pixels = max(4, int(tier_bytes * (1.05 + idx * 0.01) / bpp))
    note = ""
    if pixels > cap:
        pixels = cap
        note = "capped(PIXELS)"
    w = max(2, int((pixels * aspect) ** 0.5) // 2 * 2)
    h = max(2, int(pixels / w) // 2 * 2)
    while w * h < pixels:
        h += 2
    while w * h > cap and h > 2:
        h -= 2
    path.write_bytes(encode_rawpx(layout, w, h, rng))
    return "%dx%d" % (w, h), path.stat().st_size, note, layout


def row(claimed_by, tier, ext, kind, path, dims, note=""):
    return {"claimed_by": claimed_by, "tier": tier, "ext": ext, "kind": kind, "dims": dims,
            "bytes": path.stat().st_size, "file": str(path).replace("\\", "/"), "note": note}


def have_pil():
    try:
        import PIL  # noqa: F401
        return True
    except ImportError:
        return False


def readback(path, ext):
    """Reopen with Pillow to prove the bytes are a real image. Not a substitute for running
    the DLL, but it does catch a broken hand-written header immediately. Degrades quietly
    when Pillow is absent: the hand-written encoders can still run without it, and their
    output is judged by the DLL, not by this check."""
    if ext in NO_READBACK or not have_pil():
        return ""
    try:
        with pil().open(path) as im:
            im.verify()
        return ""
    except Exception as exc:
        return "读不回 %s" % repr(exc)[:70]


def seed_path(seed_dir, ext, tier_bytes=None):
    """Find the real seed closest to a tier. Prefer the largest seed at or below
    the tier; if none fits, use the smallest available (never the 100 MB file in
    the 1M tier). Prefer the exact suffix (e.g. .DCS) over an archive .TIF alias."""
    family = "raw" if ext in RAW_SEED_EXT else "real"
    folder = Path(seed_dir) / family / ext.lstrip(".")
    if not folder.is_dir():
        return None
    files = [p for p in folder.iterdir() if p.is_file()]
    if not files:
        return None
    exact = [p for p in files if p.suffix.lower() == ext.lower()]
    files = exact or files
    if tier_bytes is None:
        return min(files, key=lambda p: p.stat().st_size)
    # Pick the seed whose size is closest to the requested tier. This keeps a
    # 121 MB PEF out of the 1 MiB directory when an 11 MB PEF exists, while the
    # 100 MiB tier still gets the larger file.
    return min(files, key=lambda p: abs(p.stat().st_size - tier_bytes))


def key(r):
    # One sample per (suffix, tier) - the shared corpus has no per-decoder dimension.
    return (r["ext"], r["tier"])


def merge_rows(out_root, new_rows, partial):
    """A --only/--tier run regenerates a slice of the catalogue; the manifest describes the
    whole thing, so keep the rows we did not rewrite this time. Files on disk are the truth:
    a kept row whose sample no longer exists is dropped."""
    path = out_root / "manifest.json"
    if not (partial and path.exists()):
        return new_rows
    old = json.loads(path.read_text(encoding="utf-8"))
    fresh = {key(r) for r in new_rows}
    kept = [r for r in old.get("rows", []) if key(r) not in fresh and (not r["file"] or Path(r["file"]).exists())]
    return kept + new_rows


def main():
    ap = argparse.ArgumentParser(
        description="Generate 1/10/100 MiB noise samples, one per suffix, shared by all decoders.")
    ap.add_argument("--config", default=str(CONFIG_FILE),
                    help="one-switch JSON config (output, seed_dir, tiers, strict)")
    ap.add_argument("--out", default=os.environ.get("RUPIX_SAMPLE_DIR"))
    ap.add_argument("--seed-dir", default=os.environ.get("RUPIX_SEED_DIR"))
    ap.add_argument("--strict", action="store_true",
                    help="require a real seed file for every seed-only suffix")
    ap.add_argument("--tier", default="all")
    ap.add_argument("--only", default="",
                    help="comma separated suffixes (.jpg) or decoder ids (jpeg)")
    ap.add_argument("--seed", type=int, default=20261016,
                    help="fixed by default, so two runs give byte-identical samples")
    ap.add_argument("--list", action="store_true", help="print the plan and exit")
    ap.add_argument("--check", action="store_true",
                    help="verify that every claims.tsv suffix is generated or seeded, write nothing")
    args = ap.parse_args()
    cfg = load_config(args.config)
    if args.out:
        cfg["output"] = Path(args.out)
    if args.seed_dir:
        cfg["seed_dir"] = Path(args.seed_dir)
    if args.strict:
        cfg["strict"] = True
    tiers = cfg["tiers"]
    if args.tier != "all" and args.tier not in tiers:
        ap.error("unknown tier %r; available: %s" % (args.tier, ", ".join(tiers)))

    coverage = coverage_errors()
    if coverage:
        for line in coverage:
            print("COVERAGE: " + line, file=sys.stderr)
        return 1

    if cfg["strict"]:
        missing = [ext for ext in SEED_EXT if seed_path(cfg["seed_dir"], ext) is None]
        if missing:
            print("MISSING real seeds under %s: %s"
                  % (cfg["seed_dir"], " ".join(missing)), file=sys.stderr)
            return 1

    if args.check:
        planned = {ext for _, ext, _, _ in PLAN}
        seeded = set(SEED_EXT)
        print("sample-plan: %d generated, %d real seeds, %d covered"
              % (len(planned), len(seeded), len(planned | seeded)))
        return 0

    if args.list:
        print("%-11s %-8s %-6s %s" % ("claimed_by", "suffix", "kind", "options"))
        for claimed_by, ext, kind, opts in PLAN:
            print("%-11s %-8s %-6s %s" % (claimed_by, ext, kind, opts or ""))
        for ext, member in sorted(SEED_MEMBER.items()):
            src = seed_path(cfg["seed_dir"], ext)
            print("%-11s %-8s %-6s %s" % (member, ext, "seed", src or "MISSING"))
        print("\nsamples: one per suffix under <out>/<tier>/noise<ext>; default out = %s"
              % (ROOT.parent / ".temp" / "testdata"))
        return 0

    selected_tiers = list(tiers.items()) if args.tier == "all" else [(args.tier, tiers[args.tier])]
    known_exts = {ext for _, ext, *_ in PLAN} | set(SEED_EXT)
    only_exts = set()
    only_filter = bool(args.only.strip())
    for tok in (t.strip() for t in args.only.split(",")):
        if not tok:
            continue
        if tok in TIER_MEMBERS:                                  # DLL id -> all its members
            only_exts |= {e for p, e, *_ in PLAN if p in TIER_MEMBERS[tok]}
            only_exts |= {e for e, m in SEED_MEMBER.items() if m in TIER_MEMBERS[tok]}
        elif tok in DECODER_IDS:                                 # member id -> all its suffixes
            only_exts |= {e for p, e, *_ in PLAN if p == tok}
            only_exts |= {e for e, m in SEED_MEMBER.items() if m == tok}
        elif tok in known_exts:                              # ".jpg"
            only_exts.add(tok)
        elif "." + tok in known_exts:                        # "jpg"
            only_exts.add("." + tok)
        else:
            ap.error("unknown suffix or decoder id %r; suffixes: %s"
                     % (tok, " ".join(sorted(known_exts))))

    # An alias reads from its canonical file, so asking for only `.jpeg` must also build
    # `.jpg` (resolve chains such as alias -> alias defensively).
    alias_src = {ext: opts["src"] for _, ext, kind, opts in PLAN if kind == "alias"}
    changed = True
    while changed:
        changed = False
        for ext in list(only_exts):
            src = alias_src.get(ext)
            if src and src not in only_exts:
                only_exts.add(src)
                changed = True

    if only_filter and not only_exts:
        print("no suffix in %r; see --list for generated and seeded rows"
              % args.only, file=sys.stderr)
        return 0

    rows = []
    canonical = {}   # (ext, tier) -> row, so aliases can inherit dims and bytes
    out_root = Path(cfg["output"])

    for tname, tbytes in selected_tiers:
        tdir = out_root / tname
        tdir.mkdir(parents=True, exist_ok=True)
        for claimed_by, ext, kind, opts in PLAN:
            if only_exts and ext not in only_exts:
                continue
            if kind == "alias":
                # PLAN lists every alias after its source, so canonical always has it.
                src = canonical[(opts["src"], tname)]
                src_file = Path(src["file"])
                path = tdir / ("noise" + ext)
                if path.exists():
                    path.unlink()
                try:
                    os.link(src_file, path)          # hard link: dedupe pass copies only when needed
                except OSError:
                    shutil.copyfile(src_file, path)  # other volume / FS without links
                note = "同 %s（同图）%s" % (opts["src"],
                                          ("；" + ALIAS_NOTE[ext]) if ext in ALIAS_NOTE else "")
                rows.append(row(claimed_by, tname, ext, kind, path, src["dims"], note))
                canonical[(ext, tname)] = rows[-1]
                print("  %-6s %-7s %10d B  %s" % (tname, ext, rows[-1]["bytes"], note),
                      file=sys.stderr)
                continue

            path = tdir / ("noise" + ext)

            if kind == "rawpx":
                dims, size, note, layout = grow_rawpx(path, ext, tbytes, args.seed)
                r = row(claimed_by, tname, ext, kind, path, dims,
                        (note + " " if note else "") + "layout=%s 按此宽高在宿主里手填" % layout)
                r["layout"] = layout  # iv_render_raw wants it as a PF_* number
            else:
                side, size, note = grow(path, kind, ext, opts, tbytes, args.seed)
                r = row(claimed_by, tname, ext, kind, path, "%dx%d" % (side, side),
                        note or readback(path, ext))
            print("  %-6s %-7s %10d B  %s" % (tname, ext, r["bytes"], r["note"]),
                  file=sys.stderr)
            rows.append(r)
            canonical[(ext, tname)] = r

        # Real seeds replace every suffix that has no honest encoder. The seed
        # is reused at every tier; if it is smaller than the tier floor it is
        # reported as capped instead of being padded or fabricated.
        for ext, claimed_by in sorted(SEED_MEMBER.items()):
            if only_exts and ext not in only_exts:
                continue
            src = seed_path(cfg["seed_dir"], ext, tbytes)
            if src is None:
                raise SystemExit("missing real seed for %s under %s" % (ext, cfg["seed_dir"]))
            path = tdir / ("noise" + ext)
            if path.exists():
                path.unlink()
            try:
                os.link(src, path)
            except OSError:
                shutil.copyfile(src, path)
            actual = path.stat().st_size
            note = "seed %s" % src.name
            if actual < tbytes:
                note = "capped(SEED %d B < %d B) %s" % (actual, tbytes, src.name)
            r = row(claimed_by, tname, ext, "seed", path, "-", note)
            rows.append(r)
            print("  %-6s %-7s %10d B  %s" % (tname, ext, actual, note), file=sys.stderr)

    out_root.mkdir(parents=True, exist_ok=True)
    rows = merge_rows(out_root, rows, bool(only_exts) or args.tier != "all")

    # Hard invariant: within one tier every file has a distinct byte length. Alias
    # suffixes and reused real seeds otherwise collide. Break the tie by writing a
    # small, format-tolerant trailer to all but the first file in each group.
    for tname, _tbytes in selected_tiers:
        by_size = {}
        for r in rows:
            if r["tier"] == tname and r["file"]:
                by_size.setdefault(r["bytes"], []).append(r)
        used = set(by_size)
        for same in by_size.values():
            if len(same) < 2:
                continue
            for r in same[1:]:
                if r["kind"] == "rawpx":
                    raise SystemExit("rawpx size collision needs a dimension change: %s" % r["ext"])
                p = Path(r["file"])
                tag = ("\nRuPix unique-size %s\n" % r["ext"]).encode("ascii")
                while len(tag) + r["bytes"] in used:
                    tag += b"."
                tmp = p.with_name(p.name + ".dedupe")
                shutil.copyfile(p, tmp)  # break a possible hard link before writing
                p.unlink()
                os.replace(tmp, p)
                if r["ext"] in (".heic", ".heif"):
                    # A top-level `free` box is legal ISO-BMFF; raw trailing bytes are not.
                    body = struct.pack(">I", 8 + len(tag)) + b"free" + tag
                elif r["ext"] == ".dicom":
                    # DICOM Data Set Trailing Padding (FFFC,FFFC), explicit VR little-endian.
                    payload = tag + (b"\0" if len(tag) % 2 else b"")
                    body = b"\xfc\xff\xfc\xffOB\x00\x00" + struct.pack("<I", len(payload)) + payload
                else:
                    body = tag
                with p.open("ab") as f:
                    f.write(body)
                r["bytes"] = p.stat().st_size
                r["note"] = (r["note"] + "；长度唯一（尾注 %d B）" % len(body)).strip("；")
                used.add(r["bytes"])

    for tname, _tbytes in selected_tiers:
        sizes = {}
        for r in rows:
            if r["tier"] != tname or not r["file"]:
                continue
            sizes.setdefault(r["bytes"], []).append(r["ext"])
        dup = {n: exts for n, exts in sizes.items() if len(exts) > 1}
        if dup:
            raise SystemExit("duplicate file sizes in %s: %r" % (tname, dup))
    (out_root / "manifest.json").write_text(json.dumps(
         {"seed": args.seed, "max_pixels": MAX_PIXELS, "tiers": dict(selected_tiers),
         "keyed_by": "suffix (one sample per suffix per tier, shared by all decoders)",
         "rows": rows}, indent=1), encoding="utf-8")

    print("%-11s %-5s %-7s %12s %-11s %s"
          % ("claimed_by", "tier", "suffix", "bytes", "dims", "note"))
    for r in rows:
        print("%-11s %-5s %-7s %12s %-11s %s"
              % (r["claimed_by"], r["tier"], r["ext"], r["bytes"], r["dims"], r["note"]))
    made = [r for r in rows if r["kind"] != "skip"]
    short = [r for r in made if r["note"].startswith(("capped", "读不回", "先跑"))]
    print("文件 %d 个，未达标/读不回 %d 个，声明但造不出的格式 %d 组 -> %s"
          % (len(made), len(short), len(rows) - len(made), out_root))
    return 0


if __name__ == "__main__":
    sys.exit(main())
