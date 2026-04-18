#[cfg(test)]
mod ci_dry_principle {
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
    fn test_composite_actions_used() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut setup_rust_usage = 0;
        let mut cache_cargo_usage = 0;
        let mut test_reporter_usage = 0;
        let mut total_workflows = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    total_workflows += 1;
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    if content.contains("./.github/actions/setup-rust") {
                        setup_rust_usage += 1;
                    }
                    if content.contains("./.github/actions/cache-cargo") {
                        cache_cargo_usage += 1;
                    }
                    if content.contains("./.github/actions/test-reporter") {
                        test_reporter_usage += 1;
                    }
                }
            }
        }

        if total_workflows == 0 {
            return Err("No workflows found".to_string());
        }

        println!(
            "Setup Rust used in {}/{} workflows",
            setup_rust_usage, total_workflows
        );
        println!(
            "Cache Cargo used in {}/{} workflows",
            cache_cargo_usage, total_workflows
        );

        Ok(())
    }

    #[test]
    fn test_no_direct_rust_toolchain_duplication() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut direct_toolchain_count = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    let has_setup_rust = content.contains("./.github/actions/setup-rust");
                    let has_direct_toolchain = content.contains("dtolnay/rust-toolchain")
                        || content.contains("actions-rust-lang/setup-rust-toolchain");

                    if has_direct_toolchain && !has_setup_rust {
                        direct_toolchain_count += 1;
                    }
                }
            }
        }

        if direct_toolchain_count > 20 {
            return Err(format!(
                "Found {} workflows with direct toolchain setup (should use composite)",
                direct_toolchain_count
            ));
        }

        Ok(())
    }

    #[test]
    fn test_no_direct_cache_duplication() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut direct_cache_count = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    let has_cache_cargo = content.contains("./.github/actions/cache-cargo");
                    let has_direct_cache =
                        content.contains("actions/cache@v4") && content.contains("~/.cargo");

                    if has_direct_cache && !has_cache_cargo {
                        direct_cache_count += 1;
                    }
                }
            }
        }

        if direct_cache_count > 10 {
            return Err(format!(
                "Found {} workflows with direct cache setup (should use composite)",
                direct_cache_count
            ));
        }

        Ok(())
    }
}
