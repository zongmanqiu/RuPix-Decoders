from ._legacy import dicom as write_legacy


def write(request):
    return write_legacy(request)
