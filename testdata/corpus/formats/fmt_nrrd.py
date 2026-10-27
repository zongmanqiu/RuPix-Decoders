from ._legacy import nrrd as write_legacy


def write(request):
    return write_legacy(request)
