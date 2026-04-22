//! Cooperative context-switch extension for BSPs.
//!
//! See [ADR-0020] for the design rationale. This trait is deliberately
//! separate from [`Cpu`][crate::Cpu] to preserve `Cpu`'s object-safety;
//! the scheduler is generic over `C: ContextSwitch` and does not use
//! dynamic dispatch.
//!
//! [ADR-0020]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0020-cpu-trait-v2-context-switch.md

/// Context-switch extension for BSPs that support cooperative task switching.
///
/// Separate from [`Cpu`][crate::Cpu] to preserve `Cpu`'s object-safety.
/// The scheduler is generic over `C: ContextSwitch`; it never needs
/// dynamic dispatch.
///
/// # Safety contract
///
/// Implementations must ensure that `context_switch` atomically saves
/// all callee-saved registers of the current execution context and
/// restores all callee-saved registers of the next context. On aarch64
/// that is `x19`–`x28`, `x29` (fp), `x30` (lr), and `sp`. From the
/// perspective of both call sites, `context_switch` appears to return
/// normally — the saving side resumes here when it is later selected as
/// `next`.
pub trait ContextSwitch {
    /// The saved register state for one cooperative task.
    ///
    /// Must be `Default` so the scheduler can zero-initialise a slot
    /// before `init_context` fills it in. Must be `Send` so contexts
    /// can be moved between (future) CPU cores.
    type TaskContext: Default + Send;

    /// Save the calling task's register state into `current` and resume
    /// the task whose state was saved in `next`.
    ///
    /// When this task is later resumed (by another call to
    /// `context_switch` with this `current` as the `next` argument),
    /// execution continues as if `context_switch` returned normally.
    ///
    /// # Safety
    ///
    /// - Interrupts must be disabled before this call. An IRQ firing
    ///   mid-switch would observe a partially saved state.
    /// - `current` must be valid for the entire time this task is
    ///   suspended; the caller is responsible for keeping the context
    ///   array alive.
    /// - `next` must contain a context previously written by
    ///   `context_switch` or fully initialised by `init_context`.
    ///   Restoring an uninitialised context is undefined behaviour.
    unsafe fn context_switch(&self, current: &mut Self::TaskContext, next: &Self::TaskContext);

    /// Write an initial register state into `ctx` so that the first
    /// restore begins executing `entry` with `stack_top` as the initial
    /// stack pointer.
    ///
    /// # Safety
    ///
    /// - `stack_top` must point one byte past the top of a
    ///   sufficiently-sized (≥ 512 bytes recommended for aarch64),
    ///   16-byte-aligned stack region that remains valid for the
    ///   task's entire lifetime.
    /// - `entry` must be a `fn() -> !` that never returns; returning
    ///   from a task entry function is undefined behaviour.
    unsafe fn init_context(
        &self,
        ctx: &mut Self::TaskContext,
        entry: fn() -> !,
        stack_top: *mut u8,
    );
}
