"""XBM is a C-source monochrome bitmap; generate valid text directly."""

from __future__ import annotations

import math
import random

from corpus.spec import Generated


def write(request):
    target = request.target_bytes
    pixels = max(8, target * 8)
    side = max(8, int(math.sqrt(pixels)))
    width, height = side, max(8, pixels // side)
    row = (width + 7) // 8
    if width * height > 41_000_000:
        width = height = 6400
        row = (width + 7) // 8
        capped = True
    else:
        capped = False
    bits = random.Random(request.seed).randbytes(row * height)
    body = ", ".join(f"0x{b:02x}" for b in bits)
    text = (
        f"#define noise_width {width}\n"
        f"#define noise_height {height}\n"
        f"static unsigned char noise_bits[] = {{ {body} }};\n"
    ).encode("ascii")
    return Generated(
        data=text,
        width=width,
        height=height,
        note="capped(PIXELS)" if capped else "",
        capped=capped,
    )
