use clips::Clip;
use ids::{TimelineId, TrackId};
use time::RationalTime;

use crate::TimelineError;
use crate::track::{Track, TrackRole, TrackType, track_order_index};

pub struct Timeline {
    pub id: TimelineId,
    pub fps: RationalTime,
    tracks: Vec<Track>,
    next_track_id: u64,
}

impl Timeline {
    /// Construct a timeline with its always-present flow-layout main track.
    pub fn new(id: TimelineId, fps: RationalTime) -> Self {
        let mut timeline = Self {
            id,
            fps,
            tracks: Vec::new(),
            next_track_id: 0,
        };
        timeline
            .create_track(TrackRole::Main, TrackType::Video)
            .expect("a fresh timeline cannot already contain a main track");
        timeline
    }

    pub fn create_track(
        &mut self,
        role: TrackRole,
        kind: TrackType,
    ) -> Result<TrackId, TimelineError> {
        if !role_accepts_type(role, kind) {
            return Err(TimelineError::TrackRoleTypeMismatch);
        }
        if role == TrackRole::Main
            && self
                .tracks
                .iter()
                .any(|track| track.role == TrackRole::Main)
        {
            return Err(TimelineError::DuplicateMainTrack);
        }

        let id = TrackId(self.next_track_id);
        self.next_track_id += 1;
        let insert_at = self
            .tracks
            .iter()
            .position(|track| track_order_index(track.role) > track_order_index(role))
            .unwrap_or(self.tracks.len());
        self.tracks.insert(insert_at, Track::new(id, role, kind));
        Ok(id)
    }

    pub fn remove_track(&mut self, track_id: TrackId) -> Result<Track, TimelineError> {
        let index = self
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .expect("caller-guaranteed: track_id names a track on this timeline");
        if self.tracks[index].role == TrackRole::Main {
            return Err(TimelineError::CannotDeleteMainTrack);
        }
        Ok(self.tracks.remove(index))
    }

    pub fn insert_clip(&mut self, track_id: TrackId, clip: Clip) -> Result<(), TimelineError> {
        self.tracks
            .iter_mut()
            .find(|track| track.id == track_id)
            .expect("caller-guaranteed: track_id names a track on this timeline")
            .insert_clip(clip)
    }

    pub fn main_track_id(&self) -> TrackId {
        self.tracks
            .iter()
            .find(|track| track.role == TrackRole::Main)
            .expect("every timeline has one main track")
            .id
    }

    pub fn track(&self, track_id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == track_id)
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Latest point in time reached by any track, or zero when empty.
    pub fn duration(&self) -> RationalTime {
        let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
        self.tracks.iter().fold(zero, |duration, track| {
            let track_duration = track.duration();
            if duration.lt(&track_duration) {
                track_duration
            } else {
                duration
            }
        })
    }
}

fn role_accepts_type(role: TrackRole, kind: TrackType) -> bool {
    match role {
        TrackRole::Main => kind == TrackType::Video,
        TrackRole::Audio => kind == TrackType::Audio,
        TrackRole::Overlay => kind != TrackType::Audio,
    }
}

#[cfg(test)]
mod tests {
    use clips::{Clip, ClipKind};
    use ids::ClipId;

    use super::*;

    fn seconds(value: i64) -> RationalTime {
        RationalTime::new(value, 1).unwrap()
    }

    #[test]
    fn creates_main_track_and_preserves_track_order() {
        let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
        timeline
            .create_track(TrackRole::Overlay, TrackType::Text)
            .unwrap();
        timeline
            .create_track(TrackRole::Audio, TrackType::Audio)
            .unwrap();

        assert_eq!(
            timeline
                .tracks()
                .iter()
                .map(|track| track.role)
                .collect::<Vec<_>>(),
            vec![TrackRole::Audio, TrackRole::Main, TrackRole::Overlay]
        );
        assert_eq!(
            timeline.create_track(TrackRole::Main, TrackType::Video),
            Err(TimelineError::DuplicateMainTrack)
        );
        assert_eq!(
            timeline.create_track(TrackRole::Audio, TrackType::Video),
            Err(TimelineError::TrackRoleTypeMismatch)
        );
    }

    #[test]
    fn flow_track_positions_are_prefix_sums() {
        let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
        let main = timeline.main_track_id();
        timeline
            .insert_clip(
                main,
                Clip::flow(ClipId(1), ClipKind::Spacer, seconds(5)).unwrap(),
            )
            .unwrap();
        timeline
            .insert_clip(
                main,
                Clip::flow(ClipId(2), ClipKind::Spacer, seconds(7)).unwrap(),
            )
            .unwrap();

        let track = timeline.track(main).unwrap();
        assert_eq!(track.clip_at(seconds(4)).unwrap().id, ClipId(1));
        assert_eq!(track.clip_at(seconds(5)).unwrap().id, ClipId(2));
        assert_eq!(track.clip_at(seconds(12)), None);
        assert_eq!(timeline.duration(), seconds(12));
    }

    #[test]
    fn fixed_tracks_reject_overlaps_and_drive_timeline_duration() {
        let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
        let overlay = timeline
            .create_track(TrackRole::Overlay, TrackType::Video)
            .unwrap();
        timeline
            .insert_clip(
                overlay,
                Clip::fixed(ClipId(1), ClipKind::Spacer, seconds(20), seconds(5)).unwrap(),
            )
            .unwrap();

        assert_eq!(
            timeline.insert_clip(
                overlay,
                Clip::fixed(ClipId(2), ClipKind::Spacer, seconds(24), seconds(3)).unwrap(),
            ),
            Err(TimelineError::ClipOverlap)
        );
        assert_eq!(timeline.duration(), seconds(25));
        assert_eq!(
            timeline
                .track(overlay)
                .unwrap()
                .clip_at(seconds(24))
                .unwrap()
                .id,
            ClipId(1)
        );
    }

    #[test]
    fn layout_mismatch_is_rejected() {
        let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
        assert_eq!(
            timeline.insert_clip(
                timeline.main_track_id(),
                Clip::fixed(ClipId(1), ClipKind::Spacer, seconds(0), seconds(1)).unwrap(),
            ),
            Err(TimelineError::LayoutMismatch)
        );
    }

    #[test]
    fn flow_duration_overflow_is_a_domain_error() {
        let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
        let main = timeline.main_track_id();
        timeline
            .insert_clip(
                main,
                Clip::flow(
                    ClipId(1),
                    ClipKind::Spacer,
                    RationalTime::new(i64::MAX, 1).unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            timeline.insert_clip(
                main,
                Clip::flow(ClipId(2), ClipKind::Spacer, seconds(1)).unwrap(),
            ),
            Err(TimelineError::TimeOverflow)
        );
    }
}
