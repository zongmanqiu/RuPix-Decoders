from .fmt_jpg import write as write_jpg
from ._common import _pad_jpeg
from corpus.spec import Generated


def write(request):
    out = write_jpg(request)
    data = _pad_jpeg(out.data, "jps", request.seed)
    return Generated(
        data=data,
        width=out.width,
        height=out.height,
        note=out.note,
        capped=out.capped,
    )
