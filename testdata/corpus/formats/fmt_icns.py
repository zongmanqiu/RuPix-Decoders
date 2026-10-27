from ._common import write_pillow


def write(request):
    # ICNS has a hard practical ceiling; report capped instead of growing forever.
    return write_pillow(request, "ICNS", mode="RGBA", max_pixels=1024 * 1024)
