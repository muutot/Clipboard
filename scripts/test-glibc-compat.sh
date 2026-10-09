#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
gcc -std=c11 -Wall -Wextra -Werror -c "$repo/src-tauri/glibc_compat.c" -o "$scratch/compat.o"
g++ -std=c++17 -Wall -Wextra -Werror "$repo/src-tauri/tests/glibc_compat_abi.cpp" "$scratch/compat.o" -o "$scratch/abi"
"$scratch/abi"
