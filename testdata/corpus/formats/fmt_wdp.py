from corpus.spec import Generated

from .fmt_jxr import write as write_jxr


def write(request):
    out = write_jxr(request)
    return Generated(
        out.data + b"\nRuPix-wdp-" + str(request.seed).encode("ascii") + b"\n" + b"." * 17,
        out.width,
        out.height,
        out.note,
    )
