"""A real Windows DIB: BMP pixel/file headers minus the 14-byte file header."""

from dataclasses import replace

from corpus.spec import Generated

from .fmt_bmp import write as write_bmp


def write(request):
    # Build 14 extra bytes so stripping the BITMAPFILEHEADER still meets the floor.
    bmp = write_bmp(replace(request, target_bytes=request.target_bytes + 14))
    data = bmp.data[14:]
    if data[:4] != (40).to_bytes(4, "little"):
        raise ValueError("Pillow BMP did not start with a BITMAPINFOHEADER")
    return Generated(data=data, width=bmp.width, height=bmp.height, note="DIB without BITMAPFILEHEADER")
