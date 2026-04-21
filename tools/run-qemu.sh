#!/usr/bin/env bash
# Run the Umbrix kernel under QEMU virt aarch64.
#
# Usage:
#   tools/run-qemu.sh                                         — debug build
#   tools/run-qemu.sh --release                               — release build
#   tools/run-qemu.sh --int-log                               — log exceptions to /tmp/qemu_int.log
#   tools/run-qemu.sh <path/to/elf>                           — explicit ELF path
#
# --int-log adds -d int -D /tmp/qemu_int.log to the QEMU invocation.
# Use it when the kernel hangs silently to see what exception fired.
# After the run: grep "Taking exception" /tmp/qemu_int.log
#
# See docs/guides/run-under-qemu.md for the full walkthrough and the
# manual invocation used under the hood.

set -euo pipefail

BUILD_PROFILE="debug"
KERNEL=""
INT_LOG=""

for arg in "$@"; do
    case "$arg" in
        --release)
            BUILD_PROFILE="release"
            ;;
        --int-log)
            INT_LOG="yes"
            ;;
        *)
            KERNEL="$arg"
            ;;
    esac
done

if [[ -z "$KERNEL" ]]; then
    KERNEL="target/aarch64-unknown-none/${BUILD_PROFILE}/umbrix-bsp-qemu-virt"
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "error: kernel image not found at $KERNEL" >&2
    echo "hint: run 'cargo kernel-build' first (or 'cargo build --release --target aarch64-unknown-none -p umbrix-bsp-qemu-virt' for release)" >&2
    exit 1
fi

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "error: qemu-system-aarch64 not found in PATH" >&2
    echo "hint (macOS): brew install qemu" >&2
    echo "hint (Debian/Ubuntu): sudo apt install qemu-system-arm" >&2
    exit 1
fi

INT_LOG_FLAGS=()
if [[ -n "$INT_LOG" ]]; then
    INT_LOG_FLAGS=(-d int -D /tmp/qemu_int.log)
    echo "exception log → /tmp/qemu_int.log  (grep 'Taking exception' to inspect)" >&2
fi

exec qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a72 \
    -m 128M \
    -smp 1 \
    -nographic \
    -serial mon:stdio \
    "${INT_LOG_FLAGS[@]}" \
    -kernel "$KERNEL"
