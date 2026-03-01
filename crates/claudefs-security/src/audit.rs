// FILE: audit.rs
//! Audit finding types for tracking security issues across the codebase.

use serde::{Deserialize, Serialize};

/// Severity level for security findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Informational - no direct security impact
    Info,
    /// Low - minor issue, limited impact
    Low,
    /// Medium - significant issue, exploitable under certain conditions
    Medium,
    /// High - serious vulnerability, exploitable with moderate effort
    High,
    /// Critical - severe vulnerability, easily exploitable
    Critical,
}

/// Category of security finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingCategory {
    /// Unsafe code review findings
    UnsafeCode,
    /// Cryptographic implementation issues
    Crypto,
    /// Protocol parsing vulnerabilities
    Protocol,
    /// Memory safety issues
    Memory,
    /// Authentication/authorization flaws
    AuthN,
    /// Denial of service vectors
    DoS,
    /// Information disclosure
    InfoLeak,
    /// Input validation failures
    InputValidation,
    /// Dependency vulnerabilities
    Dependency,
}

/// A security audit finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Unique identifier (e.g., "FINDING-01")
    pub id: String,
    /// Severity level
    pub severity: Severity,
    /// Category
    pub category: FindingCategory,
    /// Short title
    pub title: String,
    /// Affected file path
    pub location: String,
    /// Detailed description
    pub description: String,
    /// Recommended fix
    pub recommendation: String,
    /// Current status
    pub status: FindingStatus,
}

/// Status of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingStatus {
    /// Newly identified
    Open,
    /// Fix in progress
    InProgress,
    /// Fixed, awaiting re-audit
    Fixed,
    /// Accepted risk (documented)
    Accepted,
}

/// Collection of audit findings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditReport {
    /// All findings
    pub findings: Vec<Finding>,
    /// Auditor identifier
    pub auditor: String,
    /// Date of audit
    pub date: String,
}

impl AuditReport {
    /// Create a new audit report.
    pub fn new(auditor: &str, date: &str) -> Self {
        Self {
            findings: Vec::new(),
            auditor: auditor.to_string(),
            date: date.to_string(),
        }
    }

    /// Add a finding to the report.
    pub fn add_finding(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// Count findings by severity.
    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// Get all open findings.
    pub fn open_findings(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.status == FindingStatus::Open)
            .collect()
    }

    /// Get findings above a severity threshold.
    pub fn findings_above(&self, min_severity: Severity) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity >= min_severity)
            .collect()
    }
}
