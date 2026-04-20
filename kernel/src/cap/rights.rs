//! Rights bitfield for [`super::Capability`].
//!
//! Hand-rolled rather than using the `bitflags` crate to keep the kernel
//! dependency-free for now; see the open question in [ADR-0014][adr-0014].
//!
//! [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md

use core::ops::{BitAnd, BitOr, BitOrAssign};

/// A set of rights a capability confers.
///
/// Rights are **narrowing-only**: [`super::CapabilityTable::cap_copy`] and
/// [`super::CapabilityTable::cap_derive`] may drop rights but never add
/// them. Attempting to widen returns [`super::CapError::WidenedRights`].
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CapRights(u32);

impl CapRights {
    /// No rights.
    pub const EMPTY: Self = Self(0);
    /// The bearer may invoke `cap_copy` on this capability to produce a peer.
    pub const DUPLICATE: Self = Self(1 << 0);
    /// The bearer may invoke `cap_derive` on this capability to produce a child.
    pub const DERIVE: Self = Self(1 << 1);
    /// The bearer may invoke `cap_revoke` on this capability to destroy its subtree.
    pub const REVOKE: Self = Self(1 << 2);
    /// The bearer may include this capability in an IPC message (placeholder for A4).
    pub const TRANSFER: Self = Self(1 << 3);

    /// Union of every bit defined by this version of the rights bitfield.
    ///
    /// Any bit outside this mask is reserved for future ADRs and is not a
    /// valid rights flag today. ABI-boundary code constructs `CapRights`
    /// through [`from_raw`][Self::from_raw], which silently masks reserved
    /// bits away so an untrusted caller cannot smuggle unknown rights past
    /// `contains` / subset checks.
    pub const KNOWN_BITS: Self =
        Self(Self::DUPLICATE.0 | Self::DERIVE.0 | Self::REVOKE.0 | Self::TRANSFER.0);

    /// Construct an empty flag set.
    #[must_use]
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// Construct a flag set from raw bits, masking away bits outside
    /// [`KNOWN_BITS`][Self::KNOWN_BITS].
    ///
    /// Callers should prefer combining the named constants; `from_raw`
    /// exists so higher layers can pass bits across ABI boundaries. Bits
    /// outside `KNOWN_BITS` are reserved for future ADRs and are silently
    /// dropped — a hostile or buggy caller cannot use them to weaken
    /// [`contains`][Self::contains] or subset checks.
    #[must_use]
    pub const fn from_raw(bits: u32) -> Self {
        Self(bits & Self::KNOWN_BITS.0)
    }

    /// Return the raw bit pattern.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Return `true` if every flag in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Return the bitwise union of two rights sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Return the bitwise intersection of two rights sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Return `self` with every flag in `other` cleared.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Return `true` if no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for CapRights {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitAnd for CapRights {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

impl BitOrAssign for CapRights {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests are allowed pragmas forbidden in production kernel code"
)]
mod tests {
    use super::CapRights;

    #[test]
    fn empty_contains_empty_but_nothing_else() {
        assert!(CapRights::EMPTY.contains(CapRights::EMPTY));
        assert!(!CapRights::EMPTY.contains(CapRights::DUPLICATE));
        assert!(CapRights::EMPTY.is_empty());
    }

    #[test]
    fn union_and_contains() {
        let rw = CapRights::DUPLICATE | CapRights::DERIVE;
        assert!(rw.contains(CapRights::DUPLICATE));
        assert!(rw.contains(CapRights::DERIVE));
        assert!(!rw.contains(CapRights::REVOKE));
        assert!(rw.contains(CapRights::EMPTY));
    }

    #[test]
    fn intersection_narrows() {
        let all = CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE;
        let just_dup = all & CapRights::DUPLICATE;
        assert!(just_dup.contains(CapRights::DUPLICATE));
        assert!(!just_dup.contains(CapRights::DERIVE));
        assert!(!just_dup.contains(CapRights::REVOKE));
    }

    #[test]
    fn difference_clears_bits() {
        let rwx = CapRights::DUPLICATE | CapRights::DERIVE;
        let just_derive = rwx.difference(CapRights::DUPLICATE);
        assert!(just_derive.contains(CapRights::DERIVE));
        assert!(!just_derive.contains(CapRights::DUPLICATE));
    }

    #[test]
    fn from_raw_and_raw_round_trip() {
        let bits = 0b1011;
        let rights = CapRights::from_raw(bits);
        assert_eq!(rights.raw(), bits);
    }

    #[test]
    fn from_raw_masks_unknown_bits() {
        // Bit 31 is reserved; from_raw must drop it silently so no
        // reserved bit survives as part of the resulting CapRights.
        let rights = CapRights::from_raw(0x8000_0000 | CapRights::DUPLICATE.raw());
        assert!(rights.contains(CapRights::DUPLICATE));
        assert_eq!(rights.raw() & !CapRights::KNOWN_BITS.raw(), 0);
        // A value built purely from reserved bits collapses to EMPTY.
        assert!(CapRights::from_raw(0x8000_0000).is_empty());
    }

    #[test]
    fn bitor_assign_adds_bits() {
        let mut rights = CapRights::DUPLICATE;
        rights |= CapRights::REVOKE;
        assert!(rights.contains(CapRights::DUPLICATE));
        assert!(rights.contains(CapRights::REVOKE));
    }
}
