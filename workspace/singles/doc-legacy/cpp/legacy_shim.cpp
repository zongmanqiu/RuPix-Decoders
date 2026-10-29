#include <librevenge/librevenge.h>
#include <librevenge-stream/librevenge-stream.h>
#include <librevenge/RVNGSVGDrawingGenerator.h>
#include <libcdr/libcdr.h>
#include <libwpg/libwpg.h>
#include <libzmf/libzmf.h>
#include <cstdlib>
#include <cstring>
#include <string>

extern "C" int rupix_legacy_probe(const unsigned char *data, unsigned len, const char *ext) {
  if (!data || !len || !ext) return 0;
  std::string e(ext);
  librevenge::RVNGStringStream input(data, len);
  bool ok = false;
  if (e == "cmx" || e == "ccx") ok = libcdr::CMXDocument::isSupported(&input);
  else if (e == "wpg") ok = libwpg::WPGraphics::isSupported(&input);
  else if (e == "zmf") {
    libzmf::ZMFDocument::Type type;
    ok = libzmf::ZMFDocument::isSupported(&input, &type) &&
         type == libzmf::ZMFDocument::TYPE_DRAW;
  } else if (e == "zbr") {
    libzmf::ZMFDocument::Type type;
    ok = libzmf::ZMFDocument::isSupported(&input, &type) &&
         type == libzmf::ZMFDocument::TYPE_ZEBRA;
  }
  return ok ? 100 : 0;
}

extern "C" int rupix_legacy_svg(const unsigned char *data, unsigned len, const char *ext,
                                char **out, unsigned long *out_len) {
  if (!data || !len || !ext || !out || !out_len) return -1;
  std::string e(ext);
  librevenge::RVNGStringVector svg_pages;
  librevenge::RVNGSVGDrawingGenerator painter(svg_pages, librevenge::RVNGString());
  librevenge::RVNGStringStream input(data, len);
  bool ok = false;
  if (e == "cmx" || e == "ccx") {
    ok = libcdr::CMXDocument::isSupported(&input) &&
         libcdr::CMXDocument::parse(&input, &painter);
  } else if (e == "wpg") {
    ok = libwpg::WPGraphics::isSupported(&input) &&
         libwpg::WPGraphics::parse(&input, &painter);
  } else if (e == "zmf") {
    libzmf::ZMFDocument::Type type;
    ok = libzmf::ZMFDocument::isSupported(&input, &type) &&
         type == libzmf::ZMFDocument::TYPE_DRAW &&
         libzmf::ZMFDocument::parse(&input, &painter);
  } else if (e == "zbr") {
    libzmf::ZMFDocument::Type type;
    ok = libzmf::ZMFDocument::isSupported(&input, &type) &&
         type == libzmf::ZMFDocument::TYPE_ZEBRA &&
         libzmf::ZMFDocument::parse(&input, &painter);
  }
  if (!ok || svg_pages.empty()) return -2;
  const char *s = svg_pages[0].cstr();
  unsigned long n = std::strlen(s);
  char *buf = static_cast<char *>(std::malloc(n + 1));
  if (!buf) return -3;
  std::memcpy(buf, s, n + 1);
  *out = buf;
  *out_len = n;
  return 0;
}

extern "C" void rupix_legacy_free(char *p) { std::free(p); }
