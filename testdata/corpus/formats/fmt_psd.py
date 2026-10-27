from ._legacy import psd as write_legacy


def write(request):
    return write_legacy(request)
