"""Headerless raw-pixel dump writer (the host auto-infers these parameters)."""

from __future__ import annotations

import math
import random

from corpus.spec import Generated

import make_samples as legacy


BPP = {
    "gray": 1,
    "gry": 1,
    "rgb": 3,
    "rgba": 4,
    "bgra": 4,
    "i420": 1.5,
    "yuv": 1.5,
    "yv12": 1.5,
    "nv12": 1.5,
    "yuy2": 2,
    "uyvy": 2,
}
LAYOUT = {
    "gray": "GRAY8",
    "gry": "GRAY8",
    "rgb": "RGB8",
    "rgba": "RGBA8",
    "bgra": "BGRA8",
    "i420": "I420",
    "yuv": "I420",
    "yv12": "YV12",
    "nv12": "NV12",
    "yuy2": "YUY2",
    "uyvy": "UYVY",
}
ASPECT = {"gray": 4 / 3, "gry": 5 / 4, "rgb": 4 / 3, "rgba": 3 / 2, "bgra": 4 / 3,
          "i420": 16 / 9, "yuv": 3 / 2, "yv12": 4 / 3, "nv12": 16 / 10,
          "yuy2": 16 / 9, "uyvy": 3 / 2}


def write(request, ext: str) -> Generated:
    bpp = BPP[ext]
    aspect = ASPECT[ext]
    # Stable per-suffix offset prevents two equal-bpp layouts sharing a length.
    target = request.target_bytes + 17 * sum(ext.encode("ascii"))
    pixels = max(4, int(target / bpp))
    cap = 41_000_000 - (sum(ext.encode("ascii")) % 1000)
    capped = False
    if pixels > cap:
        pixels = cap
        capped = True
    width = max(2, int(math.sqrt(pixels * aspect)) // 2 * 2)
    height = max(2, int(pixels / width) // 2 * 2)
    while width * height < pixels:
        height += 2
    data = legacy.encode_rawpx(LAYOUT[ext], width, height, random.Random(request.seed))
    return Generated(
        data=data,
        width=width,
        height=height,
        note="capped(PIXELS)" if capped else "",
        capped=capped,
    )
