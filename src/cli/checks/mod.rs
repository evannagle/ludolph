//! Diagnostic check system for Ludolph.
//!
//! This module provides a unified check system used by both `lu doctor` (diagnostics)
//! and `lu setup` (installation verification). Checks have dependencies and are run
//! in order, with failed dependencies causing subsequent checks to be skipped.

mod config;
mod network;
mod services;

use std::collections::HashMap;
use std::fmt;

use crate::config::Config;

pub use config::{config_exists, config_valid, vault_accessible};
pub use network::{pi_mcp_connectivity, pi_reachable};
pub use services::{
    fix_mcp_config, mac_mcp_port_available, mac_mcp_running, mcp_config_consistent,
    pi_service_running,
};

/// Result of running a diagnostic check.
#[derive(Debug, Clone)]
pub enum CheckResult {
    /// Check passed successfully.
    Pass { message: String },
    /// Check failed with diagnostic information.
    Fail {
        message: String,
        fix_hint: String,
        doc_anchor: &'static str,
    },
    /// Check was skipped due to missing dependency.
    Skip { reason: String },
}

impl CheckResult {
    /// Create a passing result.
    #[must_use]
    pub fn pass(message: impl Into<String>) -> Self {
        Self::Pass {
            message: message.into(),
        }
    }

    /// Create a failing result.
    #[must_use]
    pub fn fail(
        message: impl Into<String>,
        fix_hint: impl Into<String>,
        doc_anchor: &'static str,
    ) -> Self {
        Self::Fail {
            message: message.into(),
            fix_hint: fix_hint.into(),
            doc_anchor,
        }
    }

    /// Create a skipped result.
    #[must_use]
    pub fn skip(reason: impl Into<String>) -> Self {
        Self::Skip {
            reason: reason.into(),
        }
    }

    /// Returns true if the check passed.
    #[must_use]
    pub const fn is_pass(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }

    /// Returns true if the check failed.
    #[must_use]
    #[allow(dead_code)]
    pub const fn is_fail(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }

    /// Returns true if the check was skipped.
    #[must_use]
    #[allow(dead_code)]
    pub const fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }
}

impl fmt::Display for CheckResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass { message } | Self::Fail { message, .. } => write!(f, "{message}"),
            Self::Skip { reason } => write!(f, "{reason}"),
        }
    }
}

/// Context passed to check functions.
#[derive(Debug, Clone)]
pub struct CheckContext {
    /// Loaded configuration (if available).
    pub config: Option<Config>,
    /// Results of previously run checks.
    pub results: HashMap<&'static str, CheckResult>,
}

impl CheckContext {
    /// Create a new check context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: Config::load().ok(),
            results: HashMap::new(),
        }
    }

    /// Check if a dependency passed.
    #[must_use]
    pub fn dep_passed(&self, name: &str) -> bool {
        self.results.get(name).is_some_and(CheckResult::is_pass)
    }

    /// Check if all dependencies passed.
    #[must_use]
    #[allow(dead_code)]
    pub fn all_deps_passed(&self, deps: &[&str]) -> bool {
        deps.iter().all(|d| self.dep_passed(d))
    }
}

impl Default for CheckContext {
    fn default() -> Self {
        Self::new()
    }
}

/// A diagnostic check definition.
pub struct Check {
    /// Unique identifier for this check.
    pub name: &'static str,
    /// Human-readable label (for future use in verbose output).
    #[allow(dead_code)]
    pub label: &'static str,
    /// Names of checks that must pass before this one runs.
    pub depends_on: &'static [&'static str],
    /// The check function.
    pub run: fn(&CheckContext) -> CheckResult,
}

impl Check {
    /// Run the check, respecting dependencies.
    #[must_use]
    pub fn execute(&self, ctx: &CheckContext) -> CheckResult {
        // Check dependencies
        for dep in self.depends_on {
            if !ctx.dep_passed(dep) {
                return CheckResult::skip(format!("Requires {dep} to pass"));
            }
        }

        // Run the check
        (self.run)(ctx)
    }
}

/// Get all checks in dependency order.
#[must_use]
pub fn all_checks() -> Vec<Check> {
    vec![
        Check {
            name: "config_exists",
            label: "Config file exists",
            depends_on: &[],
            run: config_exists,
        },
        Check {
            name: "config_valid",
            label: "Config file valid",
            depends_on: &["config_exists"],
            run: config_valid,
        },
        Check {
            name: "vault_accessible",
            label: "Vault accessible",
            depends_on: &["config_valid"],
            run: vault_accessible,
        },
        Check {
            name: "mcp_config_consistent",
            label: "MCP configuration consistent",
            depends_on: &["config_valid"],
            run: mcp_config_consistent,
        },
        Check {
            name: "mac_mcp_port_available",
            label: "MCP port available",
            depends_on: &["mcp_config_consistent"],
            run: mac_mcp_port_available,
        },
        Check {
            name: "mac_mcp_running",
            label: "Mac MCP server running",
            depends_on: &["mac_mcp_port_available"],
            run: mac_mcp_running,
        },
        Check {
            name: "pi_reachable",
            label: "Pi reachable via SSH",
            depends_on: &["config_valid"],
            run: pi_reachable,
        },
        Check {
            name: "pi_service_running",
            label: "Pi service running",
            depends_on: &["pi_reachable"],
            run: pi_service_running,
        },
        Check {
            name: "pi_mcp_connectivity",
            label: "Pi can reach Mac MCP",
            depends_on: &["pi_reachable", "mac_mcp_running"],
            run: pi_mcp_connectivity,
        },
    ]
}

/// Run all checks and return results.
#[must_use]
pub fn run_all_checks() -> (CheckContext, Vec<(&'static str, CheckResult)>) {
    let mut ctx = CheckContext::new();
    let mut results = Vec::new();

    for check in all_checks() {
        let result = check.execute(&ctx);
        ctx.results.insert(check.name, result.clone());
        results.push((check.name, result));
    }

    (ctx, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_result_constructors() {
        let pass = CheckResult::pass("OK");
        assert!(pass.is_pass());

        let fail = CheckResult::fail("Failed", "Fix it", "test");
        assert!(fail.is_fail());

        let skip = CheckResult::skip("Skipped");
        assert!(skip.is_skip());
    }

    #[test]
    fn check_context_deps() {
        let mut ctx = CheckContext::new();
        ctx.results.insert("test", CheckResult::pass("OK"));

        assert!(ctx.dep_passed("test"));
        assert!(!ctx.dep_passed("missing"));
        assert!(ctx.all_deps_passed(&["test"]));
        assert!(!ctx.all_deps_passed(&["test", "missing"]));
    }
}
