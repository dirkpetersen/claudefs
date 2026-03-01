//! Metadata integrity checker (fsck) for ClaudeFS distributed filesystem.

use crate::types::InodeId;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FsckSeverity {
    Error,
    Warning,
    Info,
}

impl FsckSeverity {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsckIssue {
    OrphanInode {
        inode: InodeId,
    },
    LinkCountMismatch {
        inode: InodeId,
        expected: u32,
        actual: u32,
    },
    DanglingEntry {
        parent: InodeId,
        name: String,
        child: InodeId,
    },
    DuplicateEntry {
        parent: InodeId,
        name: String,
        inode1: InodeId,
        inode2: InodeId,
    },
    DisconnectedSubtree {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsckFinding {
    pub severity: FsckSeverity,
    pub issue: FsckIssue,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FsckConfig {
    pub check_orphans: bool,
    pub check_links: bool,
    pub check_dangling: bool,
    pub check_duplicates: bool,
    pub check_connectivity: bool,
    pub repair: bool,
    pub max_errors: usize,
}

impl FsckConfig {
    pub fn default() -> Self {
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FsckReport {
    pub findings: Vec<FsckFinding>,
    pub errors: u64,
    pub warnings: u64,
    pub repaired: u64,
}

impl FsckReport {
    pub fn is_clean(&self) -> bool {
        self.errors == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsckRepairAction {
    RemoveEntry { parent: InodeId, name: String },
    RemoveInode { inode: InodeId },
    UpdateLinkCount { inode: InodeId, nlink: u32 },
}

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
            inode2,
        } => vec![FsckRepairAction::RemoveEntry {
            parent: *parent,
            name: name.clone(),
        }],
        FsckIssue::DisconnectedSubtree { root: _ } => vec![],
    }
}

struct MetadataChecker {
    config: FsckConfig,
    inodes: HashMap<InodeId, (u32, bool)>,
    dir_entries: HashMap<InodeId, Vec<(String, InodeId)>>,
    root: Option<InodeId>,
}

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
            .or_insert_with(Vec::new)
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

        for (parent, entries) in &self.dir_entries {
            for (_, child) in entries {
                *parent_refs.entry(*child).or_insert(0) += 1;
            }
        }

        for (inode, &(nlink, is_dir)) in &self.inodes {
            if report.errors >= self.config.max_errors as u64 {
                return;
            }

            let actual_counts = self
                .dir_entries
                .get(inode)
                .map(|entries| entries.len() as u32)
                .unwrap_or(0);

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
            if report.errors >= self.config.max_errors as u64 {
                return;
            }
            for (name, child) in entries {
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
        config.check_dangling = false;
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
