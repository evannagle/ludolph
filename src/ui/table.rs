//! Minimal line tables for CLI output.

use console::style;

/// A simple table with minimal styling.
///
/// # Example
/// ```text
/// Service         Status    Uptime
/// ───────────────────────────────────
/// Telegram Bot    running   2d 4h
/// Vault Sync      idle      -
/// ```
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    col_widths: Vec<usize>,
}

impl Table {
    /// Create a new table with the given headers.
    #[must_use]
    pub fn new(headers: &[&str]) -> Self {
        let headers: Vec<String> = headers.iter().map(|&s| s.to_string()).collect();
        let col_widths: Vec<usize> = headers.iter().map(String::len).collect();

        Self {
            headers,
            rows: Vec::new(),
            col_widths,
        }
    }

    /// Add a row to the table.
    pub fn add_row(&mut self, cells: &[&str]) {
        let row: Vec<String> = cells.iter().map(|&s| s.to_string()).collect();

        // Update column widths
        for (i, cell) in row.iter().enumerate() {
            if i < self.col_widths.len() {
                self.col_widths[i] = self.col_widths[i].max(cell.len());
            }
        }

        self.rows.push(row);
    }

    /// Render the table to stdout.
    pub fn print(&self) {
        // Print header
        let header_line = self.format_row(&self.headers);
        println!("{}", style(header_line).bold());

        // Print separator
        let total_width: usize =
            self.col_widths.iter().sum::<usize>() + (self.col_widths.len() - 1) * 2;
        println!("{}", "─".repeat(total_width));

        // Print rows
        for row in &self.rows {
            self.print_row(row);
        }
    }

    fn format_row(&self, cells: &[String]) -> String {
        let mut parts = Vec::new();

        for (i, cell) in cells.iter().enumerate() {
            let width = self.col_widths.get(i).copied().unwrap_or(cell.len());
            parts.push(format!("{cell:<width$}"));
        }

        parts.join("  ")
    }

    fn print_row(&self, cells: &[String]) {
        let mut parts = Vec::new();

        for (i, cell) in cells.iter().enumerate() {
            let width = self.col_widths.get(i).copied().unwrap_or(cell.len());
            let formatted = format!("{cell:<width$}");

            // Style status-like cells (second column)
            let styled = if i == 1 {
                match cell.to_lowercase().as_str() {
                    "running" | "active" | "ok" | "connected" => {
                        format!("{}", style(formatted).green())
                    }
                    "stopped" | "error" | "failed" | "disconnected" => {
                        format!("{}", style(formatted).red())
                    }
                    "idle" | "pending" | "waiting" => {
                        format!("{}", style(formatted).dim())
                    }
                    _ => formatted,
                }
            } else {
                formatted
            };

            parts.push(styled);
        }

        println!("{}", parts.join("  "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_tracks_column_widths() {
        let mut table = Table::new(&["Name", "Value"]);
        table.add_row(&["Short", "A"]);
        table.add_row(&["Much Longer Name", "B"]);

        assert_eq!(table.col_widths[0], 16); // "Much Longer Name"
        assert_eq!(table.col_widths[1], 5); // "Value"
    }

    #[test]
    fn table_handles_empty() {
        let table = Table::new(&["A", "B", "C"]);
        assert_eq!(table.rows.len(), 0);
    }
}
