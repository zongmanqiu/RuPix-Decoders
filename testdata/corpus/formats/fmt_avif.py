import pillow_avif  # noqa: F401 - registers the Pillow codec

from ._common import write_pillow


def write(request):
    return write_pillow(request, "AVIF", mode="RGB", quality=70)
