//! Editor-core timeline primitive: tracks of clips with flow/fixed layout.
//!
//! The main track is flow layout: clip position is the prefix sum of earlier
//! durations. Overlay and audio tracks are fixed layout and store explicit
//! positions. Layout is derived from track role, never chosen by a consumer.

mod error;
mod timeline;
mod track;

pub use error::TimelineError;
pub use timeline::Timeline;
pub use track::{Track, TrackLayout, TrackRole, TrackType};
