from ._legacy import mrc as write_legacy


def write(request):
    return write_legacy(request)
