/* Linux-only glibc/libstdc++ compatibility shim.
 *
 * pyke's prebuilt ONNX Runtime (the ort-sys "dfbin" static archive pulled in
 * by the `ort` crate) is compiled against glibc >= 2.38 and GCC 13's
 * libstdc++. Linking it on older glibc (e.g. ubuntu-22.04, glibc 2.35) fails
 * with undefined symbols:
 *
 *   __isoc23_strtol / __isoc23_strtoll / __isoc23_strtoull ...
 *       C23 strtol-family and scanf-family entry points, introduced in
 *       glibc 2.38.
 *   std::string::_M_replace_cold
 *       An out-of-line cold path of basic_string::replace(), introduced in
 *       GCC 13's libstdc++. Both the char and wchar_t instantiations are
 *       referenced by pyke's prebuilt objects (e.g. logging.cc.o uses
 *       std::wstring), so both are provided here.
 *
 * This translation unit provides the missing symbols so the final binary
 * still floors at the glibc version the rest of the build targets (2.35):
 *
 * - The __isoc23_* wrappers delegate to their C17 counterparts. The only
 *   behavioral difference of the C23 variants is binary-literal ("0b...")
 *   parsing for base 0/2, which ONNX Runtime does not rely on.
 * - _M_replace_cold is a verbatim port of GCC 13.3's implementation
 *   (libstdc++-v3/include/bits/basic_string.tcc:480-506, basic_string<char>),
 *   with _S_move -> memmove and _S_copy -> memcpy, exported under its
 *   Itanium ABI mangled name:
 *   std::__cxx11::basic_string<char, std::char_traits<char>,
 *   std::allocator<char>>::_M_replace_cold(char*, unsigned long,
 *   char const*, unsigned long, unsigned long)
 *
 * - The wchar_t instantiation is the same port with _S_move -> wmemmove
 *   and _S_copy -> wmemcpy (wchar_t is 4 bytes on Linux; lengths are in
 *   elements, not bytes), exported as:
 *   std::__cxx11::basic_string<wchar_t, std::char_traits<wchar_t>,
 *   std::allocator<wchar_t>>::_M_replace_cold(wchar_t*, unsigned long,
 *   wchar_t const*, unsigned long, unsigned long)
 *
 * Compiled and linked by build.rs only for linux targets; on glibc >= 2.38
 * systems the dynamic linker resolves these definitions (in the executable)
 * ahead of libc's, which is fine — they are behaviorally equivalent.
 */

#include <inttypes.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/* ---- glibc 2.38 C23 strtol/scanf-family entry points -------------------- */

long __isoc23_strtol(const char *nptr, char **endptr, int base) {
  return strtol(nptr, endptr, base);
}

unsigned long __isoc23_strtoul(const char *nptr, char **endptr, int base) {
  return strtoul(nptr, endptr, base);
}

long long __isoc23_strtoll(const char *nptr, char **endptr, int base) {
  return strtoll(nptr, endptr, base);
}

unsigned long long __isoc23_strtoull(const char *nptr, char **endptr,
                                     int base) {
  return strtoull(nptr, endptr, base);
}

intmax_t __isoc23_strtoimax(const char *nptr, char **endptr, int base) {
  return strtoimax(nptr, endptr, base);
}

uintmax_t __isoc23_strtoumax(const char *nptr, char **endptr, int base) {
  return strtoumax(nptr, endptr, base);
}

int __isoc23_sscanf(const char *s, const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  int r = vsscanf(s, fmt, ap);
  va_end(ap);
  return r;
}

int __isoc23_vsscanf(const char *s, const char *fmt, va_list ap) {
  return vsscanf(s, fmt, ap);
}

int __isoc23_fscanf(FILE *stream, const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  int r = vfscanf(stream, fmt, ap);
  va_end(ap);
  return r;
}

int __isoc23_vfscanf(FILE *stream, const char *fmt, va_list ap) {
  return vfscanf(stream, fmt, ap);
}

int __isoc23_scanf(const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  int r = vscanf(fmt, ap);
  va_end(ap);
  return r;
}

int __isoc23_vscanf(const char *fmt, va_list ap) { return vscanf(fmt, ap); }

/* ---- GCC 13 libstdc++ basic_string<char>::_M_replace_cold --------------- */

void clipboard__replace_cold(void *self, char *p, unsigned long len1, const char *s,
                             unsigned long len2, unsigned long how_much);

/* Alias to the same-TU implementation below, exported under the mangled
 * name the GCC-13-compiled ort objects reference (verified against GCC's
 * libstdc++ ABI baseline, GLIBCXX_3.4.31). */
void _ZNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEE15_M_replace_coldEPcmPKcmm(
    void *self, char *p, unsigned long len1, const char *s, unsigned long len2,
    unsigned long how_much)
    __attribute__((alias("clipboard__replace_cold")));

void clipboard__replace_cold(void *self, char *p, unsigned long len1, const char *s,
                             unsigned long len2, unsigned long how_much) {
  /* A non-static C++ member receives this before its explicit arguments.
   * The mangled name omits this, but the ABI does not. */
  (void)self;
  /* Work in-place; verbatim port of GCC 13.3 basic_string.tcc:480-506
   * (_S_move -> memmove, _S_copy -> memcpy for char_traits<char>). */
  if (len2 && len2 <= len1)
    memmove(p, s, len2);
  if (how_much && len1 != len2)
    memmove(p + len2, p + len1, how_much);
  if (len2 > len1) {
    if (s + len2 <= p + len1) {
      memmove(p, s, len2);
    } else if (s >= p + len1) {
      const unsigned long poff = (unsigned long)(s - p) + (len2 - len1);
      memcpy(p, p + poff, len2);
    } else {
      const unsigned long nleft = (unsigned long)((p + len1) - s);
      memmove(p, s, nleft);
      memcpy(p + nleft, p + len2, len2 - nleft);
    }
  }
}

/* ---- GCC 13 libstdc++ basic_string<wchar_t>::_M_replace_cold ------------ */

void clipboard__replace_cold_wide(void *self, wchar_t *p, unsigned long len1,
                                  const wchar_t *s, unsigned long len2,
                                  unsigned long how_much);

/* Alias to the same-TU implementation below, exported under the mangled
 * name the GCC-13-compiled ort objects reference (Itanium ABI mangling of
 * the wchar_t instantiation, identical to the char version with
 * c -> w). */
void _ZNSt7__cxx1112basic_stringIwSt11char_traitsIwESaIwEE15_M_replace_coldEPwmPKwmm(
    void *self, wchar_t *p, unsigned long len1, const wchar_t *s, unsigned long len2,
    unsigned long how_much)
    __attribute__((alias("clipboard__replace_cold_wide")));

void clipboard__replace_cold_wide(void *self, wchar_t *p, unsigned long len1,
                                  const wchar_t *s, unsigned long len2,
                                  unsigned long how_much) {
  (void)self;
  /* Work in-place; verbatim port of GCC 13.3 basic_string.tcc:480-506
   * (_S_move -> wmemmove, _S_copy -> wmemcpy for char_traits<wchar_t>). */
  if (len2 && len2 <= len1)
    wmemmove(p, s, len2);
  if (how_much && len1 != len2)
    wmemmove(p + len2, p + len1, how_much);
  if (len2 > len1) {
    if (s + len2 <= p + len1) {
      wmemmove(p, s, len2);
    } else if (s >= p + len1) {
      const unsigned long poff = (unsigned long)(s - p) + (len2 - len1);
      wmemcpy(p, p + poff, len2);
    } else {
      const unsigned long nleft = (unsigned long)((p + len1) - s);
      wmemmove(p, s, nleft);
      wmemcpy(p + nleft, p + len2, len2 - nleft);
    }
  }
}
