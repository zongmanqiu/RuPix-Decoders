from __future__ import annotations

import sys
import unittest
from pathlib import Path

TESTDATA = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TESTDATA))

from PIL import Image

from corpus.formats import fmt_jpg, fmt_png
from corpus.spec import Request


def request(ext: str, target: int = 200_000) -> Request:
    return Request(ext=ext, tier="test", target_bytes=target, out_dir=Path("."), seed=7)


class PngJpegTest(unittest.TestCase):
    def test_png_is_png_and_meets_floor(self):
        out = fmt_png.write(request("png"))
        self.assertTrue(out.data.startswith(b"\x89PNG\r\n\x1a\n"))
        self.assertGreaterEqual(len(out.data), 200_000)
        with Image.open(__import__("io").BytesIO(out.data)) as im:
            self.assertEqual(im.format, "PNG")
            self.assertEqual(im.size, (out.width, out.height))

    def test_jpeg_and_aliases_are_valid_and_size_unique(self):
        sizes = []
        for ext in ("jpg", "jpeg", "jpe", "jfif", "jif", "jfi", "exif"):
            out = fmt_jpg.write(request(ext))
            self.assertTrue(out.data.startswith(b"\xff\xd8"))
            self.assertTrue(out.data.endswith(b"\xff\xd9"))
            with Image.open(__import__("io").BytesIO(out.data)) as im:
                self.assertEqual(im.format, "JPEG")
            sizes.append(len(out.data))
        self.assertEqual(len(sizes), len(set(sizes)))
