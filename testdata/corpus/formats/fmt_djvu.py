"""DjVu seed writer.

DjVu needs a JB2/IW44 encoder; no portable encoder is part of the Python
toolchain. Use the checked-in public-domain scan as the seed instead of
fabricating a container. The trailing bytes are format-ignorable and keep
`.djvu`/`.djv` byte lengths distinct within a tier.
"""

from pathlib import Path

from corpus.spec import Generated

SEED = Path(__file__).resolve().parents[2] / "golden" / "djvu_pd_scan.djvu"
WIDTH = 1234
HEIGHT = 1733


def write(request):
    seed = SEED.read_bytes()
    marker = (
        b"\nRuPix-" + request.ext.encode("ascii") + b"-" +
        str(request.seed).encode("ascii") + b"\n"
    )
    data = seed + marker + b"." * 24
    return Generated(
        data=data,
        width=WIDTH,
        height=HEIGHT,
        note=f"capped(SEED {len(seed)} B < {request.target_bytes} B) djvu_pd_scan.djvu",
        capped=True,
    )
