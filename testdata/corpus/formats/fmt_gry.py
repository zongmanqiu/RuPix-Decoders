from ._rawpx import write as write_raw


def write(request):
    return write_raw(request, "gry")
