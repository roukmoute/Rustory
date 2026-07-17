//! Update-availability domain (`Update Availability Contract`): knowing
//! that a newer OFFICIAL version exists — pure, framework-free, zero I/O.
//! Information only: the update GESTURE (download, install, feed) is a
//! separate capability this module deliberately excludes.

pub mod availability;

pub use availability::{
    decide_update_check, format_release_version, parse_release_version, resolve_availability,
    update_headline, update_notice, ReleaseProbe, ReleaseVersion, UpdateAvailability,
    UpdateCheckDecision, UpdateCheckSkipReason,
};
