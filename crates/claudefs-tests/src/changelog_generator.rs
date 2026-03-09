//! Changelog Generator
//!
//! Generates and updates CHANGELOG.md with daily test metrics and status updates.
//! Provides automated daily entries with per-crate test counts, issues resolved, and upcoming work.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Test statistics for a changelog entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestStats {
    pub unit_tests_total: usize,
    pub unit_tests_passed: usize,
    pub unit_tests_failed: usize,
    pub integration_tests_total: usize,
    pub flaky_test_count: usize,
}

impl TestStats {
    pub fn new() -> Self {
        Self {
            unit_tests_total: 0,
            unit_tests_passed: 0,
            unit_tests_failed: 0,
            integration_tests_total: 0,
            flaky_test_count: 0,
        }
    }

    pub fn pass_rate(&self) -> f64 {
        if self.unit_tests_total == 0 {
            1.0
        } else {
            self.unit_tests_passed as f64 / self.unit_tests_total as f64
        }
    }
}

impl Default for TestStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-crate status in changelog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateStatus {
    pub crate_name: String,
    pub test_count: usize,
    pub status: String, // "✅", "🟡", "🔴"
}

/// A single changelog entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub date: String,
    pub test_stats: TestStats,
    pub crate_statuses: Vec<CrateStatus>,
    pub issues_resolved: Vec<String>,
    pub flaky_alerts: Vec<String>,
    pub upcoming: Vec<String>,
}

impl ChangelogEntry {
    pub fn new(date: String) -> Self {
        Self {
            date,
            test_stats: TestStats::new(),
            crate_statuses: Vec::new(),
            issues_resolved: Vec::new(),
            flaky_alerts: Vec::new(),
            upcoming: Vec::new(),
        }
    }

    pub fn with_stats(mut self, stats: TestStats) -> Self {
        self.test_stats = stats;
        self
    }

    pub fn with_crates(mut self, crates: Vec<CrateStatus>) -> Self {
        self.crate_statuses = crates;
        self
    }

    pub fn with_resolved(mut self, issues: Vec<String>) -> Self {
        self.issues_resolved = issues;
        self
    }

    pub fn with_flaky_alerts(mut self, alerts: Vec<String>) -> Self {
        self.flaky_alerts = alerts;
        self
    }

    pub fn with_upcoming(mut self, upcoming: Vec<String>) -> Self {
        self.upcoming = upcoming;
        self
    }

    /// Generate markdown for this entry
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // Date header
        md.push_str(&format!("## {}\n\n", self.date));

        // Test Health section
        md.push_str("### Test Health ✅\n");
        if self.test_stats.unit_tests_total > 0 {
            md.push_str(&format!(
                "- **Unit tests:** {} passing ({} failed)\n",
                self.test_stats.unit_tests_passed, self.test_stats.unit_tests_failed
            ));
            md.push_str(&format!(
                "  - Pass rate: {:.1}%\n",
                self.test_stats.pass_rate() * 100.0
            ));
        }
        if self.test_stats.integration_tests_total > 0 {
            md.push_str(&format!(
                "- **Integration tests:** {} total\n",
                self.test_stats.integration_tests_total
            ));
        }
        if self.test_stats.flaky_test_count == 0 {
            md.push_str("- **Flaky tests:** 0 (all stable) ✅\n");
        } else {
            md.push_str(&format!(
                "- **Flaky tests:** {} flagged for review\n",
                self.test_stats.flaky_test_count
            ));
        }
        md.push('\n');

        // Crate Status section
        if !self.crate_statuses.is_empty() {
            md.push_str("### Crate Status\n");
            for crate_status in &self.crate_statuses {
                md.push_str(&format!(
                    "- {} ({}): {} tests {}\n",
                    crate_status.crate_name, crate_status.crate_name.replace("claudefs-", "A"),
                    crate_status.test_count, crate_status.status
                ));
            }
            md.push('\n');
        }

        // Issues Resolved
        if !self.issues_resolved.is_empty() {
            md.push_str("### Issues Resolved\n");
            for issue in &self.issues_resolved {
                md.push_str(&format!("- {}\n", issue));
            }
            md.push('\n');
        }

        // Flaky Alerts
        if !self.flaky_alerts.is_empty() {
            md.push_str("### 🐛 Flaky Test Alerts\n");
            for alert in &self.flaky_alerts {
                md.push_str(&format!("- {}\n", alert));
            }
            md.push('\n');
        }

        // Upcoming
        if !self.upcoming.is_empty() {
            md.push_str("### Upcoming\n");
            for item in &self.upcoming {
                md.push_str(&format!("- {}\n", item));
            }
            md.push('\n');
        }

        md
    }
}

/// Updates CHANGELOG.md with a new entry
pub fn update_changelog(changelog_path: &Path, entry: &ChangelogEntry) -> io::Result<()> {
    let mut content = if changelog_path.exists() {
        fs::read_to_string(changelog_path)?
    } else {
        "# ClaudeFS Changelog\n\nAll notable changes to ClaudeFS are documented here.\n\n".to_string()
    };

    // Check if today's entry already exists
    if content.contains(&format!("## {}", entry.date)) {
        return Ok(()); // Already has today's entry
    }

    // Insert new entry at the top (after any description/intro)
    let marker = "# ClaudeFS Changelog";
    if let Some(pos) = content.find("\n\n") {
        let mut new_content = String::new();
        new_content.push_str(&content[..pos + 2]);
        new_content.push_str(&entry.to_markdown());
        new_content.push_str(&content[pos + 2..]);
        content = new_content;
    } else {
        content.push_str(&entry.to_markdown());
    }

    fs::write(changelog_path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_stats() {
        let stats = TestStats::new();
        assert_eq!(stats.unit_tests_total, 0);
        assert_eq!(stats.pass_rate(), 1.0);
    }

    #[test]
    fn test_pass_rate_calculation() {
        let stats = TestStats {
            unit_tests_total: 100,
            unit_tests_passed: 95,
            unit_tests_failed: 5,
            integration_tests_total: 0,
            flaky_test_count: 1,
        };
        assert_eq!(stats.pass_rate(), 0.95);
    }

    #[test]
    fn test_changelog_entry_markdown() {
        let stats = TestStats {
            unit_tests_total: 6300,
            unit_tests_passed: 6300,
            unit_tests_failed: 0,
            integration_tests_total: 0,
            flaky_test_count: 0,
        };

        let entry = ChangelogEntry::new("2026-03-08".to_string())
            .with_stats(stats)
            .with_crates(vec![
                CrateStatus {
                    crate_name: "claudefs-storage".to_string(),
                    test_count: 1301,
                    status: "✅".to_string(),
                },
                CrateStatus {
                    crate_name: "claudefs-meta".to_string(),
                    test_count: 1035,
                    status: "✅".to_string(),
                },
            ])
            .with_resolved(vec!["Issue #22: fsinfo flaky test fixed by A5".to_string()])
            .with_upcoming(vec!["Block 4: Performance baselines".to_string()]);

        let md = entry.to_markdown();
        assert!(md.contains("## 2026-03-08"));
        assert!(md.contains("6300 passing"));
        assert!(md.contains("claudefs-storage"));
        assert!(md.contains("Issue #22"));
        assert!(md.contains("Block 4"));
    }

    #[test]
    fn test_crate_status_format() {
        let crate_status = CrateStatus {
            crate_name: "claudefs-storage".to_string(),
            test_count: 1301,
            status: "✅".to_string(),
        };
        let formatted = format!(
            "- {} ({}): {} tests {}",
            crate_status.crate_name,
            crate_status.crate_name.replace("claudefs-", "A"),
            crate_status.test_count,
            crate_status.status
        );
        assert!(formatted.contains("A-storage"));
        assert!(formatted.contains("1301 tests"));
    }

    #[test]
    fn test_entry_builder_pattern() {
        let entry = ChangelogEntry::new("2026-03-08".to_string())
            .with_stats(TestStats::new())
            .with_resolved(vec!["fix1".to_string(), "fix2".to_string()])
            .with_flaky_alerts(vec!["test1 is flaky".to_string()]);

        assert_eq!(entry.date, "2026-03-08");
        assert_eq!(entry.issues_resolved.len(), 2);
        assert_eq!(entry.flaky_alerts.len(), 1);
    }
}
