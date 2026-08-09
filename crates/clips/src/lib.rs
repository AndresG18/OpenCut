//! Closed editor-core clip vocabulary.
//!
//! Clips describe editing concepts and exact media source ranges. The media
//! engine never receives these types; a compositor will later translate them
//! into product-neutral render work.

use ids::{AssetId, ClipId, TimelineId};
use thiserror::Error;
use time::RationalTime;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClipError {
    #[error("clip duration must be greater than zero")]
    InvalidDuration,
    #[error("clip and source times cannot be negative")]
    NegativeTime,
    #[error("clip time range exceeds exact-time limits")]
    TimeOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub id: ClipId,
    pub kind: ClipKind,
    duration: RationalTime,
    placement: ClipPlacement,
}

impl Clip {
    /// Construct a clip for a flow track. No timeline position is stored;
    /// position is the prefix sum of preceding clip durations.
    pub fn flow(id: ClipId, kind: ClipKind, duration: RationalTime) -> Result<Self, ClipError> {
        validate_duration(duration)?;
        validate_kind_time(&kind)?;
        Ok(Self {
            id,
            kind,
            duration,
            placement: ClipPlacement::Flow,
        })
    }

    /// Construct a clip for a fixed track at an explicit timeline position.
    pub fn fixed(
        id: ClipId,
        kind: ClipKind,
        start_time: RationalTime,
        duration: RationalTime,
    ) -> Result<Self, ClipError> {
        validate_duration(duration)?;
        validate_non_negative(start_time)?;
        validate_kind_time(&kind)?;
        start_time
            .add(duration)
            .map_err(|_| ClipError::TimeOverflow)?;
        Ok(Self {
            id,
            kind,
            duration,
            placement: ClipPlacement::Fixed { start_time },
        })
    }

    pub fn duration(&self) -> RationalTime {
        self.duration
    }

    pub fn placement(&self) -> ClipPlacement {
        self.placement
    }

    pub fn start_time(&self) -> Option<RationalTime> {
        match self.placement {
            ClipPlacement::Flow => None,
            ClipPlacement::Fixed { start_time } => Some(start_time),
        }
    }

    pub fn fixed_end_time(&self) -> Result<Option<RationalTime>, time::TimeError> {
        self.start_time()
            .map(|start| start.add(self.duration))
            .transpose()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipPlacement {
    Flow,
    Fixed { start_time: RationalTime },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Video(VideoClip),
    Audio(AudioClip),
    Image(ImageClip),
    Text(TextClip),
    Vector(VectorClip),
    Adjustment(AdjustmentClip),
    Effect(EffectClip),
    Compound(CompoundClip),
    Spacer,
}

impl ClipKind {
    pub fn category(&self) -> ClipCategory {
        match self {
            Self::Video(_) | Self::Image(_) | Self::Compound(_) => ClipCategory::Video,
            Self::Audio(_) => ClipCategory::Audio,
            Self::Text(_) => ClipCategory::Text,
            Self::Vector(_) => ClipCategory::Vector,
            Self::Adjustment(_) => ClipCategory::Adjustment,
            Self::Effect(_) => ClipCategory::Effect,
            Self::Spacer => ClipCategory::Spacer,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipCategory {
    Video,
    Audio,
    Text,
    Vector,
    Adjustment,
    Effect,
    Spacer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoClip {
    pub asset_id: AssetId,
    pub source_start: RationalTime,
}

impl VideoClip {
    pub fn new(asset_id: AssetId, source_start: RationalTime) -> Result<Self, ClipError> {
        validate_non_negative(source_start)?;
        Ok(Self {
            asset_id,
            source_start,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioClip {
    pub asset_id: AssetId,
    pub source_start: RationalTime,
}

impl AudioClip {
    pub fn new(asset_id: AssetId, source_start: RationalTime) -> Result<Self, ClipError> {
        validate_non_negative(source_start)?;
        Ok(Self {
            asset_id,
            source_start,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageClip {
    pub asset_id: AssetId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextClip {
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorClip {
    pub svg: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdjustmentClip;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectClip {
    pub effect_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompoundClip {
    pub timeline_id: TimelineId,
}

fn validate_duration(duration: RationalTime) -> Result<(), ClipError> {
    let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
    if !zero.lt(&duration) {
        return Err(ClipError::InvalidDuration);
    }
    Ok(())
}

fn validate_non_negative(value: RationalTime) -> Result<(), ClipError> {
    let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
    if value.lt(&zero) {
        return Err(ClipError::NegativeTime);
    }
    Ok(())
}

fn validate_kind_time(kind: &ClipKind) -> Result<(), ClipError> {
    match kind {
        ClipKind::Video(clip) => validate_non_negative(clip.source_start),
        ClipKind::Audio(clip) => validate_non_negative(clip.source_start),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(value: i64) -> RationalTime {
        RationalTime::new(value, 1).unwrap()
    }

    #[test]
    fn flow_clips_do_not_store_timeline_positions() {
        let clip = Clip::flow(ClipId(1), ClipKind::Spacer, seconds(5)).unwrap();
        assert_eq!(clip.placement(), ClipPlacement::Flow);
        assert_eq!(clip.start_time(), None);
    }

    #[test]
    fn fixed_clip_end_is_exact() {
        let clip = Clip::fixed(
            ClipId(1),
            ClipKind::Spacer,
            RationalTime::new(1, 3).unwrap(),
            RationalTime::new(2, 3).unwrap(),
        )
        .unwrap();
        assert_eq!(clip.fixed_end_time().unwrap(), Some(seconds(1)));
    }

    #[test]
    fn negative_source_times_are_rejected() {
        assert_eq!(
            VideoClip::new(AssetId(1), seconds(-1)),
            Err(ClipError::NegativeTime)
        );
    }

    #[test]
    fn overflowing_fixed_ranges_are_rejected() {
        assert_eq!(
            Clip::fixed(
                ClipId(1),
                ClipKind::Spacer,
                RationalTime::new(i64::MAX, 1).unwrap(),
                seconds(1),
            ),
            Err(ClipError::TimeOverflow)
        );
    }
}
