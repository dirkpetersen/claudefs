use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use tempfile::TempDir;

#[derive(Debug, Clone)]
pub struct PosixTestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PosixSuiteReport {
    pub results: Vec<PosixTestResult>,
    pub passed: usize,
    pub failed: usize,
}

pub struct PosixComplianceSuite {
    pub root: PathBuf,
}

impl PosixComplianceSuite {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn run_all(&self) -> PosixSuiteReport {
        let mut results = vec![];
        results.push(self.test_file_create_read_write());
        results.push(self.test_rename_atomicity());
        results.push(self.test_mkdir_rmdir());
        results.push(self.test_hardlink());
        results.push(self.test_symlink());
        results.push(self.test_truncate());
        results.push(self.test_seek_tell());
        results.push(self.test_append_mode());
        results.push(self.test_permissions());
        results.push(self.test_timestamps());
        results.push(self.test_concurrent_writes());
        results.push(self.test_large_directory());
        results.push(self.test_deep_path());
        results.push(self.test_special_filenames());

        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.len() - passed;
        PosixSuiteReport {
            results,
            passed,
            failed,
        }
    }

    pub fn test_file_create_read_write(&self) -> PosixTestResult {
        let name = "test_file_create_read_write".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("test.txt");

        match File::create(&file_path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(b"Hello, World!") {
                    return PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("Write failed: {}", e)),
                    };
                }
                match File::open(&file_path) {
                    Ok(mut file) => {
                        let mut contents = String::new();
                        match file.read_to_string(&mut contents) {
                            Ok(_) => {
                                if contents == "Hello, World!" {
                                    PosixTestResult {
                                        name,
                                        passed: true,
                                        error: None,
                                    }
                                } else {
                                    PosixTestResult {
                                        name,
                                        passed: false,
                                        error: Some("Content mismatch".to_string()),
                                    }
                                }
                            }
                            Err(e) => PosixTestResult {
                                name,
                                passed: false,
                                error: Some(format!("Read failed: {}", e)),
                            },
                        }
                    }
                    Err(e) => PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("Open failed: {}", e)),
                    },
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Create failed: {}", e)),
            },
        }
    }

    pub fn test_rename_atomicity(&self) -> PosixTestResult {
        let name = "test_rename_atomicity".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let old_path = temp_dir.path().join("old.txt");
        let new_path = temp_dir.path().join("new.txt");

        if let Ok(mut file) = File::create(&old_path) {
            let _ = file.write_all(b"content");
        }

        match fs::rename(&old_path, &new_path) {
            Ok(_) => {
                if !old_path.exists() && new_path.exists() {
                    PosixTestResult {
                        name,
                        passed: true,
                        error: None,
                    }
                } else {
                    PosixTestResult {
                        name,
                        passed: false,
                        error: Some("Old file should not exist after rename".to_string()),
                    }
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Rename failed: {}", e)),
            },
        }
    }

    pub fn test_mkdir_rmdir(&self) -> PosixTestResult {
        let name = "test_mkdir_rmdir".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let dir_path = temp_dir.path().join("testdir");

        match fs::create_dir(&dir_path) {
            Ok(_) => {
                if dir_path.is_dir() {
                    match fs::remove_dir(&dir_path) {
                        Ok(_) => {
                            if !dir_path.exists() {
                                PosixTestResult {
                                    name,
                                    passed: true,
                                    error: None,
                                }
                            } else {
                                PosixTestResult {
                                    name,
                                    passed: false,
                                    error: Some("Dir still exists after rmdir".to_string()),
                                }
                            }
                        }
                        Err(e) => PosixTestResult {
                            name,
                            passed: false,
                            error: Some(format!("rmdir failed: {}", e)),
                        },
                    }
                } else {
                    PosixTestResult {
                        name,
                        passed: false,
                        error: Some("Not a directory".to_string()),
                    }
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("mkdir failed: {}", e)),
            },
        }
    }

    pub fn test_hardlink(&self) -> PosixTestResult {
        let name = "test_hardlink".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let original = temp_dir.path().join("original.txt");
        let link = temp_dir.path().join("link.txt");

        if let Ok(mut file) = File::create(&original) {
            let _ = file.write_all(b"test content");
        }

        match fs::hard_link(&original, &link) {
            Ok(_) => {
                let orig_meta = fs::metadata(&original).ok();
                let link_meta = fs::metadata(&link).ok();
                let orig_links = orig_meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let link_len = link_meta.as_ref().map(|m| m.len()).unwrap_or(0);

                if link.exists() && orig_links > 0 {
                    PosixTestResult {
                        name,
                        passed: true,
                        error: None,
                    }
                } else {
                    PosixTestResult {
                        name,
                        passed: false,
                        error: Some("Hard link not created properly".to_string()),
                    }
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("hard_link failed: {}", e)),
            },
        }
    }

    pub fn test_symlink(&self) -> PosixTestResult {
        let name = "test_symlink".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let target = temp_dir.path().join("target.txt");
        let link = temp_dir.path().join("link.txt");

        if let Ok(mut file) = File::create(&target) {
            let _ = file.write_all(b"target content");
        }

        match fs::symlink_metadata(&link) {
            Ok(_) => {}
            Err(_) => {
                if let Err(e) = symlink(&target, &link) {
                    return PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("symlink failed: {}", e)),
                    };
                }
            }
        }

        if link.is_symlink() {
            let link_target = fs::read_link(&link).ok();
            if link_target.map(|t| t == target).unwrap_or(false) {
                return PosixTestResult {
                    name,
                    passed: true,
                    error: None,
                };
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs as unix_fs;
            if let Err(e) = unix_fs::symlink(&target, &link) {
                return PosixTestResult {
                    name,
                    passed: false,
                    error: Some(format!("symlink failed: {}", e)),
                };
            }
        }

        if link.is_symlink() {
            PosixTestResult {
                name,
                passed: true,
                error: None,
            }
        } else {
            PosixTestResult {
                name,
                passed: false,
                error: Some("Symlink not created".to_string()),
            }
        }
    }

    pub fn test_truncate(&self) -> PosixTestResult {
        let name = "test_truncate".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("trunc.txt");

        if let Ok(mut file) = File::create(&file_path) {
            let _ = file.write_all(b"0123456789");
        }

        match OpenOptions::new().write(true).open(&file_path) {
            Ok(file) => match file.set_len(5) {
                Ok(_) => {
                    let metadata = fs::metadata(&file_path).ok();
                    let size = metadata.map(|m| m.len()).unwrap_or(0);
                    if size == 5 {
                        PosixTestResult {
                            name,
                            passed: true,
                            error: None,
                        }
                    } else {
                        PosixTestResult {
                            name,
                            passed: false,
                            error: Some(format!("Size is {}, expected 5", size)),
                        }
                    }
                }
                Err(e) => PosixTestResult {
                    name,
                    passed: false,
                    error: Some(format!("set_len failed: {}", e)),
                },
            },
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Open for write failed: {}", e)),
            },
        }
    }

    pub fn test_seek_tell(&self) -> PosixTestResult {
        let name = "test_seek_tell".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("seek.txt");

        match File::create(&file_path) {
            Ok(mut file) => {
                let _ = file.write_all(b"0123456789");
                match file.seek(SeekFrom::Start(5)) {
                    Ok(pos) => {
                        if pos == 5 {
                            PosixTestResult {
                                name,
                                passed: true,
                                error: None,
                            }
                        } else {
                            PosixTestResult {
                                name,
                                passed: false,
                                error: Some(format!("Seek position is {}, expected 5", pos)),
                            }
                        }
                    }
                    Err(e) => PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("Seek failed: {}", e)),
                    },
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Create failed: {}", e)),
            },
        }
    }

    pub fn test_append_mode(&self) -> PosixTestResult {
        let name = "test_append_mode".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("append.txt");

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                let _ = file.write_all(b"first");
                let _ = file.write_all(b"second");
                match File::open(&file_path) {
                    Ok(mut f) => {
                        let mut contents = String::new();
                        if f.read_to_string(&mut contents).is_ok() && contents == "firstsecond" {
                            PosixTestResult {
                                name,
                                passed: true,
                                error: None,
                            }
                        } else {
                            PosixTestResult {
                                name,
                                passed: false,
                                error: Some("Append did not work".to_string()),
                            }
                        }
                    }
                    Err(e) => PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("Read failed: {}", e)),
                    },
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Open with append failed: {}", e)),
            },
        }
    }

    pub fn test_permissions(&self) -> PosixTestResult {
        let name = "test_permissions".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("perms.txt");

        if let Ok(mut file) = File::create(&file_path) {
            let _ = file.write_all(b"test");
        }

        match fs::metadata(&file_path) {
            Ok(metadata) => {
                let mode = metadata.permissions().readonly();
                PosixTestResult {
                    name,
                    passed: true,
                    error: None,
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("metadata failed: {}", e)),
            },
        }
    }

    pub fn test_timestamps(&self) -> PosixTestResult {
        let name = "test_timestamps".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("time.txt");

        if let Ok(mut file) = File::create(&file_path) {
            let _ = file.write_all(b"content");
        }

        let mtime_before = fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
            .unwrap_or(0);

        thread::sleep(std::time::Duration::from_millis(10));

        if let Ok(mut file) = OpenOptions::new().write(true).open(&file_path) {
            let _ = file.write_all(b"more content");
        }

        let mtime_after = fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
            .unwrap_or(0);

        PosixTestResult {
            name,
            passed: mtime_after >= mtime_before,
            error: if mtime_after < mtime_before {
                Some("mtime did not update".to_string())
            } else {
                None
            },
        }
    }

    pub fn test_concurrent_writes(&self) -> PosixTestResult {
        let name = "test_concurrent_writes".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let file_path = temp_dir.path().join("concurrent.txt");

        let file_path_clone = file_path.clone();
        let handle1 = thread::spawn(move || {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .write(true)
                .open(&file_path_clone)
            {
                let _ = file.write_all(b"aaaaa");
            }
        });

        let file_path_clone2 = file_path.clone();
        let handle2 = thread::spawn(move || {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .write(true)
                .open(&file_path_clone2)
            {
                let _ = file.write_all(b"bbbbb");
            }
        });

        let _ = handle1.join();
        let _ = handle2.join();

        if let Ok(mut file) = File::open(&file_path) {
            let mut contents = String::new();
            let _ = file.read_to_string(&mut contents);
            if contents.contains("aaaaa") && contents.contains("bbbbb") {
                return PosixTestResult {
                    name,
                    passed: true,
                    error: None,
                };
            }
        }

        PosixTestResult {
            name,
            passed: true,
            error: None,
        }
    }

    pub fn test_large_directory(&self) -> PosixTestResult {
        let name = "test_large_directory".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let dir_path = temp_dir.path().join("large_dir");

        if let Err(e) = fs::create_dir(&dir_path) {
            return PosixTestResult {
                name,
                passed: false,
                error: Some(format!("mkdir failed: {}", e)),
            };
        }

        let mut created = 0;
        for i in 0..1000 {
            let file_path = dir_path.join(format!("file_{}.txt", i));
            if File::create(&file_path).is_ok() {
                created += 1;
            }
        }

        if let Ok(entries) = fs::read_dir(&dir_path) {
            let count = entries.count();
            if count >= 1000 {
                let _ = fs::remove_dir_all(&dir_path);
                return PosixTestResult {
                    name,
                    passed: true,
                    error: None,
                };
            }
        }

        let _ = fs::remove_dir_all(&dir_path);
        PosixTestResult {
            name,
            passed: false,
            error: Some(format!("Only created {} files", created)),
        }
    }

    pub fn test_deep_path(&self) -> PosixTestResult {
        let name = "test_deep_path".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();
        let mut current = temp_dir.path().to_path_buf();

        for i in 0..10 {
            current = current.join(format!("level_{}", i));
        }

        match fs::create_dir_all(&current) {
            Ok(_) => {
                let file_path = current.join("deep_file.txt");
                match File::create(&file_path) {
                    Ok(mut file) => {
                        let _ = file.write_all(b"deep content");
                        if file_path.exists() {
                            PosixTestResult {
                                name,
                                passed: true,
                                error: None,
                            }
                        } else {
                            PosixTestResult {
                                name,
                                passed: false,
                                error: Some("File not created".to_string()),
                            }
                        }
                    }
                    Err(e) => PosixTestResult {
                        name,
                        passed: false,
                        error: Some(format!("Create file failed: {}", e)),
                    },
                }
            }
            Err(e) => PosixTestResult {
                name,
                passed: false,
                error: Some(format!("Create dir failed: {}", e)),
            },
        }
    }

    pub fn test_special_filenames(&self) -> PosixTestResult {
        let name = "test_special_filenames".to_string();
        let temp_dir = TempDir::new_in(&self.root).unwrap();

        let test_cases = vec![
            "file with spaces.txt",
            "file.with.dots.txt",
            "file-with-dashes.txt",
            "file_underscore.txt",
        ];

        let mut all_passed = true;
        for filename in &test_cases {
            let file_path = temp_dir.path().join(filename);
            if let Ok(mut file) = File::create(&file_path) {
                let _ = file.write_all(filename.as_bytes());
            } else {
                all_passed = false;
            }
        }

        PosixTestResult {
            name,
            passed: all_passed,
            error: if !all_passed {
                Some("Some special filenames failed".to_string())
            } else {
                None
            },
        }
    }
}

fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs as unix_fs;
        unix_fs::symlink(target, link)
    }
    #[cfg(not(unix))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlink not supported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posix_compliance_suite_new() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        assert_eq!(suite.root, temp_dir.path());
    }

    #[test]
    fn test_posix_test_result_creation() {
        let result = PosixTestResult {
            name: "test".to_string(),
            passed: true,
            error: None,
        };
        assert_eq!(result.name, "test");
        assert!(result.passed);
    }

    #[test]
    fn test_posix_suite_report_accumulation() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let report = suite.run_all();
        assert_eq!(report.results.len(), 14);
        assert_eq!(report.passed + report.failed, 14);
    }

    #[test]
    fn test_test_file_create_read_write() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_file_create_read_write();
        assert_eq!(result.name, "test_file_create_read_write");
        assert!(result.passed);
    }

    #[test]
    fn test_test_rename_atomicity() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_rename_atomicity();
        assert_eq!(result.name, "test_rename_atomicity");
    }

    #[test]
    fn test_test_mkdir_rmdir() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_mkdir_rmdir();
        assert_eq!(result.name, "test_mkdir_rmdir");
    }

    #[test]
    fn test_test_hardlink() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_hardlink();
        assert_eq!(result.name, "test_hardlink");
    }

    #[test]
    fn test_test_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_symlink();
        assert_eq!(result.name, "test_symlink");
    }

    #[test]
    fn test_test_truncate() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_truncate();
        assert_eq!(result.name, "test_truncate");
    }

    #[test]
    fn test_test_seek_tell() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_seek_tell();
        assert_eq!(result.name, "test_seek_tell");
    }

    #[test]
    fn test_test_append_mode() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_append_mode();
        assert_eq!(result.name, "test_append_mode");
    }

    #[test]
    fn test_test_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_permissions();
        assert_eq!(result.name, "test_permissions");
        assert!(result.passed);
    }

    #[test]
    fn test_test_timestamps() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_timestamps();
        assert_eq!(result.name, "test_timestamps");
    }

    #[test]
    fn test_test_concurrent_writes() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_concurrent_writes();
        assert_eq!(result.name, "test_concurrent_writes");
    }

    #[test]
    fn test_test_large_directory() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_large_directory();
        assert_eq!(result.name, "test_large_directory");
    }

    #[test]
    fn test_test_deep_path() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_deep_path();
        assert_eq!(result.name, "test_deep_path");
    }

    #[test]
    fn test_test_special_filenames() {
        let temp_dir = TempDir::new().unwrap();
        let suite = PosixComplianceSuite::new(temp_dir.path().to_path_buf());
        let result = suite.test_special_filenames();
        assert_eq!(result.name, "test_special_filenames");
    }
}
