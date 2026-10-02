//! Rewind bookkeeping: which steps get a snapshot, and a ring of snapshot slots with metadata.
//! The textures themselves live in `Simulation`; this module is the pure part.

/// Bytes of one `rgba32float` texel.
const BYTES_PER_CELL: u64 = 16;

/// Default memory budget for snapshots.
pub const DEFAULT_BUDGET_BYTES: u64 = 256 << 20;
/// Hard cap on the number of snapshots regardless of budget.
pub const MAX_SNAPSHOTS: usize = 512;
pub const DEFAULT_INTERVAL: u32 = 5;

/// How many snapshots of a `width x height` grid fit in `budget_bytes`, at least 2, at most `max`.
pub fn snapshot_capacity(width: u32, height: u32, budget_bytes: u64, max: usize) -> usize {
    let per = (width as u64 * height as u64 * BYTES_PER_CELL).max(1);
    ((budget_bytes / per) as usize).clamp(2, max.max(2))
}

/// A snapshot is taken on every `interval`-th step (interval 0 counts as 1).
pub fn should_snapshot(step: u32, interval: u32) -> bool {
    step.is_multiple_of(interval.max(1))
}

/// What is needed to resume from a snapshot besides the texture contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub step: u32,
    /// 1D mode: the row the next step writes.
    pub row: u32,
}

/// Fixed-capacity ring of snapshot slots; `push` returns the slot to write into.
#[derive(Debug, Clone)]
pub struct SnapshotRing {
    capacity: usize,
    /// Metadata per slot, in slot order; `None` for slots never written.
    slots: Vec<Option<SnapshotMeta>>,
    /// Slot that `push` writes next.
    next: usize,
    len: usize,
}

impl SnapshotRing {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        SnapshotRing { capacity, slots: vec![None; capacity], next: 0, len: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.slots.iter_mut().for_each(|s| *s = None);
        self.next = 0;
        self.len = 0;
    }

    /// Records `meta` and returns the slot its texture should be copied into.
    pub fn push(&mut self, meta: SnapshotMeta) -> usize {
        let slot = self.next;
        self.slots[slot] = Some(meta);
        self.next = (self.next + 1) % self.capacity;
        self.len = (self.len + 1).min(self.capacity);
        slot
    }

    /// Slot of the oldest snapshot.
    fn oldest_slot(&self) -> usize {
        if self.len < self.capacity { 0 } else { self.next }
    }

    /// `(slot, meta)` pairs from oldest to newest.
    pub fn iter_oldest_first(&self) -> impl Iterator<Item = (usize, SnapshotMeta)> + '_ {
        let start = self.oldest_slot();
        (0..self.len).filter_map(move |i| {
            let slot = (start + i) % self.capacity;
            self.slots[slot].map(|m| (slot, m))
        })
    }

    /// The `index`-th snapshot in oldest-first order.
    pub fn get(&self, index: usize) -> Option<(usize, SnapshotMeta)> {
        if index >= self.len {
            return None;
        }
        let slot = (self.oldest_slot() + index) % self.capacity;
        self.slots[slot].map(|m| (slot, m))
    }

    /// Changes the capacity, keeping the newest snapshots. The caller must move texture
    /// contents accordingly; the returned list maps each kept snapshot's old slot to its new slot.
    pub fn set_capacity(&mut self, capacity: usize) -> Vec<(usize, usize)> {
        let capacity = capacity.max(1);
        let kept: Vec<(usize, SnapshotMeta)> =
            self.iter_oldest_first().collect::<Vec<_>>().into_iter().rev().take(capacity).rev().collect();
        let mut fresh = SnapshotRing::new(capacity);
        let mut moves = Vec::with_capacity(kept.len());
        for (old_slot, meta) in kept {
            let new_slot = fresh.push(meta);
            moves.push((old_slot, new_slot));
        }
        *self = fresh;
        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_fits_the_memory_budget_with_sane_bounds() {
        // 512x512 rgba32float = 4 MiB per snapshot; 256 MiB budget -> 64 snapshots.
        assert_eq!(snapshot_capacity(512, 512, 256 << 20, 512), 64);
        // Tiny grids are capped at the maximum.
        assert_eq!(snapshot_capacity(16, 16, 256 << 20, 512), 512);
        // Huge grids still keep at least two snapshots.
        assert_eq!(snapshot_capacity(8192, 8192, 256 << 20, 512), 2);
    }

    #[test]
    fn snapshots_are_taken_every_interval_steps() {
        assert!(should_snapshot(0, 5));
        assert!(!should_snapshot(3, 5));
        assert!(should_snapshot(10, 5));
        assert!(should_snapshot(7, 1));
        assert!(should_snapshot(0, 0), "interval 0 behaves like 1");
    }

    #[test]
    fn ring_keeps_the_newest_snapshots_in_order() {
        let mut ring = SnapshotRing::new(3);
        assert!(ring.is_empty());
        assert_eq!(ring.push(SnapshotMeta { step: 0, row: 1 }), 0);
        assert_eq!(ring.push(SnapshotMeta { step: 5, row: 1 }), 1);
        assert_eq!(ring.push(SnapshotMeta { step: 10, row: 1 }), 2);
        // Full: the next push overwrites the oldest slot (0).
        assert_eq!(ring.push(SnapshotMeta { step: 15, row: 1 }), 0);
        assert_eq!(ring.len(), 3);
        let steps: Vec<u32> = ring.iter_oldest_first().map(|(_, m)| m.step).collect();
        assert_eq!(steps, vec![5, 10, 15]);
        let slots: Vec<usize> = ring.iter_oldest_first().map(|(slot, _)| slot).collect();
        assert_eq!(slots, vec![1, 2, 0]);
        // Index i in oldest-first order maps to a slot and its metadata.
        assert_eq!(ring.get(0).map(|(s, m)| (s, m.step)), Some((1, 5)));
        assert_eq!(ring.get(2).map(|(s, m)| (s, m.step)), Some((0, 15)));
        assert_eq!(ring.get(3), None);
        ring.clear();
        assert!(ring.is_empty());
    }

    #[test]
    fn ring_capacity_can_shrink_and_grow() {
        let mut ring = SnapshotRing::new(4);
        for s in 0..4 {
            ring.push(SnapshotMeta { step: s * 2, row: 1 });
        }
        ring.set_capacity(2);
        let steps: Vec<u32> = ring.iter_oldest_first().map(|(_, m)| m.step).collect();
        assert_eq!(steps, vec![4, 6], "keeps the newest when shrinking");
        ring.set_capacity(5);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.capacity(), 5);
    }
}
