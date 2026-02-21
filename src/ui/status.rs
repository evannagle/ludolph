//! Status indicators for CLI output.
//!
//! Provides `[•ok]` and `[•!!]` status prefixes.

use console::style;

/// Status indicator states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Success - green `[•ok]`
    Ok,
    /// Error - red `[•!!]`
    Error,
}

impl Status {
    /// Render the status indicator as a styled string.
    #[must_use]
    pub fn render(self) -> String {
        match self {
            Self::Ok => format!("[{}]", style("•ok").green()),
            Self::Error => format!("[{}]", style("•!!").red()),
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// A status line with indicator and message.
pub struct StatusLine {
    status: Status,
    message: String,
}

impl StatusLine {
    /// Create a new status line.
    #[must_use]
    pub fn new(status: Status, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// Create a success status line.
    #[must_use]
    pub fn ok(message: impl Into<String>) -> Self {
        Self::new(Status::Ok, message)
    }

    /// Create an error status line.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Status::Error, message)
    }

    /// Print the status line with proper indentation.
    pub fn print(&self) {
        println!("  {} {}", self.status, self.message);
    }
}

/// Print an error message with help text.
pub fn print_error(message: &str, help: Option<&str>) {
    println!();
    StatusLine::error(message).print();

    if let Some(help_text) = help {
        println!();
        for line in help_text.lines() {
            println!("  {line}");
        }
    }
    println!();
}

/// Print a success completion message.
pub fn print_success(title: &str, details: Option<&str>) {
    println!();
    println!("{} {}", style(title).bold(), style("✓").green());
    println!();

    if let Some(detail_text) = details {
        for line in detail_text.lines() {
            println!("  {line}");
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_renders_correctly() {
        // Just verify no panics - actual styling is hard to test
        let _ = Status::Ok.render();
        let _ = Status::Error.render();
    }

    #[test]
    fn status_line_creates_variants() {
        let ok = StatusLine::ok("Test");
        assert_eq!(ok.status, Status::Ok);

        let err = StatusLine::error("Test");
        assert_eq!(err.status, Status::Error);
    }
}
