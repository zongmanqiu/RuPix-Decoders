from ._legacy import dds as write_legacy


def write(request):
    return write_legacy(request)
