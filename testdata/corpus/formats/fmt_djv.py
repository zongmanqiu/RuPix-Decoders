from .fmt_djvu import write as write_djvu


def write(request):
    return write_djvu(request)
