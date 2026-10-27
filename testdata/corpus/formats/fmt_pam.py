from ._legacy import pam as write_legacy


def write(request):
    return write_legacy(request)
