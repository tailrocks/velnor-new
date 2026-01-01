//! Runner-observed GitHub event resolution for plan and merge.
//!
//! [`request_event`] maps one GitHub event name plus payload to the
//! workflow event and extracts the base/head refs, so plan-request
//! materialization and merge-time capture can never disagree about
//! fork handling or ref selection.

pub mod request_event;
