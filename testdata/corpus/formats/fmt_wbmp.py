from ._legacy import wbmp as write_legacy


def write(request):
    return write_legacy(request)
