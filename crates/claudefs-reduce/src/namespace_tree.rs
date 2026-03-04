use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DirId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub id: DirId,
    pub parent: Option<DirId>,
    pub name: String,
    pub child_count: usize,
    pub file_count: usize,
    pub bytes_used: u64,
}

impl DirEntry {
    fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

pub struct NamespaceTree {
    entries: HashMap<DirId, DirEntry>,
}

impl Default for NamespaceTree {
    fn default() -> Self {
        Self::new()
    }
}

impl NamespaceTree {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn add_dir(&mut self, id: DirId, parent: Option<DirId>, name: String) {
        if let Some(parent_id) = parent {
            if let Some(parent_entry) = self.entries.get_mut(&parent_id) {
                parent_entry.child_count += 1;
            }
        }
        self.entries.insert(
            id,
            DirEntry {
                id,
                parent,
                name,
                child_count: 0,
                file_count: 0,
                bytes_used: 0,
            },
        );
    }

    pub fn get(&self, id: DirId) -> Option<&DirEntry> {
        self.entries.get(&id)
    }

    pub fn children(&self, parent: DirId) -> Vec<&DirEntry> {
        self.entries
            .values()
            .filter(|e| e.parent == Some(parent))
            .collect()
    }

    pub fn ancestors(&self, id: DirId) -> Vec<&DirEntry> {
        let mut result = Vec::new();
        let mut current = self.entries.get(&id).and_then(|e| e.parent);
        while let Some(parent_id) = current {
            if let Some(entry) = self.entries.get(&parent_id) {
                result.push(entry);
                current = entry.parent;
            } else {
                break;
            }
        }
        result
    }

    pub fn update_usage(&mut self, id: DirId, bytes_delta: i64) {
        if let Some(entry) = self.entries.get_mut(&id) {
            if bytes_delta >= 0 {
                entry.bytes_used = entry.bytes_used.saturating_add(bytes_delta as u64);
            } else {
                let abs_delta = (-bytes_delta) as u64;
                entry.bytes_used = entry.bytes_used.saturating_sub(abs_delta);
            }
        }
    }

    pub fn record_file(&mut self, dir_id: DirId) {
        let mut current = Some(dir_id);
        while let Some(id) = current {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.file_count += 1;
                current = entry.parent;
            } else {
                break;
            }
        }
    }

    pub fn remove_dir(&mut self, id: DirId) -> bool {
        if let Some(entry) = self.entries.get(&id) {
            if entry.child_count > 0 {
                return false;
            }
            if let Some(parent_id) = entry.parent {
                if let Some(parent_entry) = self.entries.get_mut(&parent_id) {
                    parent_entry.child_count -= 1;
                }
            }
            self.entries.remove(&id);
            true
        } else {
            false
        }
    }

    pub fn dir_count(&self) -> usize {
        self.entries.len()
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries.values().map(|e| e.bytes_used).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_id_equality() {
        let a = DirId(42);
        let b = DirId(42);
        assert_eq!(a, b);
    }

    #[test]
    fn new_tree_empty() {
        let tree = NamespaceTree::new();
        assert_eq!(tree.dir_count(), 0);
    }

    #[test]
    fn add_root_dir() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        assert_eq!(tree.dir_count(), 1);
        let entry = tree.get(DirId(1)).unwrap();
        assert!(entry.is_root());
        assert_eq!(entry.name, "root");
    }

    #[test]
    fn add_child_dir() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child".to_string());
        assert_eq!(tree.dir_count(), 2);
        let parent = tree.get(DirId(1)).unwrap();
        assert_eq!(parent.child_count, 1);
    }

    #[test]
    fn get_dir_found() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        let entry = tree.get(DirId(1));
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "root");
    }

    #[test]
    fn get_dir_not_found() {
        let tree = NamespaceTree::new();
        let entry = tree.get(DirId(999));
        assert!(entry.is_none());
    }

    #[test]
    fn children_of_root() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child1".to_string());
        tree.add_dir(DirId(3), Some(DirId(1)), "child2".to_string());
        let children = tree.children(DirId(1));
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn children_empty() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        let children = tree.children(DirId(1));
        assert!(children.is_empty());
    }

    #[test]
    fn ancestors_of_root() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        let ancestors = tree.ancestors(DirId(1));
        assert!(ancestors.is_empty());
    }

    #[test]
    fn ancestors_of_child() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child".to_string());
        let ancestors = tree.ancestors(DirId(2));
        assert_eq!(ancestors.len(), 1);
        assert_eq!(ancestors[0].id, DirId(1));
    }

    #[test]
    fn ancestors_deep_path() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "level1".to_string());
        tree.add_dir(DirId(3), Some(DirId(2)), "level2".to_string());
        let ancestors = tree.ancestors(DirId(3));
        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0].id, DirId(2));
        assert_eq!(ancestors[1].id, DirId(1));
    }

    #[test]
    fn update_usage_positive() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.update_usage(DirId(1), 100);
        let entry = tree.get(DirId(1)).unwrap();
        assert_eq!(entry.bytes_used, 100);
    }

    #[test]
    fn update_usage_negative_clamped() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.update_usage(DirId(1), 50);
        tree.update_usage(DirId(1), -100);
        let entry = tree.get(DirId(1)).unwrap();
        assert_eq!(entry.bytes_used, 0);
    }

    #[test]
    fn record_file_increments_file_count() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child".to_string());
        tree.record_file(DirId(2));
        let child = tree.get(DirId(2)).unwrap();
        let parent = tree.get(DirId(1)).unwrap();
        assert_eq!(child.file_count, 1);
        assert_eq!(parent.file_count, 1);
    }

    #[test]
    fn remove_dir_no_children() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        let result = tree.remove_dir(DirId(1));
        assert!(result);
        assert_eq!(tree.dir_count(), 0);
    }

    #[test]
    fn remove_dir_has_children_fails() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child".to_string());
        let result = tree.remove_dir(DirId(1));
        assert!(!result);
        assert_eq!(tree.dir_count(), 2);
    }

    #[test]
    fn dir_count() {
        let mut tree = NamespaceTree::new();
        tree.add_dir(DirId(1), None, "root".to_string());
        tree.add_dir(DirId(2), Some(DirId(1)), "child".to_string());
        tree.add_dir(DirId(3), Some(DirId(1)), "child2".to_string());
        assert_eq!(tree.dir_count(), 3);
        tree.remove_dir(DirId(2));
        assert_eq!(tree.dir_count(), 2);
    }
}
