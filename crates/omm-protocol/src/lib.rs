pub mod envelope;
pub mod messages;
pub mod musical_time;
pub mod note_event;
pub mod params;
pub mod scheduler;
pub mod source_timeline;
pub mod validation;

pub use envelope::{Envelope, MessagePriority, MessageSource};
pub use messages::{EngineCommand, EngineEvent, OutputRoute, SessionMode, VoiceType};
pub use musical_time::{
    frame_to_musical_time, musical_time_to_frame, quantize_to_next_bar, quantize_to_next_beat,
    MusicalTime, TimeSignature, Transport, TICKS_PER_QUARTER,
};
pub use note_event::{NoteEvent, NoteEventBatch, NoteEventValidationError};
pub use params::{ParamId, RtTarget};
pub use scheduler::{
    frames_for_duration_ms, resolve_trigger, validate_schedule_request, ActionOrigin, EngineTime,
    ScheduleRequestTiming, ScheduleTrigger, ScheduleValidation, ScheduleValidationError,
    ScheduledActionId, PLANNED_ACTION_MIN_LEAD_MS,
};
pub use source_timeline::{
    GeneratedEngine, PlaybackState, PlaybackStatusAuthority, SourceAssetRef, SourceEffectStatus,
    SourceEqStatus, SourceInstanceId, SourceKind, SourcePlaybackStatus, SourceTimelinePlacement,
    SourceTimelineSnapshot, SourceTimelineValidationError, TimelineActiveWindow,
    TimelineSourceInstance,
};
