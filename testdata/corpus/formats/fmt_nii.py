from ._legacy import nii as write_legacy


def write(request):
    return write_legacy(request)
