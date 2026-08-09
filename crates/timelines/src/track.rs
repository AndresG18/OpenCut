use clips::{Clip, ClipCategory, ClipPlacement};
use ids::TrackId;
use time::RationalTime;

use crate::TimelineError;

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub role: TrackRole,
    pub kind: TrackType,
    clips: Vec<Clip>,
    pub muted: bool,
    pub hidden: bool,
}

impl Track {
    pub(crate) fn new(id: TrackId, role: TrackRole, kind: TrackType) -> Self {
        Self {
            id,
            role,
            kind,
            clips: Vec::new(),
            muted: false,
            hidden: false,
        }
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub(crate) fn insert_clip(&mut self, clip: Clip) -> Result<(), TimelineError> {
        if !placement_matches(self.role.layout(), clip.placement()) {
            return Err(TimelineError::LayoutMismatch);
        }
        if !type_accepts(self.kind, clip.kind.category()) {
            return Err(TimelineError::ClipTypeMismatch);
        }

        if self.role.layout() == TrackLayout::Fixed {
            let start = clip
                .start_time()
                .expect("layout match guarantees a fixed clip start time");
            let end = clip
                .fixed_end_time()
                .map_err(|_| TimelineError::TimeOverflow)?
                .expect("layout match guarantees a fixed clip end time");
            if self.clips.iter().any(|existing| {
                let existing_start = existing
                    .start_time()
                    .expect("fixed tracks contain only fixed clips");
                let existing_end = existing
                    .fixed_end_time()
                    .expect("validated clip times must add without overflow")
                    .expect("fixed tracks contain only fixed clips");
                start.lt(&existing_end) && existing_start.lt(&end)
            }) {
                return Err(TimelineError::ClipOverlap);
            }

            let insert_at = self
                .clips
                .iter()
                .position(|existing| {
                    start.lt(&existing
                        .start_time()
                        .expect("fixed tracks contain only fixed clips"))
                })
                .unwrap_or(self.clips.len());
            self.clips.insert(insert_at, clip);
        } else {
            self.duration()
                .add(clip.duration())
                .map_err(|_| TimelineError::TimeOverflow)?;
            self.clips.push(clip);
        }
        Ok(())
    }

    /// The clip active at `at`, using a half-open `[start, end)` interval.
    pub fn clip_at(&self, at: RationalTime) -> Option<&Clip> {
        let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
        if at.lt(&zero) {
            return None;
        }

        match self.role.layout() {
            TrackLayout::Flow => {
                let mut cursor = zero;
                for clip in &self.clips {
                    let end = cursor
                        .add(clip.duration())
                        .expect("flow clip durations must compose without overflow");
                    if !at.lt(&cursor) && at.lt(&end) {
                        return Some(clip);
                    }
                    cursor = end;
                }
                None
            }
            TrackLayout::Fixed => self.clips.iter().find(|clip| {
                let start = clip
                    .start_time()
                    .expect("fixed tracks contain only fixed clips");
                let end = clip
                    .fixed_end_time()
                    .expect("validated clip times must add without overflow")
                    .expect("fixed tracks contain only fixed clips");
                !at.lt(&start) && at.lt(&end)
            }),
        }
    }

    pub(crate) fn duration(&self) -> RationalTime {
        let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
        match self.role.layout() {
            TrackLayout::Flow => self.clips.iter().fold(zero, |duration, clip| {
                duration
                    .add(clip.duration())
                    .expect("flow clip durations must compose without overflow")
            }),
            TrackLayout::Fixed => self.clips.iter().fold(zero, |duration, clip| {
                let end = clip
                    .fixed_end_time()
                    .expect("validated clip times must add without overflow")
                    .expect("fixed tracks contain only fixed clips");
                if duration.lt(&end) { end } else { duration }
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackRole {
    Main,
    Overlay,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackType {
    Video,
    Audio,
    Text,
    Vector,
    Adjustment,
    Effect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackLayout {
    Flow,
    Fixed,
}

impl TrackRole {
    pub fn layout(self) -> TrackLayout {
        match self {
            Self::Main => TrackLayout::Flow,
            Self::Overlay | Self::Audio => TrackLayout::Fixed,
        }
    }
}

pub(crate) fn track_order_index(role: TrackRole) -> usize {
    match role {
        TrackRole::Audio => 0,
        TrackRole::Main => 1,
        TrackRole::Overlay => 2,
    }
}

fn placement_matches(layout: TrackLayout, placement: ClipPlacement) -> bool {
    matches!(
        (layout, placement),
        (TrackLayout::Flow, ClipPlacement::Flow)
            | (TrackLayout::Fixed, ClipPlacement::Fixed { .. })
    )
}

fn type_accepts(track_type: TrackType, category: ClipCategory) -> bool {
    category == ClipCategory::Spacer
        || matches!(
            (track_type, category),
            (TrackType::Video, ClipCategory::Video)
                | (TrackType::Audio, ClipCategory::Audio)
                | (TrackType::Text, ClipCategory::Text)
                | (TrackType::Vector, ClipCategory::Vector)
                | (TrackType::Adjustment, ClipCategory::Adjustment)
                | (TrackType::Effect, ClipCategory::Effect)
        )
}
