//! Simple ponging ball spinner.

use std::io::{self, Write};
use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};

/// A spinner that displays a bouncing ball animation.
///
/// The ball pongs back and forth:
/// ```text
/// [*  ] → [ * ] → [  *] → [ * ] → repeat
/// ```
pub struct Spinner {
    bar: ProgressBar,
}

impl Spinner {
    /// Create a new spinner with the given message.
    #[must_use]
    pub fn new(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();

        // Ponging ball animation
        let tick_strings = ["*  ", " * ", "  *", " * "];

        bar.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&tick_strings)
                .template("[{spinner}] {msg}")
                .expect("valid template"),
        );

        bar.enable_steady_tick(Duration::from_millis(200));
        bar.set_message(message.to_string());

        Self { bar }
    }

    /// Complete the spinner with a checkmark on the same line.
    pub fn finish(&self) {
        self.bar.finish_and_clear();
        let _ = writeln!(
            io::stdout(),
            "{} {}",
            style("[•ok]").green(),
            self.bar.message()
        );
    }

    /// Complete the spinner with an error indicator on the same line.
    pub fn finish_error(&self) {
        self.bar.finish_and_clear();
        let _ = writeln!(
            io::stdout(),
            "{} {}",
            style("[•!!]").red(),
            self.bar.message()
        );
    }

    /// Clear the spinner without printing anything.
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.bar.finish_and_clear();
    }

    /// Update the spinner message.
    #[allow(dead_code)]
    pub fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if !self.bar.is_finished() {
            self.bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_creates_without_panic() {
        let spinner = Spinner::new("Testing");
        spinner.clear();
    }

    #[test]
    fn spinner_finish_without_panic() {
        let spinner = Spinner::new("Testing");
        spinner.finish();
    }

    #[test]
    fn spinner_finish_error_without_panic() {
        let spinner = Spinner::new("Testing");
        spinner.finish_error();
    }
}
