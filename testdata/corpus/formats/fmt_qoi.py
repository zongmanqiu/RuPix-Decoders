from ._legacy import qoi as write_legacy


def write(request):
    return write_legacy(request)
