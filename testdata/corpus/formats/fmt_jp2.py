from ._common import write_pillow


def write(request):
    return write_pillow(request, "JPEG2000", mode="RGB", quality_mode="rates", quality=1)
