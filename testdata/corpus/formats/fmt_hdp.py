from corpus.spec import Generated

from .fmt_jxr import write as write_jxr


def write(request):
    out = write_jxr(request)
    return Generated(
        out.data + b"\nRuPix-hdp-" + str(request.seed).encode("ascii") + b"\n" + b"." * 34,
        out.width,
        out.height,
        out.note,
    )
