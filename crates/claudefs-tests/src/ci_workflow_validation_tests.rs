#[cfg(test)]
mod ci_workflow_validation {
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
    fn test_workflow_syntax_valid() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        if !workflows_dir.exists() {
            return Err("workflows directory not found".to_string());
        }

        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut yml_count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    yml_count += 1;
                    let content = fs::read_to_string(&path)
                        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

                    if !content.contains("name:") {
                        return Err(format!("{} missing 'name' field", path.display()));
                    }
                    if !content.contains("on:") {
                        return Err(format!("{} missing 'on' field", path.display()));
                    }
                    if !content.contains("jobs:") {
                        return Err(format!("{} missing 'jobs' field", path.display()));
                    }
                }
            }
        }

        if yml_count == 0 {
            return Err("No workflow files found".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_workflow_triggers_configured() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut push_count = 0;
        let mut pull_request_count = 0;
        let mut workflow_dispatch_count = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    if content.contains("push:") {
                        push_count += 1;
                    }
                    if content.contains("pull_request:") {
                        pull_request_count += 1;
                    }
                    if content.contains("workflow_dispatch:") {
                        workflow_dispatch_count += 1;
                    }
                }
            }
        }

        if push_count == 0 {
            return Err("No workflows with push trigger found".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_workflow_timeout_configured() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut jobs_with_timeout = 0;
        let mut jobs_without_timeout = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    let job_sections: Vec<&str> = content.split("jobs:").collect();
                    for section in job_sections.iter().skip(1) {
                        if section.contains("timeout-minutes:") {
                            jobs_with_timeout += 1;
                        } else if section.contains("name:") {
                            jobs_without_timeout += 1;
                        }
                    }
                }
            }
        }

        if jobs_without_timeout > 0 && jobs_with_timeout == 0 {
            return Err("No jobs have timeout configured".to_string());
        }

        Ok(())
    }

    #[test]
    fn test_workflow_artifact_lifecycle() -> Result<(), String> {
        let root = workspace_root();
        let workflows_dir = root.join(".github/workflows");
        let entries = fs::read_dir(&workflows_dir).map_err(|e| e.to_string())?;

        let mut artifact_count = 0;
        let mut with_retention = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "yml" || ext == "yaml" {
                    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;

                    if content.contains("upload-artifact") {
                        artifact_count += 1;
                        if content.contains("retention-days:") {
                            with_retention += 1;
                        }
                    }
                }
            }
        }

        if artifact_count > 0 && with_retention == 0 {
            return Err("No artifacts have retention-days configured".to_string());
        }

        Ok(())
    }
}
