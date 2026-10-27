"""Explicit suffix -> writer module registry.

No filesystem auto-discovery: a missing or renamed format module must fail loudly
at review time, not silently change the generated corpus. One module per suffix
keeps a format's writer readable in isolation; aliases are still explicit modules
so callers can always map an extension to exactly one file.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class FormatEntry:
    ext: str
    module: str
    phase: str
    family: str
    note: str = ""

    @property
    def import_path(self) -> str:
        return f"corpus.formats.{self.module}"


_GROUPS: tuple[tuple[str, str, str, str], ...] = (
    ("A1", "pil", "png", ""),
    ("A1", "jpeg", "jpg jpeg jpe jfif jif jfi exif jps mpo", "aliases share the JPEG core"),
    ("A1", "pil", "gif", ""),
    ("A1", "tiff", "tif tiff", "aliases share the TIFF core"),
    ("A1", "bmp", "bmp dib rle", "aliases share the BMP core"),
    ("A1", "ico", "ico icon cur ani", "aliases share the ICO core"),
    ("A1", "pnm", "ppm pnm pgm pbm", "aliases share the PNM family"),
    ("A1", "pil", "pam", ""),
    ("A1", "pil", "tga", ""),
    ("A1", "pil", "dds", ""),
    ("A1", "pil", "webp", ""),
    ("A1", "pil", "apng", ""),
    ("A1", "pil", "pcx pcc", "aliases share the PCX core"),
    ("A1", "pil", "qoi", ""),
    ("A1", "pil", "hdr", ""),
    ("A1", "pil", "psd", ""),
    ("A1", "jp2", "jp2 j2k j2c jpc jpx jpf jpt", "aliases share the JP2 core"),
    ("A1", "pil", "icns", ""),
    ("A1", "pil", "xbm", ""),
    ("A1", "pil", "wbmp", ""),
    ("A1", "pil", "dcx", ""),
    ("A1", "pil", "ktx", ""),
    ("A1", "pil", "nii", ""),
    ("A1", "pil", "nrrd", ""),
    ("A1", "pil", "mrc mrcs ccp4 map", "aliases share the MRC core"),
    ("A1", "pil", "dcm dicom", "aliases share the DICOM core"),
    ("A1", "pil", "exr", ""),
    ("A1", "pil", "wmf", ""),
    ("A1", "pil", "djvu djv", "aliases share the DjVu core"),
    ("A1", "pil", "ktx2", ""),
    ("A1", "pil", "ase aseprite", "aliases share the Aseprite core"),
    ("A1", "pil", "xcf", ""),
    ("A1", "pil", "ff", ""),
    ("A1", "heif", "heic heif", "aliases share the HEIF core"),
    ("A1", "heif", "hif", ""),
    ("A1", "avif", "avif avifs", "aliases share the AVIF core"),
    ("A1", "jxl", "jxl", ""),
    ("A1", "jxr", "jxr wdp hdp", "aliases share the JXR core"),
    ("A1", "rawpx", "gray gry bgra rgb rgba i420 nv12 uyvy yuv yuy2 yv12", ""),
    ("A2", "raw_tiff", "3fr arw cr2 dcr dcs dng erf fff iiq kdc mef mos mrw nef nrw orf ori pef rw2 rwl srw", "TIFF-derived RAW"),
    ("A2", "raw_bmff", "cr3 crm", "BMFF/CRAW"),
    ("A2", "raw_legacy", "crw qtk raf x3f", "legacy proprietary containers"),
    ("A2", "raw_other", "ari raw", "other proprietary containers"),
)


def _module_name(ext: str) -> str:
    return "fmt_" + ext.replace(".", "_").replace("-", "_")


def _build() -> tuple[FormatEntry, ...]:
    entries: list[FormatEntry] = []
    for phase, family, raw_exts, note in _GROUPS:
        for ext in raw_exts.split():
            entries.append(
                FormatEntry(
                    ext=ext,
                    module=_module_name(ext),
                    phase=phase,
                    family=family,
                    note=note,
                )
            )
    entries.sort(key=lambda e: e.ext)
    return tuple(entries)


# Formats that RuPix can decode but the synthetic generator cannot produce, and
# for which no publicly redistributable sample was found. Any new entry here
# must also be documented in testdata/GENERATION_EXCEPTIONS.md; the test suite
# checks both sides so a future suffix cannot disappear silently.
GENERATION_EXCEPTIONS: dict[str, str] = {
    "crm": (
        "Canon Cinema RAW Light is a video container; no CC0/publicly "
        "redistributable sample found. Local-only test sample."
    ),
    "qtk": (
        "Apple QuickTake 150 RAW still image; no CC0/publicly redistributable "
        "sample found. Local-only test sample."
    ),
}

REGISTRY: tuple[FormatEntry, ...] = _build()
SUFFIXES: tuple[str, ...] = tuple(e.ext for e in REGISTRY)
BY_EXT: dict[str, FormatEntry] = {e.ext: e for e in REGISTRY}


def entry_for(ext: str) -> FormatEntry:
    key = ext.lower().lstrip(".")
    try:
        return BY_EXT[key]
    except KeyError:
        raise KeyError(f"unknown suffix {ext!r}") from None


def select(exts: str | None) -> tuple[FormatEntry, ...]:
    if not exts:
        return REGISTRY
    wanted = {e.strip().lower().lstrip(".") for e in exts.split(",") if e.strip()}
    unknown = sorted(wanted - set(BY_EXT))
    if unknown:
        raise ValueError("unknown suffix(es): " + ", ".join(unknown))
    return tuple(e for e in REGISTRY if e.ext in wanted)
