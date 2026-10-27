"""Adapters for the hand-written encoders that already live in make_samples.py.

These encoders are ours and write the real container layout; this module only
chooses a square side from the byte floor and keeps the old code single-sourced
until it is moved into per-format helpers.
"""

from __future__ import annotations

import math
import random

from corpus.spec import Generated, Request

import make_samples as legacy


MAX_PIXELS = 41_000_000


def fit(request: Request, encoder, bytes_per_pixel: float, *, multiple: int = 1) -> Generated:
    target = request.target_bytes
    side = max(multiple, int(math.sqrt(target / bytes_per_pixel)))
    side = max(multiple, (side // multiple) * multiple)
    data = b""
    for _ in range(10):
        data = encoder(side, random.Random(request.seed))
        if len(data) >= target:
            return Generated(data=data, width=side, height=side)
        if side * side >= MAX_PIXELS:
            return Generated(
                data=data,
                width=side,
                height=side,
                note="capped(PIXELS)",
                capped=True,
            )
        side = max(side + multiple, int(side * 1.08))
        side = (side // multiple) * multiple
    return Generated(data=data, width=side, height=side, note="capped(legacy)", capped=True)


def qoi(request):
    return fit(request, legacy.encode_qoi, 3)


def pam(request):
    return fit(request, legacy.encode_pam, 4)


def psd(request):
    return fit(request, legacy.encode_psd, 3)


def hdr(request):
    return fit(request, legacy.encode_hdr, 4)


def dds(request):
    return fit(request, legacy.encode_dds, 0.5, multiple=4)


def dcx(request):
    return fit(request, legacy.encode_dcx, 3)


def wbmp(request):
    return fit(request, legacy.encode_wbmp, 1 / 8)


def ktx(request):
    return fit(request, legacy.encode_ktx1, 4)


def nii(request):
    return fit(request, legacy.encode_nii, 1)


def nrrd(request):
    return fit(request, legacy.encode_nrrd, 1)


def mrc(request):
    return fit(request, legacy.encode_mrc, 1)


def dicom(request):
    return fit(request, legacy.encode_dicom, 3)
