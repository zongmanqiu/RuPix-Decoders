"""A real RLE8 BMP, not an uncompressed BMP under a different suffix."""

from __future__ import annotations

import math
import random
import struct

from corpus.spec import Generated


MAX_PIXELS = 41_000_000


def write(request):
    target = request.target_bytes
    width = 1024
    # Each random pixel is encoded as (1, value); each row ends with (0, 0).
    row_bytes = width * 2 + 2
    height = max(2, int(math.ceil((target - 1078) / row_bytes)))
    if width * height > MAX_PIXELS:
        height = MAX_PIXELS // width
        capped = True
    else:
        capped = False

    pixels = random.Random(request.seed).randbytes(width * height)
    rows = []
    for y in range(height - 1, -1, -1):
        start = y * width
        for value in pixels[start : start + width]:
            rows.append(bytes((1, value)))
        rows.append(b"\x00\x00")
    image = b"".join(rows)

    file_size = 14 + 40 + 1024 + len(image)
    file_header = struct.pack("<2sIHHI", b"BM", file_size, 0, 0, 1078)
    info = struct.pack(
        "<IiiHHIIiiII",
        40,
        width,
        height,
        1,
        8,
        1,
        len(image),
        0,
        0,
        256,
        256,
    )
    palette = b"".join(bytes((i, i, i, 0)) for i in range(256))
    return Generated(
        data=file_header + info + palette + image,
        width=width,
        height=height,
        note="RLE8" + (" capped(PIXELS)" if capped else ""),
        capped=capped,
    )
