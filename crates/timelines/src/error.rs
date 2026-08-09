use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimelineError {
    #[error("a timeline can only have one main track")]
    DuplicateMainTrack,
    #[error("the main track can never be deleted")]
    CannotDeleteMainTrack,
    #[error("clip placement does not match the track's derived layout")]
    LayoutMismatch,
    #[error("clip kind is not compatible with this track type")]
    ClipTypeMismatch,
    #[error("fixed-layout clips cannot overlap")]
    ClipOverlap,
    #[error("track role and track type are incompatible")]
    TrackRoleTypeMismatch,
    #[error("clip placement exceeds exact-time limits")]
    TimeOverflow,
}
