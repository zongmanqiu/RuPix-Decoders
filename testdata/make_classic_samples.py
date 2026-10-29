#!/usr/bin/env python3
"""Build the 20-file classic sample pack.

Every byte is synthetic (no third-party image), so the pack can be published
without chasing per-file image licenses. PCX/DCX/PNM/PAM/WBMP/XBM are written
here; PICT/ILBM/SGI/XPM come from the `make_legacy` Rust example because their
encoders already live in the classic dependencies.

Usage:
    python testdata/make_classic_samples.py
    python testdata/make_classic_samples.py --out <dir>
"""

import argparse
import hashlib
import json
import shutil
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = ROOT.parent
CLASSIC = ROOT / "workspace" / "singles" / "classic"
SIDE = 16

NAMES = [
    "sample.pcx", "sample.pcc", "sample.dcx",
    "sample.pnm", "sample.ppm", "sample.pgm", "sample.pbm", "sample.pam",
    "sample.wbmp", "sample.xbm", "sample.xpm",
    "sample.pict", "sample.pct", "sample.pic",
    "sample.ilbm", "sample.lbm", "sample.iff",
    "sample.sgi", "sample.iris", "sample.bw",
]


def rgb(x, y):
    r = x * 15
    g = y * 15
    b = 220 if ((x // 4) + (y // 4)) % 2 == 0 else 40
    return r, g, b


def pixels():
    return [rgb(x, y) for y in range(SIDE) for x in range(SIDE)]


def rle(data):
    out = bytearray()
    for b in data:
        if b >= 0xC0:
            out += bytes((0xC1, b))
        else:
            out.append(b)
    return out


def pcx_bytes():
    px = pixels()
    header = struct.pack("<BBBBHHHHHH", 0x0A, 5, 1, 8, 0, 0, SIDE - 1, SIDE - 1, 72, 72)
    header += b"\0" * 48
    header += struct.pack("<BBHHHH", 0, 3, SIDE, 1, 0, 0)
    header += b"\0" * 54
    assert len(header) == 128, len(header)
    body = bytearray()
    for y in range(SIDE):
        for channel in range(3):
            row = bytes(px[y * SIDE + x][channel] for x in range(SIDE))
            body += rle(row)
    return header + body


def dcx(pcx):
    return struct.pack("<II", 0x3ADE68B1, 8) + pcx


def pnm():
    return b"P6\n%d %d\n255\n" % (SIDE, SIDE) + bytes(v for p in pixels() for v in p)


def pgm():
    return b"P5\n%d %d\n255\n" % (SIDE, SIDE) + bytes(
        sum(p) // 3 for p in pixels()
    )


def pbm():
    rows = []
    for y in range(SIDE):
        bits = 0
        for x in range(SIDE):
            # 1 = black. Checkerboard, 4px cells.
            bits = (bits << 1) | (1 if ((x // 4) + (y // 4)) % 2 else 0)
        rows.append((bits << 0).to_bytes(2, "big"))
    return b"P4\n%d %d\n" % (SIDE, SIDE) + b"".join(rows)


def pam():
    header = (
        b"P7\n"
        b"WIDTH 16\nHEIGHT 16\nDEPTH 4\nMAXVAL 255\n"
        b"TUPLTYPE RGB_ALPHA\nENDHDR\n"
    )
    body = bytearray()
    for p in pixels():
        body += bytes((*p, 255))
    return header + bytes(body)


def wbmp():
    row = (SIDE + 7) // 8
    body = bytearray()
    for y in range(SIDE):
        bits = 0
        for x in range(SIDE):
            bits = (bits << 1) | (1 if ((x // 4) + (y // 4)) % 2 else 0)
        body += (bits << (row * 8 - SIDE)).to_bytes(row, "big")
    return bytes((0, 0)) + bytes((SIDE,)) + bytes((SIDE,)) + bytes(body)


def xbm():
    row = (SIDE + 7) // 8
    vals = []
    for y in range(SIDE):
        for byte in range(row):
            bits = 0
            for bit in range(8):
                x = byte * 8 + bit
                if x < SIDE:
                    bits |= (1 if ((x // 4) + (y // 4)) % 2 else 0) << bit
            vals.append(bits)
    data = ", ".join("0x%02x" % v for v in vals)
    return (
        "#define sample_width 16\n"
        "#define sample_height 16\n"
        "static unsigned char sample_bits[] = {%s};\n" % data
    ).encode()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(REPO / ".temp" / "classic-samples"))
    ap.add_argument("--zip", action="store_true", help="also write <out>.zip")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    for old in NAMES:
        p = out / old
        if p.exists():
            p.unlink()

    pcx = pcx_bytes()
    (out / "sample.pcx").write_bytes(pcx)
    (out / "sample.pcc").write_bytes(pcx)
    (out / "sample.dcx").write_bytes(dcx(pcx))
    (out / "sample.pnm").write_bytes(pnm())
    (out / "sample.ppm").write_bytes(pnm())
    (out / "sample.pgm").write_bytes(pgm())
    (out / "sample.pbm").write_bytes(pbm())
    (out / "sample.pam").write_bytes(pam())
    (out / "sample.wbmp").write_bytes(wbmp())
    (out / "sample.xbm").write_bytes(xbm())

    subprocess.run(
        [
            "cargo", "run", "--release", "--example", "make_legacy", "--",
            str(out),
        ],
        cwd=CLASSIC,
        check=True,
    )

    for alias, src in (
        (".pct", ".pict"), (".pic", ".pict"),
        (".lbm", ".ilbm"), (".iff", ".ilbm"),
        (".iris", ".sgi"),
    ):
        shutil.copyfile(out / ("sample" + src), out / ("sample" + alias))

    # `.bw` is the grayscale member of the SGI family: keep one channel.
    sgi = bytearray((out / "sample.sgi").read_bytes())
    sgi[10:12] = (1).to_bytes(2, "big")
    (out / "sample.bw").write_bytes(bytes(sgi[:512 + SIDE * SIDE]))

    manifest = []
    for name in NAMES:
        b = (out / name).read_bytes()
        manifest.append({
            "file": name,
            "ext": name.rsplit(".", 1)[1],
            "bytes": len(b),
            "sha256": hashlib.sha256(b).hexdigest(),
            "width": SIDE,
            "height": SIDE,
            "source": "synthetic",
            "license": "CC0-1.0",
        })
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    (out / "README.md").write_text(
        "# RuPix classic samples\n\n"
        "20 files, one per classic suffix. All pixels are synthetic geometric "
        "patterns generated by `testdata/make_classic_samples.py`; no third-party "
        "image bytes are included. The generated images are dedicated to the "
        "public domain under CC0-1.0 "
        "(<https://creativecommons.org/publicdomain/zero/1.0/>).\n\n"
        "Regenerate:\n\n"
        "```powershell\n"
        "cd RuPix-Decoder\n"
        "python testdata/make_classic_samples.py\n"
        "```\n\n"
        "`manifest.json` carries the size and sha256 of each file.\n",
        encoding="utf-8",
    )
    print("classic samples: %d files -> %s" % (len(NAMES), out))
    for row in manifest:
        print("  %-11s %6d B  %s" % (row["file"], row["bytes"], row["sha256"][:16]))
    if args.zip:
        archive = shutil.make_archive(str(out), "zip", root_dir=out.parent, base_dir=out.name)
        print("zip: %s" % archive)


if __name__ == "__main__":
    main()
