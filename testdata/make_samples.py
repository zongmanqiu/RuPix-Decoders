"""Generate random-noise ("snow") test images at 1 MiB / 10 MiB / 100 MiB, one per SUFFIX.

This folder holds only the generator. No image ever enters the repo: the samples are
written outside the source tree, to `<repo>/../.temp/testdata/<tier>/noise<ext>` plus a
`manifest.json` (override with --out or RUPIX_SAMPLE_DIR). From the repo root:

    python testdata/make_samples.py                  # every suffix, all three tiers
    python testdata/make_samples.py --only .jpg,.png # a subset (suffixes, or decoder ids)
    python testdata/make_samples.py --tier 10M
    python testdata/make_samples.py --list           # print the plan, write nothing

WHY THE SAMPLES ARE KEYED BY SUFFIX AND NOT BY DECODER
  Many libraries can decode one format and one suffix has exactly one claiming decoder
  (scripts/check_deps.py enforces ext -> single plugin), so "the JPEG test images" are the
  same bytes no matter who reads them. Keeping a copy per decoder meant the same 100 MiB
  file existed up to twelve times and drifted: two decoders could be tested against two
  different "10 MB png". Here a suffix is generated once and shared; the manifest still
  records `claimed_by` so you can see who is supposed to read it.

Four choices worth knowing before editing:

* Noise is the point. It is the worst case for every entropy coder, which is what lets a
  100 MiB file exist inside the size gate at all: the viewer refuses anything over 40 MP or
  160 MB of RGBA (plugins themselves cap at 64 MP, see MAX_PIXELS below), so a bigger sample
  would be a sample the host won't open and we could not measure.
* A tier is a **floor**, not an exact size. Dimensions grow until the encoded file clears
  it. When a format cannot clear a tier inside the pixel bound or its own ceiling (1-bit PBM,
  256 px ICO, lossless WebP at 4096, 1 B/px GRAY8, BC1 .dds at 0.5 B/px), the row reports
  `capped` with the size actually achieved rather than pretending.
* Alias suffixes share the same bytes on purpose: `.jpe .jfif .jif .jfi .exif` are JPEG under
  another suffix, `.dib .rle` are BMP, `.icon` is ICO, `.pnm` is PPM, `.tiff` is TIFF - that is
  how the host routes them and the decoder never sees the suffix as a hint. They are written
  as **hard links** (0 extra bytes; at the 100 MiB tier a copy would be 100 MiB each).
  Caveat: `.rle` gets an uncompressed BMP, so the BMP RLE4/RLE8 code paths stay untested.

Formats with no encoder reachable from here are reported in SKIP and never faked: `.jxl`
(needs cjxl), `.heic/.heif/.avif` (need heif-enc / SVT-AV1), camera RAW (real sensor mosaic
plus vendor metadata - synthetic bytes only ever test "cannot decode"), and core's
`.ff` (no magic, and unlike `.tga` the decoder gives it no extension route, so it could
only ever be a dead sample).

Dependency: Pillow, for the containers it can save. QOI, PAM/P7, Radiance .hdr, .psd, the
DXT1 .dds and the rawpx dumps are hand-encoded below against the same specs our DLLs read.
The .hdr writer emits literal-only scanlines, so the ">128 repeat" branch is exercised by
the crate's own unit tests, not by these samples. .dds is written as BC1 because that is all
core can decode: image-rs' DDS decoder accepts only DXT1/DXT3/DXT5 (plus DX10 headers in the
BC1-BC3 range), so uncompressed RGB/RGBA DDS and BC4-BC7 stay ERR_UNSUPPORTED - Pillow opens
them, the DLL does not, and the compressed sample is the one that tests the real path. That
path also insists on dimensions divisible by 4, hence ROUND below.
"""

import argparse
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
TIERS = [("1M", 1 << 20), ("10M", 10 * (1 << 20)), ("100M", 100 * (1 << 20))]
# Two gates exist and the host's is the tighter one, so samples obey the tighter one:
# plugins answer ERR_TOO_LARGE above 64 MP (the per-decoder `const MAX_PIXELS` in
# decoders/<id>/src/lib.rs - 12 copies, kept equal by scripts/check_caps.py, which also
# checks the mirror value below against main/src/core/pixels.rs), while the viewer refuses
# anything over 40 MP or 160 MB of RGBA (main/src/core/pixels.rs MAX_PIXELS). A sample above
# the host bound is only good for testing the refusal, so we stay under it and every row in
# the manifest is a file the viewer can actually open. To probe the 64 MP plugin gate, call
# the ABI directly with budget_bytes = 0 (the plugin then applies only its own cap).
MAX_PIXELS = 40_000_000
MAX_SIDE = int(math.isqrt(MAX_PIXELS))  # 6324
GROW = 1.4
GROW_ATTEMPTS = 24

# Ceilings that come from the container, not from our gates.
DIM_CAP = {".jpg": 65500, ".png": 65500, ".apng": 65500, ".gif": 65500, ".bmp": 65500,
           ".tif": 65500, ".psd": 30000, ".tga": 65535, ".qoi": 65500, ".pnm": 65500,
           ".pgm": 65500, ".pbm": 65500, ".pam": 65500, ".dds": 16384,
           ".webp": 16383, ".ico": 256}
MODE = {".pgm": "L", ".pbm": "1", ".webp": "RGBA", ".png": "RGBA", ".apng": "RGBA",
        ".ico": "RGBA", ".dds": "RGBA", ".tif": "RGB", ".gif": "RGB"}
# image-rs' DXT path (the only one core's .dds can take) rejects any header whose width or
# height is not a multiple of 4 - Parameter(DimensionMismatch), which core reports as
# ERR_CORRUPT. Real odd-sized BC1 files exist and stay unopenable; the sample snaps to 4 so
# it tests the path that is supposed to work.
ROUND = {".dds": 4}
# Pillow 11 cannot reopen these from the bytes alone (no sniff hook), so "verify by
# reopening" stays quiet about them - the DLL itself is the judge for those formats.
NO_READBACK = {".pam", ".hdr"}
# Suffixes whose file name is the only thing distinguishing them from the source row.
ALIAS_NOTE = {".rle": "无压缩 BMP，BMP 的 RLE4/RLE8 分支仍未覆盖",
              ".exif": "JPEG 字节，EXIF 段未写入"}

RAWPX = {".gray": ("GRAY8", 1), ".gry": ("GRAY8", 1), ".raw": ("GRAY8", 1),
         ".rgb": ("RGB8", 3), ".rgba": ("RGBA8", 4), ".bgra": ("BGRA8", 4),
         ".yuy2": ("YUY2", 2), ".uyvy": ("UYVY", 2),
         ".yuv": ("I420", 1.5), ".i420": ("I420", 1.5), ".yv12": ("YV12", 1.5),
         ".nv12": ("NV12", 1.5)}

# (claimed_by, ext, kind, options). The first field is METADATA, not an output path: one
# sample per suffix is generated once and shared by whoever claims that suffix. kind:
# pil | apng | qoi | pam | hdr | psd | rawpx | alias
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
PLAN += [("jpeg", e, "alias", {"src": ".jpg"}) for e in (".jpe", ".jfif", ".jif", ".jfi", ".exif")]
PLAN += [("tiff", ".tiff", "alias", {"src": ".tif"})]
PLAN += [("core", e, "alias", {"src": s})
         for e, s in ((".dib", ".bmp"), (".rle", ".bmp"), (".icon", ".ico"), (".pnm", ".ppm"))]
PLAN += [("rawpx", e, "rawpx", {}) for e in sorted(RAWPX)]

SKIP = [
    ("jxl", ".jxl", "没有 cjxl / imagecodecs，本机无法编码"),
    ("av", ".heic .heif .hif .avif .avifs", "需要 heif-enc / SVT-AV1 / x265"),
    ("raw", ".arw .cr2 .cr3 .dng .nef .rw2 ...", "相机 RAW 载荷是真传感器马赛克 + 厂商元数据，合成字节只能测'解不出来'"),
    ("thumbdoc", ".skp .ppt .pps .sldprt .sldasm .ipt .iam", "专有容器，只能由对应应用产出"),
    ("core", ".ff", "image-rs 认不出魔数，core 也没给 .ff 留 ext 通道，造出来是死样本"),
]

# The 12 DLL ids, so --only can reject a typo instead of silently generating nothing.
PLUGINS = sorted({p for p, *_ in PLAN} | {p for p, _, _ in SKIP})


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
      ".dds": encode_dds}


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
        if ext in (".jpg", ".jpe", ".jfif", ".jif", ".jfi", ".exif"):
            kw["format"] = "JPEG"
        elif ext == ".ico":
            kw["format"] = "ICO"
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
        side += (-side) % ROUND.get(ext, 1)
    return side, path.stat().st_size, "capped(ATTEMPTS)"


def grow_rawpx(path, ext, tier_bytes, seed):
    """Dimension-first: these dumps have no header, so the geometry is what we choose and
    the manifest is what the host dialog has to be typed with."""
    layout, bpp = RAWPX[ext]
    rng = random.Random(seed)
    w = 4096
    h = int(tier_bytes * 1.05 / (w * bpp)) // 2 * 2  # every 4:2:x layout wants even width/height
    note = ""
    if w * h > MAX_PIXELS:
        h = MAX_PIXELS // w // 2 * 2
        note = "capped(PIXELS)"
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
    ap.add_argument("--out", default=os.environ.get("RUPIX_SAMPLE_DIR",
                    str(ROOT.parent / ".temp" / "testdata")))
    ap.add_argument("--tier", default="all", choices=[t for t, _ in TIERS] + ["all"])
    ap.add_argument("--only", default="",
                    help="comma separated suffixes (.jpg) or decoder ids (jpeg)")
    ap.add_argument("--seed", type=int, default=20261016,
                    help="fixed by default, so two runs give byte-identical samples")
    ap.add_argument("--list", action="store_true", help="print the plan and exit")
    args = ap.parse_args()

    if args.list:
        print("%-11s %-8s %-6s %s" % ("claimed_by", "suffix", "kind", "options"))
        for claimed_by, ext, kind, opts in PLAN:
            print("%-11s %-8s %-6s %s" % (claimed_by, ext, kind, opts or ""))
        for claimed_by, exts, why in SKIP:
            print("%-11s %-8s %-6s %s" % (claimed_by, exts, "skip", why))
        print("\nsamples: one per suffix under <out>/<tier>/noise<ext>; default out = %s"
              % (ROOT.parent / ".temp" / "testdata"))
        return 0

    tiers = TIERS if args.tier == "all" else [t for t in TIERS if t[0] == args.tier]
    known_exts = {ext for _, ext, *_ in PLAN}
    only_exts = set()
    for tok in (t.strip() for t in args.only.split(",")):
        if not tok:
            continue
        if tok in PLUGINS:                                   # decoder id -> all its suffixes
            only_exts |= {e for p, e, *_ in PLAN if p == tok}
        elif tok in known_exts:                              # ".jpg"
            only_exts.add(tok)
        elif "." + tok in known_exts:                        # "jpg"
            only_exts.add("." + tok)
        else:
            ap.error("unknown suffix or decoder id %r; suffixes: %s"
                     % (tok, " ".join(sorted(known_exts))))
    rows = []
    canonical = {}   # (ext, tier) -> row, so aliases can inherit dims and bytes
    out_root = Path(args.out)

    for tname, tbytes in tiers:
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
                    os.link(src_file, path)          # hard link: a 100 MiB tier costs 0 extra
                except OSError:
                    shutil.copyfile(src_file, path)  # other volume / FS without links
                note = "同 %s（硬链接）%s" % (opts["src"],
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

    if not only_exts:
        for claimed_by, exts, why in SKIP:
            rows.append({"claimed_by": claimed_by, "tier": "-", "ext": exts, "kind": "skip",
                         "dims": "-", "bytes": 0, "file": "", "note": why})

    out_root.mkdir(parents=True, exist_ok=True)
    rows = merge_rows(out_root, rows, bool(only_exts) or args.tier != "all")
    (out_root / "manifest.json").write_text(json.dumps(
        {"seed": args.seed, "max_pixels": MAX_PIXELS, "tiers": dict(TIERS),
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
