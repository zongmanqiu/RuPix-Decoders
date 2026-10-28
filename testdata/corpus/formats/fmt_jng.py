from __future__ import annotations

import io
import struct
import zlib

from PIL import Image

from corpus.spec import Generated

from .fmt_jpg import write as write_jpg


def _chunk(kind: bytes, body: bytes) -> bytes:
    return (
        struct.pack(">I", len(body))
        + kind
        + body
        + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
    )


def write(request):
    image = write_jpg(request)
    alpha = Image.new("L", (image.width, image.height))
    alpha.putdata(
        [
            int(255 * y / max(1, image.height - 1))
            for y in range(image.height)
            for _ in range(image.width)
        ]
    )
    alpha_png = io.BytesIO()
    alpha.save(alpha_png, format="PNG")
    png = alpha_png.getvalue()
    offset = 8
    idat = bytearray()
    while offset < len(png):
        length = struct.unpack(">I", png[offset : offset + 4])[0]
        kind = png[offset + 4 : offset + 8]
        body = png[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IDAT":
            idat += body
        if kind == b"IEND":
            break
    jhdr = (
        struct.pack(">II", image.width, image.height)
        + bytes([14, 8, 8, 0, 8, 0, 0, 0])
    )
    data = (
        b"\x8bJNG\r\n\x1a\n"
        + _chunk(b"JHDR", jhdr)
        + _chunk(b"JDAT", image.data)
        + _chunk(b"IDAT", bytes(idat))
        + _chunk(b"IEND", b"")
    )
    return Generated(
        data=data,
        width=image.width,
        height=image.height,
        note="JPEG image with PNG alpha mask",
        capped=image.capped,
    )
