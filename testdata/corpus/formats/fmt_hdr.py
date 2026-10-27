from ._legacy import hdr as write_legacy


def write(request):
    return write_legacy(request)
