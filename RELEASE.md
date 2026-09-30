# Clipboard Desktop v1.7.3

> 界面语言自动跟随系统、Linux glibc 兼容性修复
>
> Released: 2026-09-30

---

## 新功能

### 国际化

- **界面语言自动跟随系统** — 新增「自动」语言选项并设为默认：中文系统环境显示中文，其他环境默认英文，无需手动切换；设置页语言项改为下拉框（自动 / 中文 / English），已手动选择语言的用户不受影响 | [`2dfd2e6`](https://github.com/muutot/Clipboard/commit/2dfd2e66) [`b7d9d87`](https://github.com/muutot/Clipboard/commit/b7d9d875)

---

## 构建修复

### Linux 发布

- **glibc/libstdc++ 兼容垫片** — 预编译 ONNX Runtime（pyke dfbin）引用了 glibc 2.38+ / GCC 13 的新符号（`__isoc23_*`、`_M_replace_cold`），导致在 ubuntu-22.04（glibc 2.35）上链接失败；新增仅 Linux 生效的符号垫片，并补齐 `std::wstring`（wchar_t）实例化缺失的符号，发布产物兼容 glibc 2.35+（Ubuntu 22.04/24.04、Debian 12+ 等）| [`42dabe0`](https://github.com/muutot/Clipboard/commit/42dabe07) [`2226d26`](https://github.com/muutot/Clipboard/commit/2226d26e) [`b546dbb`](https://github.com/muutot/Clipboard/commit/b546dbb8)

---

## 构建产物

- **Windows**: `Clipboard_1.7.3_x64_en-US.msi` / `Clipboard_1.7.3_x64-setup.exe`
- **macOS (Apple Silicon)**: `Clipboard_1.7.3_aarch64.app.tar.gz` / `.dmg`
- **Linux (x64)**: `Clipboard_1.7.3_amd64.AppImage` / `.deb` / `.rpm`
