//! Metadata integrity checker (fsck) for ClaudeFS distributed filesystem.

use crate::types::InodeId;
#[cfg(test)]
use std::collections::{HashMap, HashSet, VecDeque};

/// Severity level for a filesystem check finding.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FsckSeverity {
    /// Error severity — indicates data corruption.
    Error,
    /// Warning severity — indicates potential issue.
    Warning,
    /// Info severity — informational note.
    Info,
}

impl FsckSeverity {
    /// Returns true if this severity is Error.
    pub fn is_error(&self) -> bool {
        matches!(self, FsckSeverity::Error)
    }
}

impl std::fmt::Display for FsckSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsckSeverity::Error => write!(f, "ERROR"),
            FsckSeverity::Warning => write!(f, "WARNING"),
            FsckSeverity::Info => write!(f, "INFO"),
        }
    }
}

/// Types of filesystem integrity issues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsckIssue {
    /// An inode with no directory entry pointing to it.
    OrphanInode {
        /// The orphaned inode.
        inode: InodeId,
    },
    /// Inode link count does not match actual references.
    LinkCountMismatch {
        /// The inode with incorrect link count.
        inode: InodeId,
        /// The correct link count.
        expected: u32,
        /// The stored link count.
        actual: u32,
    },
    /// Directory entry points to non-existent inode.
    DanglingEntry {
        /// Parent directory inode.
        parent: InodeId,
        /// Entry name.
        name: String,
        /// Target inode that does not exist.
        child: InodeId,
    },
    /// Duplicate entry names in same directory.
    DuplicateEntry {
        /// Parent directory inode.
        parent: InodeId,
        /// The duplicate name.
        name: String,
        /// First inode with this name.
        inode1: InodeId,
        /// Second inode with this name.
        inode2: InodeId,
    },
    /// Subtree not connected to root.
    DisconnectedSubtree {
        /// Root of the disconnected subtree.
        root: InodeId,
    },
}

impl std::fmt::Display for FsckIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsckIssue::OrphanInode { inode } => {
                write!(f, "Orphan inode: {}", inode)
            }
            FsckIssue::LinkCountMismatch {
                inode,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Link count mismatch for inode {}: expected {}, got {}",
                    inode, expected, actual
                )
            }
            FsckIssue::DanglingEntry {
                parent,
                name,
                child,
            } => {
                write!(
                    f,
                    "Dangling entry '{}' in inode {} points to {}",
                    name, parent, child
                )
            }
            FsckIssue::DuplicateEntry {
                parent,
                name,
                inode1,
                inode2,
            } => {
                write!(
                    f,
                    "Duplicate entry '{}' in inode {}: {} and {}",
                    name, parent, inode1, inode2
                )
            }
            FsckIssue::DisconnectedSubtree { root } => {
                write!(f, "Disconnected subtree rooted at {}", root)
            }
        }
    }
}

/// A single finding from the filesystem checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsckFinding {
    /// Severity of this finding.
    pub severity: FsckSeverity,
    /// The detected issue.
    pub issue: FsckIssue,
    /// Whether the issue was repaired.
    pub repaired: bool,
}

impl std::fmt::Display for FsckFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.severity, self.issue)?;
        if self.repaired {
            write!(f, " (repaired)")?;
        }
        Ok(())
    }
}

/// Configuration for filesystem check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsckConfig {
    /// Check for orphan inodes.
    pub check_orphans: bool,
    /// Check link count consistency.
    pub check_links: bool,
    /// Check for dangling directory entries.
    pub check_dangling: bool,
    /// Check for duplicate entries.
    pub check_duplicates: bool,
    /// Check tree connectivity.
    pub check_connectivity: bool,
    /// Automatically repair found issues.
    pub repair: bool,
    /// Maximum errors before stopping.
    pub max_errors: usize,
}

impl Default for FsckConfig {
    fn default() -> Self {
        Self {
            check_orphans: true,
            check_links: true,
            check_dangling: true,
            check_duplicates: true,
            check_connectivity: true,
            repair: false,
            max_errors: 100,
        }
    }
}

/// Report of filesystem check results.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FsckReport {
    /// All findings from the check.
    pub findings: Vec<FsckFinding>,
    /// Count of error-severity findings.
    pub errors: u64,
    /// Count of warning-severity findings.
    pub warnings: u64,
    /// Count of repaired issues.
    pub repaired: u64,
}

impl FsckReport {
    /// Returns true if no errors were found.
    pub fn is_clean(&self) -> bool {
        self.errors == 0
    }
}

/// Action to repair an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsckRepairAction {
    /// Remove a directory entry.
    RemoveEntry {
        /// Parent directory inode.
        parent: InodeId,
        /// Entry name to remove.
        name: String,
    },
    /// Remove an inode entirely.
    RemoveInode {
        /// Inode to remove.
        inode: InodeId,
    },
    /// Update inode link count.
    UpdateLinkCount {
        /// Inode to update.
        inode: InodeId,
        /// New link count.
        nlink: u32,
    },
}

/// Suggests repair actions for an issue.
pub fn suggest_repair(issue: &FsckIssue, repair: bool) -> Vec<FsckRepairAction> {
    if !repair {
        return vec![];
    }
    match issue {
        FsckIssue::OrphanInode { inode } => vec![FsckRepairAction::RemoveInode { inode: *inode }],
        FsckIssue::LinkCountMismatch {
            inode,
            expected,
            actual: _,
        } => vec![FsckRepairAction::UpdateLinkCount {
            inode: *inode,
            nlink: *expected,
        }],
        FsckIssue::DanglingEntry {
            parent,
            name,
            child: _,
        } => vec![FsckRepairAction::RemoveEntry {
            parent: *parent,
            name: name.clone(),
        }],
        FsckIssue::DuplicateEntry {
            parent,
            name,
            inode1: _,
            inode2: _,
        } => vec![FsckRepairAction::RemoveEntry {
            parent: *parent,
            name: name.clone(),
        }],
        FsckIssue::DisconnectedSubtree { root: _ } => vec![],
    }
}

#[cfg(test)]
struct MetadataChecker {
    config: FsckConfig,
    inodes: HashMap<InodeId, (u32, bool)>,
    dir_entries: HashMap<InodeId, Vec<(String, InodeId)>>,
    root: Option<InodeId>,
}

#[cfg(test)]
impl MetadataChecker {
    fn new(config: FsckConfig) -> Self {
        Self {
            config,
            inodes: HashMap::new(),
            dir_entries: HashMap::new(),
            root: None,
        }
    }

    fn add_inode(&mut self, inode: InodeId, nlink: u32, is_dir: bool) {
        self.inodes.insert(inode, (nlink, is_dir));
    }

    fn add_dir_entry(&mut self, parent: InodeId, name: String, child: InodeId) {
        self.dir_entries
            .entry(parent)
            .or_default()
            .push((name, child));
    }

    fn set_root(&mut self, root: InodeId) {
        self.root = Some(root);
    }

    fn check(&self) -> FsckReport {
        let mut report = FsckReport::default();

        if self.config.check_orphans {
            self.check_orphans(&mut report);
        }
        if self.config.check_links {
            self.check_link_counts(&mut report);
        }
        if self.config.check_dangling {
            self.check_dangling_entries(&mut report);
        }
        if self.config.check_duplicates {
            self.check_duplicate_entries(&mut report);
        }
        if self.config.check_connectivity {
            self.check_tree_connectivity(&mut report);
        }

        report
    }

    fn check_orphans(&self, report: &mut FsckReport) {
        if self.root.is_none() {
            return;
        }
        let root = self.root.unwrap();

        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(root);
        reachable.insert(root);

        while let Some(current) = queue.pop_front() {
            if let Some(entries) = self.dir_entries.get(&current) {
                for (_, child) in entries {
                    if !reachable.contains(child) {
                        reachable.insert(*child);
                        queue.push_back(*child);
                    }
                }
            }
        }

        for inode in self.inodes.keys() {
            if report.errors >= self.config.max_errors as u64 {
                return;
            }
            if !reachable.contains(inode) {
                report.errors += 1;
                let repaired = self.config.repair;
                report.findings.push(FsckFinding {
                    severity: FsckSeverity::Error,
                    issue: FsckIssue::OrphanInode { inode: *inode },
                    repaired,
                });
                if repaired {
                    report.repaired += 1;
                }
            }
        }
    }

    fn check_link_counts(&self, report: &mut FsckReport) {
        let mut parent_refs: HashMap<InodeId, u32> = HashMap::new();

        for (_parent, entries) in &self.dir_entries {
            for (_, child) in entries {
                *parent_refs.entry(*child).or_insert(0) += 1;
            }
        }

        for (inode, &(nlink, is_dir)) in &self.inodes {
            if report.errors >= self.config.max_errors as u64 {
                return;
            }

            let actual = if is_dir {
                if *inode == InodeId::ROOT_INODE {
                    let child_dir_count = self
                        .dir_entries
                        .get(inode)
                        .map(|entries| {
                            entries
                                .iter()
                                .filter(|(_, child)| {
                                    self.inodes.get(child).map(|(_, d)| *d).unwrap_or(false)
                                })
                                .count() as u32
                        })
                        .unwrap_or(0);
                    2 + child_dir_count
                } else {
                    let parent_ref = parent_refs.get(inode).copied().unwrap_or(0);
                    let child_dir_count = self
                        .dir_entries
                        .get(inode)
                        .map(|entries| {
                            entries
                                .iter()
                                .filter(|(_, child)| {
                                    self.inodes.get(child).map(|(_, d)| *d).unwrap_or(false)
                                })
                                .count() as u32
                        })
                        .unwrap_or(0);
                    parent_ref + 1 + child_dir_count
                }
            } else {
                parent_refs.get(inode).copied().unwrap_or(0)
            };

            if actual != nlink {
                report.errors += 1;
                let repaired = self.config.repair;
                report.findings.push(FsckFinding {
                    severity: FsckSeverity::Error,
                    issue: FsckIssue::LinkCountMismatch {
                        inode: *inode,
                        expected: actual,
                        actual: nlink,
                    },
                    repaired,
                });
                if repaired {
                    report.repaired += 1;
                }
            }
        }
    }

    fn check_dangling_entries(&self, report: &mut FsckReport) {
        for (parent, entries) in &self.dir_entries {
            for (name, child) in entries {
                if report.errors >= self.config.max_errors as u64 {
                    return;
                }
                if !self.inodes.contains_key(child) {
                    report.errors += 1;
                    let repaired = self.config.repair;
                    report.findings.push(FsckFinding {
                        severity: FsckSeverity::Error,
                        issue: FsckIssue::DanglingEntry {
                            parent: *parent,
                            name: name.clone(),
                            child: *child,
                        },
                        repaired,
                    });
                    if repaired {
                        report.repaired += 1;
                    }
                }
            }
        }
    }

    fn check_duplicate_entries(&self, report: &mut FsckReport) {
        for (parent, entries) in &self.dir_entries {
            if report.errors >= self.config.max_errors as u64 {
                return;
            }
            let mut seen: HashMap<String, InodeId> = HashMap::new();
            for (name, child) in entries {
                if let Some(first) = seen.get(name) {
                    if *first != *child {
                        report.errors += 1;
                        let repaired = self.config.repair;
                        report.findings.push(FsckFinding {
                            severity: FsckSeverity::Error,
                            issue: FsckIssue::DuplicateEntry {
                                parent: *parent,
                                name: name.clone(),
                                inode1: *first,
                                inode2: *child,
                            },
                            repaired,
                        });
                        if repaired {
                            report.repaired += 1;
                        }
                    }
                } else {
                    seen.insert(name.clone(), *child);
                }
            }
        }
    }

    fn check_tree_connectivity(&self, report: &mut FsckReport) {
        if self.root.is_none() {
            return;
        }
        let root = self.root.unwrap();

        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(root);
        reachable.insert(root);

        while let Some(current) = queue.pop_front() {
            if let Some(entries) = self.dir_entries.get(&current) {
                for (_, child) in entries {
                    if !reachable.contains(child) {
                        reachable.insert(*child);
                        queue.push_back(*child);
                    }
                }
            }
        }

        for inode in self.inodes.keys() {
            if report.errors >= self.config.max_errors as u64 {
                return;
            }
            if !reachable.contains(inode) {
                report.errors += 1;
                let repaired = self.config.repair;
                report.findings.push(FsckFinding {
                    severity: FsckSeverity::Error,
                    issue: FsckIssue::DisconnectedSubtree { root: *inode },
                    repaired,
                });
                if repaired {
                    report.repaired += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checker_clean() {
        let config = FsckConfig::default();
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_inode(InodeId::new(2), 1, false);
        checker.add_dir_entry(InodeId::ROOT_INODE, "file".to_string(), InodeId::new(2));

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(report.is_clean());
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn test_checker_orphan() {
        let mut config = FsckConfig::default();
        config.check_links = false;
        config.check_dangling = false;
        config.check_duplicates = false;
        config.check_connectivity = false;
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 1, true);
        checker.add_inode(InodeId::new(100), 1, false);

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| matches!(
            &f.issue,
            FsckIssue::OrphanInode { inode } if inode.as_u64() == 100
        )));
    }

    #[test]
    fn test_checker_link_count_mismatch() {
        let config = FsckConfig::default();
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 4, true);
        checker.add_inode(InodeId::new(2), 1, false);
        checker.add_inode(InodeId::new(3), 1, true);
        checker.add_dir_entry(InodeId::ROOT_INODE, "file".to_string(), InodeId::new(2));
        checker.add_dir_entry(InodeId::ROOT_INODE, "subdir".to_string(), InodeId::new(3));

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| matches!(
            &f.issue,
            FsckIssue::LinkCountMismatch { inode, .. } if *inode == InodeId::ROOT_INODE
        )));
    }

    #[test]
    fn test_checker_link_count_correct() {
        let config = FsckConfig::default();
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_inode(InodeId::new(2), 1, false);
        checker.add_dir_entry(InodeId::ROOT_INODE, "file".to_string(), InodeId::new(2));

        let mut corrected_config = checker.inodes.clone();
        *corrected_config.get_mut(&InodeId::ROOT_INODE).unwrap() = (2, true);

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(report.is_clean());
    }

    #[test]
    fn test_checker_dangling_entry() {
        let mut config = FsckConfig::default();
        config.check_orphans = false;
        config.check_links = false;
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 1, true);
        checker.add_dir_entry(
            InodeId::ROOT_INODE,
            "missing".to_string(),
            InodeId::new(999),
        );

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| matches!(
            &f.issue,
            FsckIssue::DanglingEntry { parent, name, child }
                if *parent == InodeId::ROOT_INODE && name == "missing" && child.as_u64() == 999
        )));
    }

    #[test]
    fn test_checker_duplicate_entry() {
        let config = FsckConfig::default();
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_inode(InodeId::new(5), 1, false);
        checker.add_inode(InodeId::new(6), 1, false);
        checker.add_dir_entry(InodeId::ROOT_INODE, "dup".to_string(), InodeId::new(5));
        checker.add_dir_entry(InodeId::ROOT_INODE, "dup".to_string(), InodeId::new(6));

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| matches!(
            &f.issue,
            FsckIssue::DuplicateEntry { parent, name, .. }
                if *parent == InodeId::ROOT_INODE && name == "dup"
        )));
    }

    #[test]
    fn test_checker_disconnected_subtree() {
        let mut config = FsckConfig::default();
        config.check_orphans = false;
        config.check_links = false;
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_inode(InodeId::new(100), 2, true);

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| matches!(
            &f.issue,
            FsckIssue::DisconnectedSubtree { root } if root.as_u64() == 100
        )));
    }

    #[test]
    fn test_checker_repair_mode() {
        let mut config = FsckConfig::default();
        config.repair = true;
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_inode(InodeId::new(100), 1, false);

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert!(report.findings.iter().all(|f| f.repaired));
    }

    #[test]
    fn test_checker_max_errors() {
        let mut config = FsckConfig::default();
        config.max_errors = 2;
        config.check_orphans = false;
        config.check_links = false;
        config.check_dangling = true;
        config.check_duplicates = false;
        config.check_connectivity = false;
        let mut checker = MetadataChecker::new(config);

        checker.add_inode(InodeId::ROOT_INODE, 2, true);
        checker.add_dir_entry(
            InodeId::ROOT_INODE,
            "dangling1".to_string(),
            InodeId::new(900),
        );
        checker.add_dir_entry(
            InodeId::ROOT_INODE,
            "dangling2".to_string(),
            InodeId::new(901),
        );
        checker.add_dir_entry(
            InodeId::ROOT_INODE,
            "dangling3".to_string(),
            InodeId::new(902),
        );

        checker.set_root(InodeId::ROOT_INODE);

        let report = checker.check();
        assert!(!report.is_clean());
        assert_eq!(report.errors, 2);
    }

    #[test]
    fn test_suggest_repair_no_repair() {
        let issue = FsckIssue::OrphanInode {
            inode: InodeId::new(100),
        };
        let actions = suggest_repair(&issue, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_suggest_repair_with_repair() {
        let issue = FsckIssue::OrphanInode {
            inode: InodeId::new(100),
        };
        let actions = suggest_repair(&issue, true);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            FsckRepairAction::RemoveInode { inode } if inode.as_u64() == 100
        ));
    }
}
