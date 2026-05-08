//! Deterministic fake [`tyrne_hal::Mmu`] for host-side tests.

use std::collections::HashMap;
use std::sync::Mutex;
use tyrne_hal::{FrameProvider, MapperFlush, MappingFlags, Mmu, MmuError, PhysFrame, VirtAddr};

/// A simple [`FrameProvider`] backed by a `Vec` of pre-allocated frames.
///
/// Pops from the end, so the order in which frames are consumed is the
/// reverse of insertion order. Tests can query [`Self::remaining`] to
/// check how many frames were used.
pub struct VecFrameProvider {
    available: Vec<PhysFrame>,
}

impl VecFrameProvider {
    /// Construct a `VecFrameProvider` from the given frames.
    #[must_use]
    pub fn new(frames: Vec<PhysFrame>) -> Self {
        Self { available: frames }
    }

    /// Return the number of frames remaining.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.available.len()
    }
}

impl FrameProvider for VecFrameProvider {
    fn alloc_frame(&mut self) -> Option<PhysFrame> {
        self.available.pop()
    }
}

/// Address-space representation used by [`FakeMmu`].
///
/// Stores mappings as a `HashMap` keyed by virtual address. The fake has
/// no intermediate page tables; its purpose is to validate the behaviour
/// of kernel code against the [`Mmu`] contract, not to model `VMSAv8`.
pub struct FakeAddressSpace {
    root: PhysFrame,
    mappings: HashMap<VirtAddr, (PhysFrame, MappingFlags)>,
}

impl FakeAddressSpace {
    /// Return the number of live mappings in this address space.
    #[must_use]
    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }

    /// Look up the mapping for a virtual address, if any.
    #[must_use]
    pub fn lookup(&self, va: VirtAddr) -> Option<(PhysFrame, MappingFlags)> {
        self.mappings.get(&va).copied()
    }
}

/// A [`Mmu`] that records activations, TLB invalidations, and mapping
/// operations for test assertions.
pub struct FakeMmu {
    state: Mutex<FakeMmuState>,
}

struct FakeMmuState {
    activated_root: Option<PhysFrame>,
    tlb_address_invalidations: Vec<VirtAddr>,
    tlb_all_count: u64,
}

impl FakeMmu {
    /// Construct a new `FakeMmu` with no address space activated.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeMmuState {
                activated_root: None,
                tlb_address_invalidations: Vec::new(),
                tlb_all_count: 0,
            }),
        }
    }

    /// Return the root frame of the currently activated address space, if
    /// any.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn activated_root(&self) -> Option<PhysFrame> {
        self.locked().activated_root
    }

    /// Return a copy of the list of per-address TLB invalidations seen so
    /// far, in the order they were issued.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn tlb_address_invalidations(&self) -> Vec<VirtAddr> {
        self.locked().tlb_address_invalidations.clone()
    }

    /// Return the number of full-TLB invalidations issued.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn tlb_all_count(&self) -> u64 {
        self.locked().tlb_all_count
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, FakeMmuState> {
        self.state.lock().expect("FakeMmu mutex poisoned")
    }
}

impl Default for FakeMmu {
    fn default() -> Self {
        Self::new()
    }
}

impl Mmu for FakeMmu {
    type AddressSpace = FakeAddressSpace;

    unsafe fn create_address_space(&self, root: PhysFrame) -> FakeAddressSpace {
        FakeAddressSpace {
            root,
            mappings: HashMap::new(),
        }
    }

    fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame {
        as_.root
    }

    fn activate(&self, as_: &Self::AddressSpace) {
        self.locked().activated_root = Some(as_.root);
    }

    fn map(
        &self,
        as_: &mut FakeAddressSpace,
        va: VirtAddr,
        pa: PhysFrame,
        flags: MappingFlags,
        _frames: &mut dyn FrameProvider,
    ) -> Result<MapperFlush, MmuError> {
        if as_.mappings.contains_key(&va) {
            return Err(MmuError::AlreadyMapped);
        }
        as_.mappings.insert(va, (pa, flags));
        Ok(MapperFlush::new(va))
    }

    fn unmap(
        &self,
        as_: &mut FakeAddressSpace,
        va: VirtAddr,
    ) -> Result<(MapperFlush, PhysFrame), MmuError> {
        as_.mappings
            .remove(&va)
            .map(|(pa, _)| (MapperFlush::new(va), pa))
            .ok_or(MmuError::NotMapped)
    }

    fn invalidate_tlb_address(&self, va: VirtAddr) {
        self.locked().tlb_address_invalidations.push(va);
    }

    fn invalidate_tlb_all(&self) {
        self.locked().tlb_all_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeMmu, VecFrameProvider};
    use tyrne_hal::{MapperFlush, MappingFlags, Mmu, MmuError, PhysAddr, PhysFrame, VirtAddr};

    fn frame(addr: usize) -> PhysFrame {
        PhysFrame::from_aligned(PhysAddr(addr)).expect("test addr must be page-aligned")
    }

    #[test]
    fn mapping_flags_union_and_contains() {
        let rw = MappingFlags::WRITE;
        let rwx = rw | MappingFlags::EXECUTE;
        assert!(rwx.contains(MappingFlags::WRITE));
        assert!(rwx.contains(MappingFlags::EXECUTE));
        assert!(!rwx.contains(MappingFlags::USER));
    }

    #[test]
    fn mapping_flags_difference_clears_bits() {
        let rwx = MappingFlags::WRITE | MappingFlags::EXECUTE;
        let rw = rwx.difference(MappingFlags::EXECUTE);
        assert!(rw.contains(MappingFlags::WRITE));
        assert!(!rw.contains(MappingFlags::EXECUTE));
    }

    #[test]
    fn phys_frame_rejects_unaligned() {
        assert!(PhysFrame::from_aligned(PhysAddr(0x1001)).is_none());
        assert!(PhysFrame::from_aligned(PhysAddr(0x1000)).is_some());
    }

    #[test]
    fn create_address_space_stores_root() {
        let mmu = FakeMmu::new();
        let root = frame(0x1000);
        // SAFETY: FakeMmu::create_address_space does not dereference `root`;
        // it only stores the PhysFrame value. Alignment is upheld because
        // `frame()` (and PhysFrame::from_aligned) reject unaligned addresses.
        let as_ = unsafe { mmu.create_address_space(root) };
        assert_eq!(mmu.address_space_root(&as_), root);
        assert_eq!(as_.mapping_count(), 0);
    }

    #[test]
    fn activate_records_root() {
        let mmu = FakeMmu::new();
        let root = frame(0x1000);
        // SAFETY: FakeMmu::create_address_space does not dereference `root`;
        // it only stores the PhysFrame value. Alignment is upheld because
        // `frame()` (and PhysFrame::from_aligned) reject unaligned addresses.
        let as_ = unsafe { mmu.create_address_space(root) };
        assert!(mmu.activated_root().is_none());
        mmu.activate(&as_);
        assert_eq!(mmu.activated_root(), Some(root));
    }

    #[test]
    fn map_unmap_round_trip() {
        let mmu = FakeMmu::new();
        // SAFETY: FakeMmu::create_address_space does not dereference its
        // argument; `frame(0x1000)` is page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let mut fp = VecFrameProvider::new(vec![frame(0x2000)]);

        let flush = mmu
            .map(
                &mut as_,
                VirtAddr(0x4000),
                frame(0x8000),
                MappingFlags::WRITE,
                &mut fp,
            )
            .expect("first map must succeed");
        flush.flush(&mmu);
        assert_eq!(as_.mapping_count(), 1);

        let (pa, flags) = as_
            .lookup(VirtAddr(0x4000))
            .expect("lookup must find mapping");
        assert_eq!(pa, frame(0x8000));
        assert!(flags.contains(MappingFlags::WRITE));

        let (unmap_flush, returned) = mmu
            .unmap(&mut as_, VirtAddr(0x4000))
            .expect("unmap must succeed");
        unmap_flush.flush(&mmu);
        assert_eq!(returned, frame(0x8000));
        assert_eq!(as_.mapping_count(), 0);
    }

    #[test]
    fn double_map_returns_already_mapped() {
        let mmu = FakeMmu::new();
        // SAFETY: FakeMmu::create_address_space does not dereference its
        // argument; `frame(0x1000)` is page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let mut fp = VecFrameProvider::new(vec![]);

        mmu.map(
            &mut as_,
            VirtAddr(0x4000),
            frame(0x8000),
            MappingFlags::WRITE,
            &mut fp,
        )
        .expect("first map must succeed")
        .flush(&mmu);

        let err = mmu
            .map(
                &mut as_,
                VirtAddr(0x4000),
                frame(0x9000),
                MappingFlags::WRITE,
                &mut fp,
            )
            .expect_err("second map must fail");
        assert_eq!(err, MmuError::AlreadyMapped);
    }

    #[test]
    fn unmap_missing_returns_not_mapped() {
        let mmu = FakeMmu::new();
        // SAFETY: FakeMmu::create_address_space does not dereference its
        // argument; `frame(0x1000)` is page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let err = mmu
            .unmap(&mut as_, VirtAddr(0x4000))
            .expect_err("unmap of unmapped va must fail");
        assert_eq!(err, MmuError::NotMapped);
    }

    #[test]
    fn tlb_invalidations_recorded_in_order() {
        let mmu = FakeMmu::new();
        mmu.invalidate_tlb_address(VirtAddr(0x4000));
        mmu.invalidate_tlb_address(VirtAddr(0x5000));
        mmu.invalidate_tlb_all();
        assert_eq!(
            mmu.tlb_address_invalidations(),
            vec![VirtAddr(0x4000), VirtAddr(0x5000)]
        );
        assert_eq!(mmu.tlb_all_count(), 1);
    }

    // ── MapperFlush token semantics ───────────────────────────────────────────

    #[test]
    fn mapper_flush_carries_virt_addr() {
        let token = MapperFlush::new(VirtAddr(0x4000));
        assert_eq!(token.virt_addr(), VirtAddr(0x4000));
    }

    #[test]
    fn mapper_flush_flush_invokes_invalidate_tlb_address() {
        let mmu = FakeMmu::new();
        let token = MapperFlush::new(VirtAddr(0x12_3000));
        token.flush(&mmu);
        assert_eq!(
            mmu.tlb_address_invalidations(),
            vec![VirtAddr(0x12_3000)],
            "flush() must invoke invalidate_tlb_address for the held VA"
        );
        assert_eq!(
            mmu.tlb_all_count(),
            0,
            "flush() must not invoke invalidate_tlb_all"
        );
    }

    #[test]
    fn mapper_flush_ignore_is_documented_noop() {
        let mmu = FakeMmu::new();
        let token = MapperFlush::new(VirtAddr(0x4000));
        token.ignore();
        assert!(
            mmu.tlb_address_invalidations().is_empty(),
            "ignore() must not invoke invalidate_tlb_address"
        );
        assert_eq!(
            mmu.tlb_all_count(),
            0,
            "ignore() must not invoke invalidate_tlb_all"
        );
    }

    #[test]
    fn map_returns_token_with_mapped_va() {
        let mmu = FakeMmu::new();
        // SAFETY: FakeMmu::create_address_space does not dereference its
        // argument; `frame(0x1000)` is page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let mut fp = VecFrameProvider::new(vec![]);

        let flush = mmu
            .map(
                &mut as_,
                VirtAddr(0x4_0000),
                frame(0x8000),
                MappingFlags::WRITE,
                &mut fp,
            )
            .expect("map must succeed");
        assert_eq!(
            flush.virt_addr(),
            VirtAddr(0x4_0000),
            "map's MapperFlush must carry the VA passed to map"
        );
        flush.flush(&mmu);
        assert_eq!(mmu.tlb_address_invalidations(), vec![VirtAddr(0x4_0000)]);
    }

    #[test]
    fn unmap_returns_token_with_unmapped_va_and_frame() {
        let mmu = FakeMmu::new();
        // SAFETY: FakeMmu::create_address_space does not dereference its
        // argument; `frame(0x1000)` is page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let mut fp = VecFrameProvider::new(vec![]);

        mmu.map(
            &mut as_,
            VirtAddr(0x5_0000),
            frame(0x9000),
            MappingFlags::WRITE,
            &mut fp,
        )
        .expect("map must succeed")
        .ignore();

        let (flush, returned) = mmu
            .unmap(&mut as_, VirtAddr(0x5_0000))
            .expect("unmap must succeed");
        assert_eq!(returned, frame(0x9000), "unmap must return the mapped PA");
        assert_eq!(
            flush.virt_addr(),
            VirtAddr(0x5_0000),
            "unmap's MapperFlush must carry the VA passed to unmap"
        );
        flush.flush(&mmu);
        assert_eq!(mmu.tlb_address_invalidations(), vec![VirtAddr(0x5_0000)]);
    }

    #[test]
    fn bulk_map_with_ignore_then_invalidate_tlb_all() {
        let mmu = FakeMmu::new();
        // SAFETY: page-aligned by construction.
        let mut as_ = unsafe { mmu.create_address_space(frame(0x1000)) };
        let mut fp = VecFrameProvider::new(vec![]);

        for (i, &va) in [
            VirtAddr(0x10_0000),
            VirtAddr(0x11_0000),
            VirtAddr(0x12_0000),
        ]
        .iter()
        .enumerate()
        {
            mmu.map(
                &mut as_,
                va,
                frame(0x100_0000 + i * 0x1000),
                MappingFlags::WRITE,
                &mut fp,
            )
            .expect("map must succeed")
            .ignore();
        }
        mmu.invalidate_tlb_all();

        assert!(
            mmu.tlb_address_invalidations().is_empty(),
            "bulk-with-ignore must not issue per-address invalidates"
        );
        assert_eq!(mmu.tlb_all_count(), 1);
    }
}
