from ._legacy import dcx as write_legacy


def write(request):
    return write_legacy(request)
