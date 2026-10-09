// Call the exported symbols through real non-static C++ members, so the
// compiler (not a hand-written C call) supplies the hidden this argument.
#include <algorithm>
#include <array>
#include <cassert>
#include <string>

struct CharMember {
  unsigned long untouched = 0x12345678;
  void replace(char *, unsigned long, const char *, unsigned long, unsigned long)
      asm("_ZNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEE15_M_replace_coldEPcmPKcmm");
};
struct WideMember {
  unsigned long untouched = 0x12345678;
  void replace(wchar_t *, unsigned long, const wchar_t *, unsigned long, unsigned long)
      asm("_ZNSt7__cxx1112basic_stringIwSt11char_traitsIwESaIwEE15_M_replace_coldEPwmPKwmm");
};

template <class Char, class Member> void check(const std::basic_string<Char> &base) {
  for (unsigned long pos = 0; pos <= base.size(); ++pos)
    for (unsigned long removed = 0; removed <= base.size() - pos; ++removed)
      for (unsigned long source = 0; source <= base.size(); ++source)
        for (unsigned long copied = 0; copied <= base.size() - source; ++copied) {
          std::array<Char, 64> buffer{};
          std::copy(base.begin(), base.end(), buffer.begin());
          auto expected = base.substr(0, pos) + base.substr(source, copied) +
                          base.substr(pos + removed);
          Member member;
          member.replace(buffer.data() + pos, removed, buffer.data() + source,
                         copied, base.size() - pos - removed);
          assert(member.untouched == 0x12345678);
          assert(std::equal(expected.begin(), expected.end(), buffer.begin()));
        }
}

int main() {
  check<char, CharMember>("0123456789abcdef");
  check<wchar_t, WideMember>(L"0123456789abcdef");
}
