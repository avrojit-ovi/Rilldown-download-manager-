//! Segmented transfer planning and the dynamic split board.
//!
//! A download starts with `N` byte ranges. When a worker finishes, it asks the
//! board to split the largest remaining range in half and takes the new half.
//! Splits are only created when there is at least `MIN_SEGMENT_BYTES` of work
//! on each side, so the transfer never degenerates into tiny requests.

use std::sync::Mutex;

use tokio::sync::Notify;

use crate::branding::MIN_SEGMENT_BYTES;
use crate::types::{Segment, SegmentState};

/// Split a known-length resource into contiguous, inclusive byte ranges.
#[must_use]
pub fn plan_segments(total: u64, connections: u8) -> Vec<Segment> {
    if total == 0 {
        return Vec::new();
    }
    let by_minimum = (total / MIN_SEGMENT_BYTES).max(1);
    let count = u64::from(connections.max(1)).min(by_minimum).max(1);
    let base = total / count;
    let remainder = total % count;
    let mut segments = Vec::with_capacity(usize::try_from(count).unwrap_or(1));
    let mut start = 0_u64;
    for index in 0..count {
        let length = base + u64::from(index < remainder);
        let end = start + length - 1;
        segments.push(Segment {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            start_byte: start,
            end_byte: end,
            downloaded: 0,
            state: SegmentState::Pending,
        });
        start = end + 1;
    }
    segments
}

#[derive(Debug)]
struct BoardState {
    segments: Vec<Segment>,
    next_index: u32,
}

/// Shared, thread-safe view of the active byte ranges.
#[derive(Debug)]
pub struct SegmentBoard {
    state: Mutex<BoardState>,
    notify: Notify,
    total_bytes: u64,
}

impl SegmentBoard {
    /// Build a board. Active segments from a previous run become pending.
    #[must_use]
    pub fn new(mut segments: Vec<Segment>, total_bytes: u64) -> Self {
        let next_index = segments
            .iter()
            .map(|segment| segment.index)
            .max()
            .map_or(0, |index| index.saturating_add(1));
        for segment in &mut segments {
            if segment.state == SegmentState::Active {
                segment.state = SegmentState::Pending;
            }
        }
        Self {
            state: Mutex::new(BoardState {
                segments,
                next_index,
            }),
            notify: Notify::new(),
            total_bytes,
        }
    }

    /// Total resource size.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Current segment snapshot, ordered by index.
    #[must_use]
    pub fn snapshot(&self) -> Vec<Segment> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut segments = state.segments.clone();
        segments.sort_by_key(|segment| segment.index);
        segments
    }

    /// Fetch one segment.
    #[must_use]
    pub fn segment(&self, index: u32) -> Option<Segment> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .segments
            .iter()
            .find(|segment| segment.index == index)
            .copied()
    }

    /// The current inclusive end for a segment. This may shrink after a split.
    #[must_use]
    pub fn end_for(&self, index: u32) -> Option<u64> {
        self.segment(index).map(|segment| segment.end_byte)
    }

    /// Mark a segment as actively transferring.
    pub fn mark_active(&self, index: u32) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(segment) = state
            .segments
            .iter_mut()
            .find(|segment| segment.index == index)
        {
            if segment.state != SegmentState::Complete {
                segment.state = SegmentState::Active;
            }
        }
    }

    /// Record bytes written for a segment and complete it when full.
    pub fn add_downloaded(&self, index: u32, bytes: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(segment) = state
            .segments
            .iter_mut()
            .find(|segment| segment.index == index)
        {
            segment.downloaded = segment
                .downloaded
                .saturating_add(bytes)
                .min(segment.len_bytes());
            if segment.downloaded >= segment.len_bytes() {
                segment.state = SegmentState::Complete;
                self.notify.notify_waiters();
            }
        }
    }

    /// Mark a segment complete, for example after a server confirms the range.
    pub fn mark_complete(&self, index: u32) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(segment) = state
            .segments
            .iter_mut()
            .find(|segment| segment.index == index)
        {
            segment.downloaded = segment.len_bytes();
            segment.state = SegmentState::Complete;
            self.notify.notify_waiters();
        }
    }

    /// Split the largest unfinished segment in half. Returns the new half.
    pub fn take_split(&self, minimum_bytes: u64) -> Option<Segment> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut best: Option<(usize, u64)> = None;
        for (position, segment) in state.segments.iter().enumerate() {
            if segment.state == SegmentState::Complete {
                continue;
            }
            let remaining = segment.remaining();
            if remaining >= minimum_bytes.saturating_mul(2)
                && best.is_none_or(|(_, best_remaining)| remaining > best_remaining)
            {
                best = Some((position, remaining));
            }
        }

        let (position, remaining) = best?;
        let segment = &mut state.segments[position];
        let split_at = segment.next_offset() + remaining / 2;
        if split_at <= segment.next_offset() || split_at > segment.end_byte {
            return None;
        }

        let old_end = segment.end_byte;
        segment.end_byte = split_at - 1;

        let new_segment = Segment {
            index: state.next_index,
            start_byte: split_at,
            end_byte: old_end,
            downloaded: 0,
            state: SegmentState::Active,
        };
        state.next_index = state.next_index.saturating_add(1);
        state.segments.push(new_segment);
        Some(new_segment)
    }

    /// Sum of downloaded bytes across all segments.
    #[must_use]
    pub fn downloaded_bytes(&self) -> u64 {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .segments
            .iter()
            .map(|segment| segment.downloaded)
            .sum()
    }

    /// Whether every segment has all of its bytes.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !state.segments.is_empty()
            && state
                .segments
                .iter()
                .all(|segment| segment.state == SegmentState::Complete)
    }

    /// Wait until any segment completes. Used by tests and future schedulers.
    pub async fn wait_for_completion(&self) {
        self.notify.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_contiguous_ranges() {
        let segments = plan_segments(100 * 1024 * 1024, 8);
        assert_eq!(segments.len(), 8);
        assert_eq!(segments.first().unwrap().start_byte, 0);
        assert_eq!(segments.last().unwrap().end_byte, 100 * 1024 * 1024 - 1);
        for pair in segments.windows(2) {
            assert_eq!(pair[0].end_byte + 1, pair[1].start_byte);
        }
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.len_bytes())
                .sum::<u64>(),
            100 * 1024 * 1024
        );
    }

    #[test]
    fn small_file_uses_one_connection() {
        assert_eq!(plan_segments(1024, 32).len(), 1);
        assert_eq!(plan_segments(0, 8).len(), 0);
    }

    #[test]
    fn splits_keep_ranges_contiguous() {
        let board = SegmentBoard::new(plan_segments(8 * 1024 * 1024, 2), 8 * 1024 * 1024);
        board.add_downloaded(0, board.segment(0).unwrap().len_bytes());
        let new_segment = board.take_split(MIN_SEGMENT_BYTES).unwrap();
        let segments = board.snapshot();
        let lower = segments.iter().find(|segment| segment.index == 1).unwrap();
        let upper = segments
            .iter()
            .find(|segment| segment.index == new_segment.index)
            .unwrap();
        assert_eq!(lower.end_byte + 1, upper.start_byte);
        assert_eq!(upper.end_byte, 8 * 1024 * 1024 - 1);
        assert!(lower.remaining() >= MIN_SEGMENT_BYTES);
        assert!(upper.remaining() >= MIN_SEGMENT_BYTES);
    }

    #[test]
    fn refuses_tiny_splits() {
        let board = SegmentBoard::new(plan_segments(128 * 1024, 2), 128 * 1024);
        assert!(board.take_split(MIN_SEGMENT_BYTES).is_none());
    }

    #[test]
    fn downloaded_bytes_are_clamped_and_complete() {
        let board = SegmentBoard::new(plan_segments(1024, 1), 1024);
        board.add_downloaded(0, 2048);
        assert_eq!(board.downloaded_bytes(), 1024);
        assert!(board.is_complete());
    }
}
