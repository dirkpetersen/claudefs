//! Fuzzing Infrastructure
//!
//! Utilities for structure-aware fuzzing of ClaudeFS components.

use rand::Rng;
use rand::SeedableRng;

pub struct StructuredFuzzer {
    rng: rand::rngs::SmallRng,
    pub seed: u64,
}

impl StructuredFuzzer {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: rand::rngs::SmallRng::seed_from_u64(seed),
            seed,
        }
    }

    pub fn random_bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.rng.gen()).collect()
    }

    pub fn random_string(&mut self, max_len: usize) -> String {
        let len = self.rng.gen_range(0..=max_len);
        let bytes: Vec<u8> = (0..len).map(|_| self.rng.gen_range(b' '..=b'z')).collect();
        String::from_utf8_lossy(&bytes).to_string()
    }

    pub fn random_path(&mut self, max_depth: usize) -> std::path::PathBuf {
        let depth = self.rng.gen_range(1..=max_depth.max(1));
        let mut components = Vec::new();

        for _ in 0..depth {
            let name_len = self.rng.gen_range(1..20);
            let name: String = (0..name_len)
                .map(|_| {
                    let c = self.rng.gen::<u8>();
                    if c.is_ascii_alphanumeric() {
                        c as char
                    } else {
                        'a'
                    }
                })
                .collect();
            components.push(name);
        }

        std::path::PathBuf::from("/").join(std::path::PathBuf::from_iter(components))
    }

    pub fn random_filename(&mut self) -> String {
        let len = self.rng.gen_range(1..50);
        (0..len)
            .map(|_| {
                let c = self.rng.gen::<u8>();
                if c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-' {
                    c as char
                } else {
                    'a'
                }
            })
            .collect()
    }

    pub fn random_u64(&mut self) -> u64 {
        self.rng.gen()
    }

    pub fn random_f64_0_1(&mut self) -> f64 {
        self.rng.gen_range(0.0..1.0)
    }

    pub fn random_bytes_range(&mut self, min: usize, max: usize) -> Vec<u8> {
        let len = self.rng.gen_range(min..=max.max(min));
        self.random_bytes(len)
    }
}

pub struct RpcFuzzer {
    fuzzer: StructuredFuzzer,
}

impl RpcFuzzer {
    pub fn new(seed: u64) -> Self {
        Self {
            fuzzer: StructuredFuzzer::new(seed),
        }
    }

    pub fn empty_frame(&self) -> Vec<u8> {
        vec![0u8; 24]
    }

    pub fn truncated_frame(&mut self) -> Vec<u8> {
        let len = self.fuzzer.rng.gen_range(1..24);
        self.fuzzer.random_bytes(len)
    }

    pub fn oversized_frame(&mut self, max_size: usize) -> Vec<u8> {
        let size = max_size + self.fuzzer.rng.gen_range(1..1000);
        self.fuzzer.random_bytes(size)
    }

    pub fn random_frame(&mut self) -> Vec<u8> {
        let size = self.fuzzer.rng.gen_range(24..10000);
        self.fuzzer.random_bytes(size)
    }

    pub fn malformed_header(&mut self) -> Vec<u8> {
        let mut header = vec![0u8; 24];

        header[0] = 0x12;
        header[1] = 0x34;
        header[2] = 0x56;
        header[3] = 0x78;

        header
    }
}

pub struct PathFuzzer {
    fuzzer: StructuredFuzzer,
}

impl PathFuzzer {
    pub fn new(seed: u64) -> Self {
        Self {
            fuzzer: StructuredFuzzer::new(seed),
        }
    }

    pub fn absolute_path(&mut self) -> String {
        let mut path = String::from("/");
        let components = self.fuzzer.rng.gen_range(1..5);

        for i in 0..components {
            if i > 0 {
                path.push('/');
            }
            path.push_str(&self.fuzzer.random_filename());
        }

        path
    }

    pub fn path_with_dots(&mut self) -> String {
        let base = self.absolute_path();
        let suffix = match self.fuzzer.rng.gen_range(0..3) {
            0 => "/../file",
            1 => "/./file",
            2 => "/dir/../other",
            _ => "/file",
        };
        format!("{}{}", base, suffix)
    }

    pub fn path_with_spaces(&mut self) -> String {
        let mut path = String::from("/");
        path.push_str("dir with spaces");
        path.push('/');
        path.push_str("file with spaces.txt");
        path
    }

    pub fn very_long_path(&mut self, components: usize) -> String {
        let mut path = String::from("/");
        for i in 0..components {
            if i > 0 {
                path.push('/');
            }
            let name: String = (0..100)
                .map(|_| {
                    let c = self.fuzzer.rng.gen::<u8>();
                    if c.is_ascii_alphanumeric() {
                        c as char
                    } else {
                        'a'
                    }
                })
                .collect();
            path.push_str(&name);
        }
        path
    }

    pub fn path_with_unicode(&mut self) -> String {
        let idx = self.fuzzer.rng.gen_range(0..4);
        let unicode_str = match idx {
            0 => "αβγδ",
            1 => "日本語",
            2 => "中文",
            _ => "dir",
        };
        format!("/dir/{}/file", unicode_str)
    }

    pub fn null_byte_path(&mut self) -> Vec<u8> {
        let mut path = self.absolute_path();
        path.push('\0');
        path.into_bytes()
    }
}

#[derive(Debug, Clone)]
pub struct FuzzEntry {
    pub id: String,
    pub data: Vec<u8>,
    pub description: String,
    pub triggers_bug: bool,
}

pub struct FuzzCorpus {
    entries: Vec<FuzzEntry>,
}

impl FuzzCorpus {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: FuzzEntry) {
        self.entries.push(entry);
    }

    pub fn seed_corpus() -> Self {
        let mut corpus = Self::new();

        corpus.add(FuzzEntry {
            id: "empty".to_string(),
            data: vec![],
            description: "Empty input".to_string(),
            triggers_bug: false,
        });

        corpus.add(FuzzEntry {
            id: "truncated".to_string(),
            data: vec![0u8; 10],
            description: "Truncated frame".to_string(),
            triggers_bug: false,
        });

        corpus.add(FuzzEntry {
            id: "oversized".to_string(),
            data: vec![0u8; 10_000_000],
            description: "Oversized payload".to_string(),
            triggers_bug: true,
        });

        corpus.add(FuzzEntry {
            id: "null_bytes".to_string(),
            data: vec![0u8; 100],
            description: "Contains null bytes".to_string(),
            triggers_bug: true,
        });

        corpus.add(FuzzEntry {
            id: "invalid_utf8".to_string(),
            data: vec![0xFF, 0xFE, 0xFD],
            description: "Invalid UTF-8 sequence".to_string(),
            triggers_bug: false,
        });

        corpus
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn interesting_entries(&self) -> Vec<&FuzzEntry> {
        self.entries.iter().filter(|e| !e.triggers_bug).collect()
    }

    pub fn bug_entries(&self) -> Vec<&FuzzEntry> {
        self.entries.iter().filter(|e| e.triggers_bug).collect()
    }

    pub fn get_by_id(&self, id: &str) -> Option<&FuzzEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

impl Default for FuzzCorpus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod structured_fuzzer_tests {
    use super::*;

    #[test]
    fn test_seed_produces_deterministic_output() {
        let mut fuzzer1 = StructuredFuzzer::new(12345);
        let mut fuzzer2 = StructuredFuzzer::new(12345);

        let output1 = fuzzer1.random_bytes(10);
        let output2 = fuzzer2.random_bytes(10);

        assert_eq!(output1, output2);
    }

    #[test]
    fn test_different_seeds_produce_different_output() {
        let mut fuzzer1 = StructuredFuzzer::new(12345);
        let mut fuzzer2 = StructuredFuzzer::new(67890);

        let output1 = fuzzer1.random_bytes(10);
        let output2 = fuzzer2.random_bytes(10);

        assert_ne!(output1, output2);
    }

    #[test]
    fn test_random_bytes_length_matches() {
        let mut fuzzer = StructuredFuzzer::new(42);
        let len = 100;
        let output = fuzzer.random_bytes(len);
        assert_eq!(output.len(), len);
    }

    #[test]
    fn test_random_string_is_valid_utf8() {
        let mut fuzzer = StructuredFuzzer::new(42);
        let s = fuzzer.random_string(100);
        assert!(std::str::from_utf8(s.as_bytes()).is_ok());
    }

    #[test]
    fn test_random_filename_contains_no_slash() {
        let mut fuzzer = StructuredFuzzer::new(42);
        for _ in 0..100 {
            let name = fuzzer.random_filename();
            assert!(!name.contains('/'));
            assert!(!name.contains('\0'));
        }
    }

    #[test]
    fn test_random_path_depth_matches_max_depth() {
        let mut fuzzer = StructuredFuzzer::new(42);
        let path = fuzzer.random_path(5);
        let components = path.iter().count();
        assert!(components <= 5);
    }

    #[test]
    fn test_random_u64() {
        let mut fuzzer = StructuredFuzzer::new(42);
        let val = fuzzer.random_u64();
        assert!(val >= u64::MIN);
        assert!(val <= u64::MAX);
    }

    #[test]
    fn test_random_f64_0_1_range() {
        let mut fuzzer = StructuredFuzzer::new(42);
        for _ in 0..100 {
            let val = fuzzer.random_f64_0_1();
            assert!(val >= 0.0 && val < 1.0);
        }
    }

    #[test]
    fn test_random_bytes_range() {
        let mut fuzzer = StructuredFuzzer::new(42);
        let output = fuzzer.random_bytes_range(10, 100);
        assert!(output.len() >= 10 && output.len() <= 100);
    }

    #[test]
    fn test_different_seeds_different_paths() {
        let mut fuzzer1 = StructuredFuzzer::new(1);
        let mut fuzzer2 = StructuredFuzzer::new(2);
        let path1 = fuzzer1.random_path(3);
        let path2 = fuzzer2.random_path(3);
        assert_ne!(path1, path2);
    }
}

#[cfg(test)]
mod rpc_fuzzer_tests {
    use super::*;

    #[test]
    fn test_empty_frame() {
        let fuzzer = RpcFuzzer::new(42);
        let frame = fuzzer.empty_frame();
        assert_eq!(frame.len(), 24);
    }

    #[test]
    fn test_truncated_frame() {
        let mut fuzzer = RpcFuzzer::new(42);
        let frame = fuzzer.truncated_frame();
        assert!(frame.len() < 24);
    }

    #[test]
    fn test_oversized_frame() {
        let mut fuzzer = RpcFuzzer::new(42);
        let max_size = 1000;
        let frame = fuzzer.oversized_frame(max_size);
        assert!(frame.len() > max_size);
    }

    #[test]
    fn test_random_frame() {
        let mut fuzzer = RpcFuzzer::new(42);
        let frame = fuzzer.random_frame();
        assert!(frame.len() >= 24);
    }

    #[test]
    fn test_malformed_header() {
        let mut fuzzer = RpcFuzzer::new(42);
        let header = fuzzer.malformed_header();
        assert_eq!(header.len(), 24);
        assert!(header[0] != 0xCF || header[1] != 0x5F);
    }

    #[test]
    fn test_deterministic_rpc_fuzzer() {
        let mut fuzzer1 = RpcFuzzer::new(100);
        let mut fuzzer2 = RpcFuzzer::new(100);

        let frame1 = fuzzer1.truncated_frame();
        let frame2 = fuzzer2.truncated_frame();
        assert_eq!(frame1.len(), frame2.len());
    }
}

#[cfg(test)]
mod path_fuzzer_tests {
    use super::*;

    #[test]
    fn test_absolute_path_starts_with_slash() {
        let mut fuzzer = PathFuzzer::new(42);
        for _ in 0..100 {
            let path = fuzzer.absolute_path();
            assert!(path.starts_with('/'));
        }
    }

    #[test]
    fn test_path_with_dots_contains_dots() {
        let mut fuzzer = PathFuzzer::new(42);
        let path = fuzzer.path_with_dots();
        assert!(path.contains('.') || path.contains(".."));
    }

    #[test]
    fn test_path_with_spaces_contains_spaces() {
        let mut fuzzer = PathFuzzer::new(42);
        let path = fuzzer.path_with_spaces();
        assert!(path.contains(' '));
    }

    #[test]
    fn test_very_long_path() {
        let mut fuzzer = PathFuzzer::new(42);
        let path = fuzzer.very_long_path(10);
        assert!(path.len() > 1000);
    }

    #[test]
    fn test_path_with_unicode() {
        let mut fuzzer = PathFuzzer::new(42);
        let path = fuzzer.path_with_unicode();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_null_byte_path() {
        let mut fuzzer = PathFuzzer::new(42);
        let path = fuzzer.null_byte_path();
        assert!(path.contains(&0));
    }

    #[test]
    fn test_deterministic_path_fuzzer() {
        let mut fuzzer1 = PathFuzzer::new(100);
        let mut fuzzer2 = PathFuzzer::new(100);

        let path1 = fuzzer1.absolute_path();
        let path2 = fuzzer2.absolute_path();

        assert_eq!(path1, path2);
    }
}

#[cfg(test)]
mod fuzz_corpus_tests {
    use super::*;

    #[test]
    fn test_corpus_new() {
        let corpus = FuzzCorpus::new();
        assert_eq!(corpus.len(), 0);
    }

    #[test]
    fn test_corpus_add() {
        let mut corpus = FuzzCorpus::new();
        corpus.add(FuzzEntry {
            id: "test".to_string(),
            data: vec![1, 2, 3],
            description: "test".to_string(),
            triggers_bug: false,
        });
        assert_eq!(corpus.len(), 1);
    }

    #[test]
    fn test_seed_corpus_has_entries() {
        let corpus = FuzzCorpus::seed_corpus();
        assert!(corpus.len() > 0);
    }

    #[test]
    fn test_interesting_entries_filters_bug_entries() {
        let corpus = FuzzCorpus::seed_corpus();
        let interesting = corpus.interesting_entries();
        for entry in interesting {
            assert!(!entry.triggers_bug);
        }
    }

    #[test]
    fn test_bug_entries_only_bugs() {
        let corpus = FuzzCorpus::seed_corpus();
        let bugs = corpus.bug_entries();
        for entry in bugs {
            assert!(entry.triggers_bug);
        }
    }

    #[test]
    fn test_get_by_id_finds_entry() {
        let corpus = FuzzCorpus::seed_corpus();
        let entry = corpus.get_by_id("empty");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().id, "empty");
    }

    #[test]
    fn test_get_by_id_returns_none_for_missing() {
        let corpus = FuzzCorpus::seed_corpus();
        let entry = corpus.get_by_id("nonexistent");
        assert!(entry.is_none());
    }

    #[test]
    fn test_two_fuzzers_with_same_seed_produce_same_output() {
        let mut fuzzer1 = StructuredFuzzer::new(42);
        let mut fuzzer2 = StructuredFuzzer::new(42);

        for _ in 0..10 {
            let b1 = fuzzer1.random_bytes(100);
            let b2 = fuzzer2.random_bytes(100);
            assert_eq!(b1, b2);
        }
    }

    #[test]
    fn test_corpus_len() {
        let corpus = FuzzCorpus::seed_corpus();
        assert_eq!(corpus.len(), corpus.entries.len());
    }

    #[test]
    fn test_interesting_entries_not_empty() {
        let corpus = FuzzCorpus::seed_corpus();
        assert!(!corpus.interesting_entries().is_empty());
    }

    #[test]
    fn test_bug_entries_not_empty() {
        let corpus = FuzzCorpus::seed_corpus();
        assert!(!corpus.bug_entries().is_empty());
    }
}
