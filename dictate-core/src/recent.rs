use std::collections::VecDeque;

use crate::model::Utterance;

/// The last few delivered utterances, newest first, held in memory only
/// (FR-17.2). Nothing here reaches disk and nothing survives the process.
///
/// Lives in core rather than beside the tray menu so the trimming rule is
/// covered by the host tier. The Windows side owns only the rendering.
#[derive(Debug, Clone)]
pub struct RecentUtterances {
    items: VecDeque<Utterance>,
    capacity: usize,
}

impl RecentUtterances {
    /// # Panics
    /// If `capacity` is zero — a programming error, not a runtime condition.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 1, "Capacity must be at least 1.");
        Self { items: VecDeque::with_capacity(capacity + 1), capacity }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Newest first. A snapshot: the tray menu walks it while rebuilding
    /// itself, and a live view is what crashed the C# version.
    pub fn items(&self) -> Vec<Utterance> {
        self.items.iter().cloned().collect()
    }

    /// Adds an utterance, dropping the oldest once capacity is exceeded.
    pub fn add(&mut self, utterance: Utterance) {
        self.items.push_front(utterance);
        self.items.truncate(self.capacity);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}
