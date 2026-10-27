"""farbfeld: 16-byte header, then big-endian RGBA16 pixels."""

from __future__ import annotations

import math
import random
import struct

from corpus.spec import Generated


def write(request):
    target = request.target_bytes
    pixels = max(1, (target - 16) // 8)
    side = max(1, int(math.sqrt(pixels)))
    width, height = side, max(1, pixels // side)
    if width * height > 41_000_000:
        width = height = 6400
        capped = True
    else:
        capped = False
    data = b"farbfeld" + struct.pack(">II", width, height)
    data += random.Random(request.seed).randbytes(width * height * 8)
    return Generated(
        data=data,
        width=width,
        height=height,
        note="capped(PIXELS)" if capped else "",
        capped=capped,
    )
