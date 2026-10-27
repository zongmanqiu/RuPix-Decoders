import io
import math

import pillow_avif  # noqa: F401 - registers the Pillow codec
from PIL import Image

from corpus.spec import Generated

from ._common import _noise, _pad


def _save(frames: list[Image.Image]) -> bytes:
    out = io.BytesIO()
    frames[0].save(
        out,
        format="AVIF",
        save_all=True,
        append_images=frames[1:],
        duration=100,
        loop=0,
    )
    return out.getvalue()


def write(request):
    target = request.target_bytes
    pixels = max(16, int(target * 1.08 / 3))
    side = max(4, int(math.sqrt(pixels)))
    width, height = side, max(4, pixels // side)
    data = b""
    for _ in range(10):
        frames = [_noise(width, height, "RGB", request.seed + i) for i in range(2)]
        data = _pad(_save(frames), "AVIF", request.ext, request.seed)
        if len(data) >= target:
            return Generated(data=data, width=width, height=height)
        scale = min(2.0, max(1.02, math.sqrt(target / len(data)) * 1.02))
        width = max(4, int(width * scale))
        height = max(4, int(height * scale))
    return Generated(data=data, width=width, height=height, note="capped(AVIF sequence)", capped=True)
