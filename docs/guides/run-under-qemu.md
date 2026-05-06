# Run Tyrne under QEMU

Build the Phase 4c kernel image and boot it under `qemu-system-aarch64` on the `virt` machine. The kernel writes a greeting to the serial console and halts — this is the first real-world run of Tyrne code.

## Goal

- A running `qemu-system-aarch64 -M virt` instance that has booted Tyrne and printed `tyrne: hello from kernel_main` to its serial console.
- Clean exit from QEMU back to the shell.

## Prerequisites

- macOS, Linux, or another Unix-like host.
- **Rustup** installed, with this repo's `rust-toolchain.toml` pinned nightly available. The first `cargo` invocation in the repo will install it automatically if `rustup` can reach the network.
- **QEMU** with aarch64 system support. On macOS:

  ```sh
  brew install qemu
  ```

  On Debian / Ubuntu:

  ```sh
  sudo apt install qemu-system-arm
  ```

  Verify:

  ```sh
  qemu-system-aarch64 --version
  ```

## Steps

1. **Clone the repository** if you have not already.

   ```sh
   git clone https://github.com/cemililik/Tyrne.git
   cd TyrneOS
   ```

2. **Let rustup install the pinned toolchain.** Any `cargo` command in the repo triggers it; `cargo --version` is the smallest such command.

   ```sh
   cargo --version
   ```

   The first run downloads the pinned nightly and the `aarch64-unknown-none` target; later runs are instant.

3. **Build the kernel image.** The workspace alias builds the BSP binary for the bare-metal target.

   ```sh
   cargo kernel-build
   ```

   The ELF is written to `target/aarch64-unknown-none/debug/tyrne-bsp-qemu-virt`.

4. **Run under QEMU**, using the project's shell helper.

   ```sh
   ./tools/run-qemu.sh
   ```

   Or, equivalently, via the Cargo runner:

   ```sh
   cargo run --target aarch64-unknown-none -p tyrne-bsp-qemu-virt
   ```

   Or fully manually:

   ```sh
   qemu-system-aarch64 \
       -M virt \
       -cpu cortex-a72 \
       -m 128M \
       -smp 1 \
       -nographic \
       -serial mon:stdio \
       -kernel target/aarch64-unknown-none/debug/tyrne-bsp-qemu-virt
   ```

## Verifying it worked

The terminal should show:

```text
tyrne: hello from kernel_main
```

The kernel then enters a `spin_loop` idle — there is nothing else to print yet. QEMU does not exit on its own.

## Exiting QEMU

With `-nographic` and `-serial mon:stdio`, your terminal is multiplexed between the guest serial console and the QEMU monitor. To exit:

- Press `Ctrl-A`, then `x` (QEMU exits immediately).

Alternatively, press `Ctrl-A`, then `c` to switch between the serial view and the monitor prompt (`(qemu)`), and type `quit`.

## Troubleshooting

**`cargo kernel-build` fails with "linker not found" or similar.** The `aarch64-unknown-none` target needs `rust-lld`, which is part of the toolchain's `llvm-tools-preview` component. Check:

```sh
rustup component list --installed | grep llvm-tools
```

The repo's `rust-toolchain.toml` installs it automatically; if missing, remove `~/.rustup/toolchains/<name>` and rerun `cargo --version`.

**Build fails with "undefined reference to memcpy" or similar.** The prebuilt `aarch64-unknown-none` runtime normally provides these. If missing, consider `-Z build-std-features=compiler-builtins-mem` as a workaround; file an issue with the exact error.

**QEMU says "No machine virt".** Install a newer `qemu-system-aarch64`; the `virt` machine has been available for years.

**Nothing is printed.** Two common causes:
- The PL011 MMIO base is wrong for your QEMU version. It should be `0x0900_0000` on `virt`. Verify via `info mtree` in the QEMU monitor.
- The kernel is panicking before reaching `write_bytes`. Add early prints to `_start` (assembly `mov`-and-`strb` into `0x0900_0000`) to narrow it down.

**Build succeeds but QEMU exits immediately or hangs silently.** Check the ELF's entry point:

```sh
llvm-objdump -f target/aarch64-unknown-none/debug/tyrne-bsp-qemu-virt | head
```

It should be `0x40080000`.

## What you just ran

This is the whole system, end to end, in Phase 4c v0.0.1:

- **QEMU** loads the ELF and jumps to `_start`.
- **`_start`** (in [`bsp-qemu-virt/src/boot.s`](../../bsp-qemu-virt/src/boot.s)) sets up the stack, zeroes BSS, branches into Rust.
- **`kernel_entry`** (in [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs)) constructs the `Pl011Uart` and hands it to the portable kernel.
- **`tyrne_kernel::run`** (in [`kernel/src/lib.rs`](../../kernel/src/lib.rs)) writes the greeting and halts.

See [`docs/architecture/boot.md`](../architecture/boot.md) for the full design, and [ADR-0012](../decisions/0012-boot-flow-qemu-virt.md) for the rationale.

## References

- [ADR-0012: Boot flow and memory layout for `bsp-qemu-virt`](../decisions/0012-boot-flow-qemu-virt.md).
- [`docs/architecture/boot.md`](../architecture/boot.md).
- [`docs/architecture/hal.md`](../architecture/hal.md).
- QEMU `virt` machine: https://qemu.readthedocs.io/en/latest/system/arm/virt.html
