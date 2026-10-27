from __future__ import annotations

import io
import struct
import sys
import unittest
from pathlib import Path

TESTDATA = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TESTDATA))

from PIL import Image

from corpus.formats import fmt_apng, fmt_dib, fmt_ico, fmt_rle
from corpus.spec import Request


def req(ext: str, target: int = 50_000) -> Request:
    return Request(ext=ext, tier="test", target_bytes=target, out_dir=Path("."), seed=9)


class ExtraFormatTest(unittest.TestCase):
    def test_dib_is_headerless_bitmapinfoheader(self):
        out = fmt_dib.write(req("dib"))
        self.assertNotEqual(out.data[:2], b"BM")
        self.assertGreaterEqual(struct.unpack_from("<I", out.data, 0)[0], 40)

    def test_rle_is_bmp_with_rle8_compression(self):
        out = fmt_rle.write(req("rle"))
        self.assertEqual(out.data[:2], b"BM")
        compression = struct.unpack_from("<I", out.data, 30)[0]
        self.assertEqual(compression, 1)

    def test_ico_and_icon_are_ico_containers(self):
        for ext in ("ico", "icon"):
            out = fmt_ico.write(req(ext))
            self.assertEqual(out.data[:4], b"\x00\x00\x01\x00")

    def test_apng_has_two_frames(self):
        out = fmt_apng.write(req("apng"))
        with Image.open(io.BytesIO(out.data)) as im:
            self.assertEqual(im.format, "PNG")
            self.assertGreaterEqual(getattr(im, "n_frames", 1), 2)
