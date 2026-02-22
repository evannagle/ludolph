//! CLI styling components following the Ludolph style guide.
//!
//! See STYLE.md for complete formatting guidelines.

pub mod prompt;
mod spinner;
pub mod status;

pub use spinner::Spinner;
pub use status::StatusLine;
