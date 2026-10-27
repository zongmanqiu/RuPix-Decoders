from corpus.spec import Generated

from .fmt_mrc import write as write_mrc


def write(request):
    out = write_mrc(request)
    trailer = b"\nRuPix-ccp4-" + str(request.seed).encode("ascii") + b"\n" + b"." * 34
    return Generated(out.data + trailer, out.width, out.height, out.note)
