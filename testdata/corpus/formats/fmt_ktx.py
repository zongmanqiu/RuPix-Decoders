from ._legacy import ktx as write_legacy


def write(request):
    return write_legacy(request)
