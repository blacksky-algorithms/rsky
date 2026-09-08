use std::fmt;

#[derive(Debug, thiserror::Error)]
#[error("Slot decode: {0}")]
pub struct SlotDecodeError(pub String);

/// some *small* per-repo state owned by the consumer app
///
/// gets colocated with hubble-sync's own data and cached on the repo actor,
/// reducing read amplification.
pub trait RepoSlot: fmt::Debug + Clone + Sized + Send + Sync + 'static {
    const PRESENT: bool = true;

    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, SlotDecodeError>;
}

/// unit-type slot: special non-present state
///
/// zero memory overhead, zero storage overhead
impl RepoSlot for () {
    const PRESENT: bool = false;
    fn encode(&self) -> Vec<u8> {
        unreachable!("not present")
    }
    fn decode(_: &[u8]) -> Result<Self, SlotDecodeError> {
        unreachable!("not present")
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Slot {
    /// per-repo state slot that updates frequently (per-commit, etc.)
    Commit,
    /// per-repo slot that updates slower (per-resync, identity change, etc.)
    Info,
}

/// mutable handle to one cached slot
pub struct SlotMut<'a, T: RepoSlot> {
    value: &'a mut Option<T>,
    dirty: bool,
}

impl<'a, T: RepoSlot> SlotMut<'a, T> {
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn set(&mut self, v: T) {
        *self.value = Some(v);
        self.dirty = true;
    }
    pub fn clear(&mut self) {
        *self.value = None;
        self.dirty = true;
    }
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }
}

pub struct RepoSlots<'a, C: RepoSlot, I: RepoSlot> {
    pub commit: SlotMut<'a, C>,
    pub info: SlotMut<'a, I>,
}

impl<'a, C: RepoSlot, I: RepoSlot> RepoSlots<'a, C, I> {
    pub(crate) fn new(commit: &'a mut Option<C>, info: &'a mut Option<I>) -> Self {
        Self {
            commit: SlotMut {
                value: commit,
                dirty: false,
            },
            info: SlotMut {
                value: info,
                dirty: false,
            },
        }
    }
    pub(crate) fn new_from(
        commit: &'a mut Option<C>,
        info: &'a mut Option<I>,
        (commit_dirty, info_dirty): (bool, bool),
    ) -> Self {
        Self {
            commit: SlotMut {
                value: commit,
                dirty: commit_dirty,
            },
            info: SlotMut {
                value: info,
                dirty: info_dirty,
            },
        }
    }
    pub(crate) fn dirtiness(&self) -> (bool, bool) {
        (self.commit.dirty, self.info.dirty)
    }
}
