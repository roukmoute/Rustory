//! Spoken announcements — the pure rules behind the synthesized voice that
//! lets a child pick an episode on the Lunii wheel: what is said for a
//! series title, an episode title and the menu question. No synthesis
//! here (that is infrastructure, per OS); only the text a voice will read.

pub mod announcement;

pub use announcement::{
    spoken_episode_title, spoken_series_title, MAX_SPOKEN_CHARS, MENU_QUESTION,
};
