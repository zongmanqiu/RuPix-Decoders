from ._common import write_pillow


def write(request):
    # Different real ICO dimensions keep .ico/.icon lengths distinct.
    return write_pillow(request, "ICO", mode="RGBA", max_pixels=128 * 128)
