"""Shared raster helpers.

The corpus is noise on purpose: it is deterministic, has no third-party image
rights, and forces entropy coders into their slow path. Dimensions are chosen
from the byte target, then a format-valid metadata trailer makes aliases differ
in length without changing the decoded pixels.
"""

from __future__ import annotations

import io
import math
import random
import struct
import zlib

from PIL import Image

from corpus.spec import Generated, Request


def _noise(width: int, height: int, mode: str, seed: int) -> Image.Image:
    bpp = len(mode)
    data = random.Random(seed).randbytes(width * height * bpp)
    return Image.frombytes(mode, (width, height), data)


def _encode(image: Image.Image, fmt: str, **kwargs) -> bytes:
    out = io.BytesIO()
    image.save(out, format=fmt, **kwargs)
    return out.getvalue()


def _pad_png(data: bytes, ext: str, seed: int) -> bytes:
    payload = f"RuPix-{ext}-{seed}".encode("ascii")
    chunk_type = b"tEXt"
    chunk = (
        struct.pack(">I", len(payload))
        + chunk_type
        + payload
        + struct.pack(">I", zlib.crc32(chunk_type + payload) & 0xFFFFFFFF)
    )
    # Insert before IEND (last 12 bytes) so the file stays a valid PNG.
    return data[:-12] + chunk + data[-12:]


_JPEG_PAD_BIAS = {"jpg": 0, "jpeg": 17, "jpe": 34, "jfif": 51, "jif": 68, "jfi": 85, "exif": 102}


def _pad_jpeg(data: bytes, ext: str, seed: int) -> bytes:
    payload = f"RuPix-{ext}-{seed}".encode("ascii")
    payload += b"." * (8 + _JPEG_PAD_BIAS.get(ext, 0))
    segment = b"\xff\xfe" + struct.pack(">H", len(payload) + 2) + payload
    # Insert before EOI; the COM segment is part of the JPEG stream.
    if data.endswith(b"\xff\xd9"):
        return data[:-2] + segment + data[-2:]
    return data + segment


_TRAIL_PAD_BIAS = {"tif": 0, "tiff": 17, "pcx": 0, "pcc": 34, "ppm": 0, "pnm": 51}


def _pad(data: bytes, fmt: str, ext: str, seed: int) -> bytes:
    if fmt == "PNG":
        return _pad_png(data, ext, seed)
    if fmt == "JPEG":
        return _pad_jpeg(data, ext, seed)
    if fmt in ("TIFF", "PCX", "PPM"):
        # These readers stop at the declared image data / IFD; a trailing
        # comment is tolerated and keeps aliases from sharing a byte length.
        payload = ("\nRuPix-%s-%d\n" % (ext, seed)).encode("ascii")
        return data + payload + b"." * (8 + _TRAIL_PAD_BIAS.get(ext, 0))
    if fmt == "JPEG2000":
        bias = {"jp2": 0, "j2k": 17, "j2c": 34, "jpc": 51, "jpx": 68, "jpf": 85, "jpt": 102}
        payload = ("\nRuPix-%s-%d\n" % (ext, seed)).encode("ascii")
        return data + payload + b"." * (8 + bias.get(ext, 0))
    if fmt in ("HEIF", "AVIF"):
        # A top-level `free` box is legal ISO-BMFF and does not alter pixels.
        bias = {"heic": 0, "heif": 17, "hif": 34, "avif": 0, "avifs": 17}.get(ext, 0)
        payload = ("RuPix-%s-%d" % (ext, seed)).encode("ascii") + b"." * (8 + bias)
        return data + struct.pack(">I", 8 + len(payload)) + b"free" + payload
    return data


def write_pillow(
    request: Request,
    fmt: str,
    mode: str = "RGB",
    quality: int | None = None,
    max_pixels: int = 41_000_000,
    **save_kwargs,
) -> Generated:
    """Write a random image at or above `target_bytes` using Pillow."""
    target = request.target_bytes
    bpp = len(mode)
    # Start a little above target/bpp; random noise barely compresses.
    pixels = max(16, int(target * 1.08 / bpp))
    side = max(4, int(math.sqrt(pixels)))
    width, height = side, max(4, pixels // side)
    if width * height > max_pixels:
        cap = math.sqrt(max_pixels / (width * height))
        width = max(4, int(width * cap))
        height = max(4, int(height * cap))
    if quality is not None:
        save_kwargs["quality"] = quality

    for _ in range(10):
        image = _noise(width, height, mode, request.seed)
        data = _pad(_encode(image, fmt, **save_kwargs), fmt, request.ext, request.seed)
        if len(data) >= target:
            return Generated(data=data, width=width, height=height)
        if width * height >= max_pixels:
            return Generated(
                data=data,
                width=width,
                height=height,
                note="capped(PIXELS)",
                capped=True,
            )
        ratio = target / max(1, len(data))
        scale = min(2.0, max(1.02, math.sqrt(ratio) * 1.02))
        width = max(4, int(width * scale))
        height = max(4, int(height * scale))
        if width * height > max_pixels:
            cap = math.sqrt(max_pixels / (width * height))
            width = max(4, int(width * cap))
            height = max(4, int(height * cap))
            image = _noise(width, height, mode, request.seed)
            data = _pad(_encode(image, fmt, **save_kwargs), fmt, request.ext, request.seed)
            return Generated(
                data=data,
                width=width,
                height=height,
                note="capped(PIXELS)",
                capped=True,
            )

    return Generated(
        data=data,
        width=width,
        height=height,
        note="capped(pillow target)",
        capped=True,
    )
