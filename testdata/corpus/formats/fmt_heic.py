import pillow_heif

from ._common import write_pillow

pillow_heif.register_heif_opener()


def write(request):
    return write_pillow(request, "HEIF", mode="RGB", quality=70)
