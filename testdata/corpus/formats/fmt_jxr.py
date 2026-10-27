import math
import random

import imagecodecs
import numpy as np

from corpus.spec import Generated


def _encode(width: int, height: int, seed: int) -> bytes:
    pixels = np.frombuffer(
        random.Random(seed).randbytes(width * height * 3), dtype=np.uint8
    ).reshape(height, width, 3)
    return imagecodecs.jpegxr_encode(pixels, level=1)


def write(request):
    target = request.target_bytes
    pixels = max(16, int(target * 1.2 / 3))
    side = max(4, int(math.sqrt(pixels)))
    data = b""
    for _ in range(10):
        data = _encode(side, side, request.seed)
        if len(data) >= target:
            return Generated(data=data, width=side, height=side)
        scale = min(2.0, max(1.02, math.sqrt(target / max(1, len(data))) * 1.02))
        side = max(4, int(side * scale))
    return Generated(data=data, width=side, height=side, note="capped(JXR)", capped=True)
