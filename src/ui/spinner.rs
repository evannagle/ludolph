//! Pi-themed spinner that cycles through digits of pi.

use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};

/// First 50 digits of pi (after the decimal point).
const PI_DIGITS: &str = "14159265358979323846264338327950288419716939937510";

/// A spinner that displays sliding windows of pi digits.
///
/// The spinner shows 5 digits at a time, starting with zeros
/// and gradually revealing pi digits from the right:
/// ```text
/// [00000] → [00003] → [00031] → [00314] → [03141] → [31415] → [14159] → ...
/// ```
pub struct PiSpinner {
    bar: ProgressBar,
    message: String,
}

impl PiSpinner {
    /// Create a new pi spinner with the given header message.
    #[must_use]
    pub fn new(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();

        // Generate tick strings: sliding window through pi digits
        let tick_strings = Self::generate_tick_strings();

        bar.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&tick_strings.iter().map(String::as_str).collect::<Vec<_>>())
                .template("{msg} [{spinner}]")
                .expect("valid template"),
        );

        bar.enable_steady_tick(Duration::from_millis(200));
        bar.set_message(format!("{}", style(message).bold()));

        Self {
            bar,
            message: message.to_string(),
        }
    }

    /// Generate the sliding window tick strings.
    fn generate_tick_strings() -> Vec<String> {
        let mut strings = Vec::new();

        // Start with zeros, pi digits shift in from right
        // [00000] → [00003] → [00031] → [00314] → [03141] → [31415]
        let intro = "00003";
        for i in 0..=5 {
            let zeros = "0".repeat(5 - i);
            let pi_part = if i == 0 {
                String::new()
            } else {
                intro[..i].to_string()
            };
            strings.push(format!("{zeros}{pi_part}"));
        }

        // Then cycle through pi digits with 5-char window
        let full_pi = format!("3{PI_DIGITS}");
        for i in 0..PI_DIGITS.len().saturating_sub(4) {
            strings.push(full_pi[i..i + 5].to_string());
        }

        strings
    }

    /// Complete the spinner, replacing it with a checkmark.
    pub fn finish(&self) {
        self.bar.finish_and_clear();
        println!("\n{} {}\n", style(&self.message).bold(), style("✓").green());
    }
}

impl Drop for PiSpinner {
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
    fn tick_strings_start_with_zeros() {
        let strings = PiSpinner::generate_tick_strings();
        assert_eq!(strings[0], "00000");
    }

    #[test]
    fn tick_strings_reach_pi_start() {
        let strings = PiSpinner::generate_tick_strings();
        assert!(strings.contains(&"31415".to_string()));
    }

    #[test]
    fn tick_strings_all_five_chars() {
        let strings = PiSpinner::generate_tick_strings();
        for s in &strings {
            assert_eq!(s.len(), 5, "Expected 5 chars, got: {s}");
        }
    }
}
