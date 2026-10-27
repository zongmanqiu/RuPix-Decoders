from ._common import write_pillow


def write(request):
    return write_pillow(request, "PPM", mode="1")
