import io
import math
import random

import numpy as np
import OpenEXR

from corpus.spec import Generated


def _encode(width: int, height: int, seed: int) -> bytes:
    rng = random.Random(seed)
    channels = {}
    for name in ("R", "G", "B"):
        data = np.frombuffer(rng.randbytes(width * height * 4), dtype=np.float32).reshape(
            height, width
        )
        channels[name] = OpenEXR.Channel(data)
    out = io.BytesIO()
    OpenEXR.File({"compression": OpenEXR.ZIP_COMPRESSION}, channels).write(out)
    return out.getvalue()


def write(request):
    target = request.target_bytes
    pixels = max(16, int(target * 1.1 / 12))
    side = max(4, int(math.sqrt(pixels)))
    data = b""
    for _ in range(10):
        data = _encode(side, side, request.seed)
        if len(data) >= target:
            return Generated(data=data, width=side, height=side)
        scale = min(2.0, max(1.02, math.sqrt(target / max(1, len(data))) * 1.02))
        side = max(4, int(side * scale))
    return Generated(data=data, width=side, height=side, note="capped(EXR)", capped=True)
