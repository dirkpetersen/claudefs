#[cfg(test)]
mod ci_composite_actions {
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
    fn test_setup_rust_action_exists() -> Result<(), String> {
        let root = workspace_root();
        let path = root.join(".github/actions/setup-rust/action.yml");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("setup-rust action not found: {} - {}", path.display(), e))?;

        if !content.contains("name: Setup Rust Toolchain") {
            return Err("Missing name field".to_string());
        }
        if !content.contains("inputs:") {
            return Err("Missing inputs section".to_string());
        }
        if !content.contains("toolchain:") {
            return Err("Missing toolchain input".to_string());
        }
        if !content.contains("components:") {
            return Err("Missing components input".to_string());
        }
        if !content.contains("outputs:") {
            return Err("Missing outputs section".to_string());
        }
        if !content.contains("rustc-version:") {
            return Err("Missing rustc-version output".to_string());
        }
        if !content.contains("using: composite") {
            return Err("Not using composite runner".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_cache_cargo_action_config() -> Result<(), String> {
        let root = workspace_root();
        let path = root.join(".github/actions/cache-cargo/action.yml");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("cache-cargo action not found: {} - {}", path.display(), e))?;

        if !content.contains("cache-target-debug:") {
            return Err("Missing cache-target-debug input".to_string());
        }
        if !content.contains("cache-target-release:") {
            return Err("Missing cache-target-release input".to_string());
        }
        if !content.contains("cache-hit:") {
            return Err("Missing cache-hit output".to_string());
        }
        if !content.contains("actions/cache@v4") {
            return Err("Not using actions/cache@v4".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_test_reporter_action_integration() -> Result<(), String> {
        let root = workspace_root();
        let path = root.join(".github/actions/test-reporter/action.yml");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("test-reporter action not found: {} - {}", path.display(), e))?;

        if !content.contains("test-type:") {
            return Err("Missing test-type input".to_string());
        }
        if !content.contains("fail-on-error:") {
            return Err("Missing fail-on-error input".to_string());
        }
        if !content.contains("artifact-name:") {
            return Err("Missing artifact-name input".to_string());
        }
        if !content.contains("test-count:") {
            return Err("Missing test-count output".to_string());
        }
        if !content.contains("dorny/test-reporter") {
            return Err("Not using dorny/test-reporter".to_string());
        }
        if !content.contains("actions/upload-artifact") {
            return Err("Not using upload-artifact".to_string());
        }

        Ok(())
    }
}
