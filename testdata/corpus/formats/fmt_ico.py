from ._common import write_pillow


def write(request):
    return write_pillow(request, "ICO", mode="RGBA", max_pixels=256 * 256)
