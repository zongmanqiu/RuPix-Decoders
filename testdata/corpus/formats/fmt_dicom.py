import struct

from corpus.spec import Generated

from .fmt_dcm import write as write_dcm


def write(request):
    out = write_dcm(request)
    payload = (f"RuPix-dicom-{request.seed}".encode("ascii") + b"\x00")[:200]
    if len(payload) % 2:
        payload += b"\x00"
    # Data Set Trailing Padding (FFFC,FFFC), explicit VR little-endian.
    padding = b"\xfc\xff\xfc\xffOB\x00\x00" + struct.pack("<I", len(payload)) + payload
    return Generated(out.data + padding, out.width, out.height, out.note)
