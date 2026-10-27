from ._common import write_pillow


def write(request):
    return write_pillow(request, "WEBP", mode="RGB", lossless=True)
