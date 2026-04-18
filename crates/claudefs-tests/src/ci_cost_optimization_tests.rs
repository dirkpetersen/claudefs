#[cfg(test)]
mod ci_cost_optimization {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    #[test]
    fn test_cache_keys_use_cargo_lock() -> Result<(), String> {
        let root = workspace_root();

        let action_paths = vec![
            root.join(".github/actions/cache-cargo/action.yml"),
            root.join(".github/actions/setup-rust/action.yml"),
        ];

        for action_path in action_paths {
            if action_path.exists() {
                let content = fs::read_to_string(&action_path).map_err(|e| e.to_string())?;

                if content.contains("key:") {
                    if !content.contains("Cargo.lock") && !content.contains("cargo") {
                        return Err(format!(
                            "{} uses cache key but not Cargo.lock",
                            action_path.display()
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_workflow_concurrency_configured() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut with_concurrency = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    if content.contains("concurrency:") {
                        with_concurrency += 1;
                    }
                }
            }
        }

        if with_concurrency == 0 {
            return Err("No workflows have concurrency configured".to_string());
        }

        Ok(())
    }
}
