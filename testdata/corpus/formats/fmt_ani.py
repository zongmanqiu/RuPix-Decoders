import struct

from corpus.spec import Generated

from ._common import write_pillow


def _chunk(chunk_id: bytes, body: bytes) -> bytes:
    out = bytearray(chunk_id)
    out += struct.pack("<I", len(body))
    out += body
    if len(body) & 1:
        out.append(0)
    return bytes(out)


def write(request):
    icon = write_pillow(request, "ICO", mode="RGBA", max_pixels=256 * 256)
    frames = 2
    rate = 4
    anih = struct.pack("<IIIIIIIII", 36, frames, frames, icon.width, icon.height, 32, 1, rate, 1)
    rates = struct.pack("<I", rate) * frames
    fram = bytearray(b"fram")
    for _ in range(frames):
        fram += _chunk(b"icon", icon.data)
    body = bytearray(b"ACON")
    body += _chunk(b"anih", anih)
    body += _chunk(b"rate", rates)
    body += _chunk(b"LIST", bytes(fram))
    data = b"RIFF" + struct.pack("<I", len(body)) + bytes(body)
    return Generated(
        data=data,
        width=icon.width,
        height=icon.height,
        note="2-frame ANI; capped by ICO 256",
        capped=True,
    )
