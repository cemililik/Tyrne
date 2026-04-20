#!/usr/bin/env bash
# Run the Umbrix kernel under QEMU virt aarch64.
#
# Usage:
#   tools/run-qemu.sh                                         — debug build
#   tools/run-qemu.sh --release                               — release build
#   tools/run-qemu.sh <path/to/elf>                           — explicit ELF path
#
# See docs/guides/run-under-qemu.md for the full walkthrough and the
# manual invocation used under the hood.

set -euo pipefail

BUILD_PROFILE="debug"
KERNEL=""

for arg in "$@"; do
    case "$arg" in
        --release)
            BUILD_PROFILE="release"
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

exec qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a72 \
    -m 128M \
    -smp 1 \
    -nographic \
    -serial mon:stdio \
    -kernel "$KERNEL"
