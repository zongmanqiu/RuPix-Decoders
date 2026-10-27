from ._common import write_pillow


def write(request):
    # JPEG 2000 codestream, not a JP2 box wrapper.
    return write_pillow(
        request,
        "JPEG2000",
        mode="RGB",
        quality_mode="rates",
        quality=1,
        no_jp2=True,
    )
