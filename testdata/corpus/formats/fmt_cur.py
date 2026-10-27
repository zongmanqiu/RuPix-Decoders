from ._common import write_pillow


def write(request):
    out = write_pillow(request, "ICO", mode="RGBA", max_pixels=256 * 256)
    data = bytearray(out.data)
    data[2:4] = b"\x02\x00"
    # ICO readers stop after the declared image entries. A short trailing
    # marker keeps CUR from sharing a byte length with ICO (manifest gate).
    data += b"RuPix-CUR"
    return out.__class__(
        data=bytes(data),
        width=out.width,
        height=out.height,
        note=out.note,
        capped=out.capped,
    )
