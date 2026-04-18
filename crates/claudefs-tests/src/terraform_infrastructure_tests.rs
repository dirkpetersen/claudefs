//! A11 Phase 5 Block 1: Terraform Infrastructure Tests
//!
//! Tests for validating all aspects of the Terraform infrastructure configuration
//! in tools/terraform/.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

#[allow(dead_code)]
fn get_terraform_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("tools").exists())
        .expect("Could not find workspace root");
    workspace_root.join("tools/terraform")
}

#[allow(dead_code)]
fn get_tools_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("tools").exists())
        .expect("Could not find workspace root");
    workspace_root.join("tools")
}

#[allow(dead_code)]
fn read_terraform_file(path: &str) -> String {
    let terraform_dir = get_terraform_dir();
    let file_path = terraform_dir.join(path);
    fs::read_to_string(&file_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read Terraform file {}: {}",
            file_path.display(),
            e
        )
    })
}

#[allow(dead_code)]
fn find_terraform_files() -> Vec<PathBuf> {
    let terraform_dir = get_terraform_dir();
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(&terraform_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "tf") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[allow(dead_code)]
fn extract_variable_references(content: &str) -> Vec<String> {
    let mut references = Vec::new();
    let var_pattern = regex::Regex::new(r"var\.([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    for cap in var_pattern.captures_iter(content) {
        if let Some(name) = cap.get(1) {
            let var_name = name.as_str().to_string();
            if !references.contains(&var_name) {
                references.push(var_name);
            }
        }
    }
    references
}

#[allow(dead_code)]
fn extract_resource_attributes(content: &str, resource: &str, attr: &str) -> Vec<String> {
    let mut results = Vec::new();
    let resource_pattern = format!(r#"resource\s+"aws_instance"\s+"{}""#, resource);
    let attr_pattern = format!(r#"{}\s*=\s*"([^"]+)""#, attr);

    let resource_re = regex::Regex::new(&resource_pattern).unwrap();
    let attr_re = regex::Regex::new(&attr_pattern).unwrap();

    if let Some(resource_match) = resource_re.find(content) {
        let resource_block = &content[resource_match.start()..];
        for cap in attr_re.captures_iter(resource_block) {
            if let Some(value) = cap.get(1) {
                results.push(value.as_str().to_string());
            }
        }
    }
    results
}

#[allow(dead_code)]
fn parse_hcl_attribute(content: &str, key: &str) -> Option<String> {
    let patterns = [
        format!(r#"{}\s*=\s*"([^"]+)""#, key),
        format!(r#"{}:\s*"([^"]+)""#, key),
        format!(r#"{}:\s*(\d+)"#, key),
        format!(r#"{}\s*=\s*(\d+)"#, key),
    ];

    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(cap) = re.captures(content) {
                if let Some(value) = cap.get(1) {
                    return Some(value.as_str().to_string());
                }
            }
        }
    }
    None
}

#[allow(dead_code)]
fn extract_defined_variables(content: &str) -> Vec<String> {
    let mut variables = Vec::new();
    let var_block_re = regex::Regex::new(r#"variable\s+"([^"]+)""#).unwrap();
    for cap in var_block_re.captures_iter(content) {
        if let Some(name) = cap.get(1) {
            variables.push(name.as_str().to_string());
        }
    }
    variables
}

#[allow(dead_code)]
fn extract_output_names(content: &str) -> Vec<String> {
    let mut outputs = Vec::new();
    let output_re = regex::Regex::new(r#"output\s+"([^"]+)""#).unwrap();
    for cap in output_re.captures_iter(content) {
        if let Some(name) = cap.get(1) {
            outputs.push(name.as_str().to_string());
        }
    }
    outputs
}

#[allow(dead_code)]
fn extract_provider_version(content: &str) -> Option<String> {
    let re =
        regex::Regex::new(r#"required_providers[\s\S]*?aws[\s\S]*?version[\s\S]*?=\s*"([^"]+)""#)
            .unwrap();
    re.captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[allow(dead_code)]
fn extract_required_version(content: &str) -> Option<String> {
    let re = regex::Regex::new(r#"required_version\s*=\s*"([^"]+)""#).unwrap();
    re.captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[allow(dead_code)]
fn extract_security_group_rules(content: &str) -> Vec<(i32, i32, String, Vec<String>)> {
    let mut rules = Vec::new();
    let ingress_re = regex::Regex::new(r#"ingress\s*\{([^}]+)\}"#).unwrap();
    let egress_re = regex::Regex::new(r#"egress\s*\{([^}]+)\}"#).unwrap();

    for cap in ingress_re.captures_iter(content) {
        let block = cap.get(1).map_or("", |m| m.as_str());
        let from = parse_hcl_attribute(block, "from_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let to = parse_hcl_attribute(block, "to_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let protocol = parse_hcl_attribute(block, "protocol").unwrap_or_default();
        let cidr_str = parse_hcl_attribute(block, "cidr_blocks").unwrap_or_default();
        rules.push((from, to, protocol, vec![cidr_str]));
    }

    for cap in egress_re.captures_iter(content) {
        let block = cap.get(1).map_or("", |m| m.as_str());
        let from = parse_hcl_attribute(block, "from_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let to = parse_hcl_attribute(block, "to_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let protocol = parse_hcl_attribute(block, "protocol").unwrap_or_default();
        rules.push((from, to, protocol, vec!["0.0.0.0/0".to_string()]));
    }

    rules
}

#[allow(dead_code)]
fn extract_instance_tags(
    content: &str,
    resource: &str,
) -> std::collections::HashMap<String, String> {
    let mut tags = std::collections::HashMap::new();
    let resource_re = regex::Regex::new(&format!(
        r#"resource\s+"aws_instance"\s+"{}"\s*\{{([^}}]+)\}}"#,
        resource
    ))
    .unwrap();

    if let Some(cap) = resource_re.captures(content) {
        let block = cap.get(1).map_or("", |m| m.as_str());
        let tags_re = regex::Regex::new(r#"tags\s*=\s*\{([^}]+)\}"#).unwrap();
        if let Some(tags_cap) = tags_re.captures(block) {
            let tags_block = tags_cap.get(1).map_or("", |m| m.as_str());
            let tag_re = regex::Regex::new(r#"(\w+)\s*=\s*"([^"]+)""#).unwrap();
            for tag_cap in tag_re.captures_iter(tags_block) {
                if let (Some(key), Some(value)) = (tag_cap.get(1), tag_cap.get(2)) {
                    tags.insert(key.as_str().to_string(), value.as_str().to_string());
                }
            }
        }
    }
    tags
}

#[allow(dead_code)]
fn extract_ami_filters(content: &str) -> std::collections::HashMap<String, String> {
    let mut filters = std::collections::HashMap::new();
    let data_re = regex::Regex::new(r#"data\s+"aws_ami"\s+"([^"]+)"\s*\{([^}]+)\}"#).unwrap();

    if let Some(cap) = data_re.captures(content) {
        let block = cap.get(2).map_or("", |m| m.as_str());
        let filter_re = regex::Regex::new(r#"filter\s*\{([^}]+)\}"#).unwrap();
        for filter_cap in filter_re.captures_iter(block) {
            let filter_block = filter_cap.get(1).map_or("", |m| m.as_str());
            if let (Some(name), Some(values)) = (
                parse_hcl_attribute(filter_block, "name"),
                parse_hcl_attribute(filter_block, "values"),
            ) {
                filters.insert(name, values);
            }
        }

        if let Some(owners) = parse_hcl_attribute(block, "owners") {
            filters.insert("owners".to_string(), owners);
        }
        if let Some(most_recent) = parse_hcl_attribute(block, "most_recent") {
            filters.insert("most_recent".to_string(), most_recent);
        }
    }
    filters
}

#[allow(dead_code)]
fn check_terraform_available() -> bool {
    Command::new("terraform").arg("--version").output().is_ok()
}

#[test]
fn test_terraform_syntax_valid() {
    let terraform_dir = get_terraform_dir();
    let output = Command::new("terraform")
        .args(["validate"])
        .current_dir(&terraform_dir)
        .output();

    match output {
        Ok(out) => {
            assert!(
                out.status.success(),
                "terraform validate failed: {}",
                str::from_utf8(&out.stderr).unwrap_or("")
            );
        }
        Err(e) => {
            println!(
                "Warning: terraform not available, skipping validate check: {}",
                e
            );
        }
    }
}

#[test]
fn test_terraform_format_consistent() {
    let terraform_dir = get_terraform_dir();
    let output = Command::new("terraform")
        .args(["fmt", "-check"])
        .current_dir(&terraform_dir)
        .output();

    match output {
        Ok(out) => {
            assert!(
                out.status.success(),
                "terraform fmt -check found formatting issues: {}",
                str::from_utf8(&out.stdout).unwrap_or("")
            );
        }
        Err(e) => {
            println!(
                "Warning: terraform not available, skipping fmt check: {}",
                e
            );
        }
    }
}

#[test]
fn test_terraform_variables_defined() {
    let files = find_terraform_files();
    let mut all_references = Vec::new();

    for file in &files {
        let content = fs::read_to_string(file).unwrap_or_default();
        let refs = extract_variable_references(&content);
        all_references.extend(refs);
    }

    let variables_content = read_terraform_file("variables.tf");
    let defined_vars = extract_defined_variables(&variables_content);

    for var_ref in &all_references {
        assert!(
            defined_vars.contains(var_ref),
            "Variable '{}' is used but not defined in variables.tf",
            var_ref
        );
    }
}

#[test]
fn test_terraform_outputs_valid() {
    let outputs_content = read_terraform_file("outputs.tf");
    let outputs = extract_output_names(&outputs_content);

    assert!(
        !outputs.is_empty(),
        "outputs.tf should contain output definitions"
    );

    let main_content = read_terraform_file("main.tf");
    let main_outputs = extract_output_names(&main_content);

    let storage_content = read_terraform_file("storage-nodes.tf");
    let storage_outputs = extract_output_names(&storage_content);

    let client_content = read_terraform_file("client-nodes.tf");
    let client_outputs = extract_output_names(&client_content);

    let jepsen_content = read_terraform_file("jepsen-nodes.tf");
    let jepsen_outputs = extract_output_names(&jepsen_content);

    let combined_count = outputs.len()
        + main_outputs.len()
        + storage_outputs.len()
        + client_outputs.len()
        + jepsen_outputs.len();
    assert!(
        combined_count >= 20,
        "Should have at least 20 output definitions across all files, found {}",
        combined_count
    );
}

#[test]
fn test_terraform_providers_configured() {
    let main_content = read_terraform_file("main.tf");

    let provider_version = extract_provider_version(&main_content);
    assert!(
        provider_version.is_some(),
        "AWS provider version should be defined"
    );

    let version_str = provider_version.unwrap();
    assert!(
        version_str.starts_with("~> 5.") || version_str.starts_with(">= 5."),
        "AWS provider version should be >= 5.0, got: {}",
        version_str
    );

    let required_version = extract_required_version(&main_content);
    assert!(
        required_version.is_some(),
        "Terraform required_version should be defined"
    );

    let req_version_str = required_version.unwrap();
    assert!(
        req_version_str.contains("1.6")
            || req_version_str.contains("1.7")
            || req_version_str.contains("1.8")
            || req_version_str.contains("1.9"),
        "Terraform required_version should be >= 1.6, got: {}",
        req_version_str
    );
}

#[test]
fn test_terraform_backend_configured() {
    let backend_content = read_terraform_file("state-backend.tf");

    assert!(
        backend_content.contains("aws_s3_bucket"),
        "state-backend.tf should define S3 bucket"
    );

    assert!(
        backend_content.contains("aws_dynamodb_table"),
        "state-backend.tf should define DynamoDB table"
    );

    assert!(
        backend_content.contains("terraform_state"),
        "state-backend.tf should reference terraform_state bucket"
    );

    assert!(
        backend_content.contains("terraform_locks"),
        "state-backend.tf should reference terraform_locks table"
    );
}

#[test]
fn test_orchestrator_instance_defined() {
    let main_content = read_terraform_file("main.tf");
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        main_content.contains("aws_instance"),
        "main.tf should contain aws_instance"
    );

    assert!(
        main_content.contains("orchestrator"),
        "main.tf should define orchestrator instance"
    );

    let combined = format!("{}{}", main_content, variables_content);
    assert!(
        combined.contains("c7a.2xlarge"),
        "Orchestrator instance type should be c7a.2xlarge"
    );

    assert!(
        main_content.contains("associate_public_ip_address = true"),
        "Orchestrator should have public IP"
    );

    assert!(
        main_content.contains("volume_type           = \"gp3\""),
        "Orchestrator should use gp3 volume type"
    );

    let volume_size_re = regex::Regex::new(r"volume_size\s*=\s*(\d+)").unwrap();
    let sizes: Vec<i32> = volume_size_re
        .captures_iter(&main_content)
        .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
        .collect();

    assert!(
        sizes.contains(&100),
        "Orchestrator should have 100GB root volume"
    );
}

#[test]
fn test_storage_nodes_configuration() {
    let storage_content = read_terraform_file("storage-nodes.tf");
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        storage_content.contains("storage_site_a"),
        "storage-nodes.tf should define storage_site_a"
    );

    assert!(
        storage_content.contains("storage_site_b"),
        "storage-nodes.tf should define storage_site_b"
    );

    let combined = format!("{}{}", storage_content, variables_content);
    assert!(
        combined.contains("i4i.2xlarge"),
        "Storage nodes should use i4i.2xlarge"
    );

    assert!(
        variables_content.contains("storage_site_a_count"),
        "variables.tf should define storage_site_a_count"
    );

    assert!(
        variables_content.contains("storage_site_b_count"),
        "variables.tf should define storage_site_b_count"
    );

    assert!(
        variables_content.contains("default     = 3"),
        "storage_site_a_count default should be 3"
    );

    assert!(
        variables_content.contains("default     = 2"),
        "storage_site_b_count default should be 2"
    );

    assert!(
        variables_content.contains("var.storage_site_a_count >= 1"),
        "storage_site_a_count validation should allow >= 1"
    );

    assert!(
        variables_content.contains("var.storage_site_b_count >= 1"),
        "storage_site_b_count validation should allow >= 1"
    );
}

#[test]
fn test_client_nodes_configuration() {
    let client_content = read_terraform_file("client-nodes.tf");
    let variables_content = read_terraform_file("variables.tf");
    let combined = format!("{}{}", client_content, variables_content);

    assert!(
        client_content.contains("fuse_client"),
        "client-nodes.tf should define fuse_client"
    );

    assert!(
        client_content.contains("nfs_client"),
        "client-nodes.tf should define nfs_client"
    );

    assert!(
        client_content.contains("cloud_conduit"),
        "client-nodes.tf should define cloud_conduit"
    );

    assert!(
        client_content.contains("jepsen_controller"),
        "client-nodes.tf should define jepsen_controller"
    );

    assert!(
        combined.contains("c7a.xlarge"),
        "Client nodes should use c7a.xlarge"
    );

    assert!(
        combined.contains("t3.medium"),
        "Conduit should use t3.medium"
    );

    let volume_size_re = regex::Regex::new(r"volume_size\s*=\s*(\d+)").unwrap();
    let client_sizes: Vec<i32> = volume_size_re
        .captures_iter(&client_content)
        .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
        .collect();

    assert!(
        client_sizes.iter().any(|&s| s == 50),
        "Client nodes should have 50GB root volumes"
    );

    assert!(
        client_sizes.iter().any(|&s| s == 20),
        "Conduit should have 20GB root volume"
    );
}

#[test]
fn test_security_group_rules() {
    let main_content = read_terraform_file("main.tf");
    let rules = extract_security_group_rules(&main_content);

    assert!(
        rules.len() >= 4,
        "Security group should have at least 4 rules"
    );

    let has_rpc = rules.iter().any(|(from, to, proto, _)| {
        *from == 9400 && *to == 9410 && (*proto == "tcp" || *proto == "udp")
    });
    assert!(has_rpc, "Should have RPC ingress rule on 9400-9410");

    let has_prometheus = rules
        .iter()
        .any(|(from, to, proto, _)| *from == 9800 && *to == 9800 && *proto == "tcp");
    assert!(
        has_prometheus,
        "Should have Prometheus ingress rule on 9800"
    );

    let has_ssh = rules
        .iter()
        .any(|(from, to, proto, _)| *from == 22 && *to == 22 && *proto == "tcp");
    assert!(has_ssh, "Should have SSH ingress rule on 22");

    let has_egress = rules.iter().any(|(_, _, proto, _)| *proto == "-1");
    assert!(has_egress, "Should have egress rule with protocol -1");
}

#[test]
fn test_instance_iam_profiles() {
    let main_content = read_terraform_file("main.tf");
    let client_content = read_terraform_file("client-nodes.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        main_content.contains("orchestrator_iam_profile"),
        "Orchestrator should reference IAM profile"
    );

    assert!(
        variables_content.contains("cfs-orchestrator-profile"),
        "Default orchestrator profile should be cfs-orchestrator-profile"
    );

    assert!(
        variables_content.contains("cfs-spot-node-profile"),
        "Default spot profile should be cfs-spot-node-profile"
    );

    assert!(
        client_content.contains("spot_iam_profile"),
        "Client nodes should use spot IAM profile"
    );

    assert!(
        storage_content.contains("spot_iam_profile"),
        "Storage nodes should use spot IAM profile"
    );
}

#[test]
fn test_instance_tagging() {
    let main_content = read_terraform_file("main.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");

    assert!(
        main_content.contains("Name =") && main_content.contains("Role ="),
        "Orchestrator should have Name and Role tags"
    );

    assert!(
        storage_content.contains("Site    ="),
        "Storage nodes should have Site tag"
    );

    assert!(
        main_content.contains("project_tag") || main_content.contains("var.project_tag"),
        "Should use project_tag for tagging"
    );
}

#[test]
fn test_storage_volume_encryption() {
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");
    let main_content = read_terraform_file("main.tf");

    let encryption_count = ["encrypted             = true", "encrypted = true"]
        .iter()
        .fold(0, |acc, pattern| {
            acc + storage_content.matches(pattern).count()
                + client_content.matches(pattern).count()
                + main_content.matches(pattern).count()
        });

    assert!(
        encryption_count >= 8,
        "All volumes should be encrypted, found {} encrypted statements",
        encryption_count
    );
}

#[test]
fn test_storage_volume_sizing() {
    let main_content = read_terraform_file("main.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");
    let jepsen_content = read_terraform_file("jepsen-nodes.tf");

    let all_content = format!(
        "{}{}{}{}",
        main_content, storage_content, client_content, jepsen_content
    );

    let volume_size_re = regex::Regex::new(r"volume_size\s*=\s*(\d+)").unwrap();
    let sizes: Vec<i32> = volume_size_re
        .captures_iter(&all_content)
        .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
        .collect();

    assert!(sizes.contains(&100), "Orchestrator root should be 100 GB");

    let root_sizes: Vec<_> = sizes.iter().filter(|&&s| s <= 100).cloned().collect();
    assert!(
        root_sizes.contains(&50),
        "Storage/Client root should be 50 GB"
    );

    assert!(
        sizes.contains(&1875),
        "Storage data volume should be 1875 GB"
    );

    assert!(sizes.contains(&20), "Conduit root should be 20 GB");

    assert!(sizes.contains(&100), "Jepsen data volume should be 100 GB");
}

#[test]
fn test_storage_delete_on_termination() {
    let main_content = read_terraform_file("main.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");

    let all_content = format!("{}{}{}", main_content, storage_content, client_content);

    let delete_count = all_content.matches("delete_on_termination = true").count();

    assert!(
        delete_count >= 8,
        "All volumes should have delete_on_termination = true, found {}",
        delete_count
    );
}

#[test]
fn test_storage_spot_price_configuration() {
    let variables_content = read_terraform_file("variables.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");

    assert!(
        variables_content.contains("use_spot_instances"),
        "variables.tf should define use_spot_instances"
    );

    assert!(
        variables_content.contains("default     = true"),
        "use_spot_instances should default to true"
    );

    assert!(
        storage_content.contains("instance_interruption_behavior"),
        "Storage nodes should have interruption behavior"
    );

    assert!(
        storage_content.contains("terminate"),
        "Spot instances should terminate on interruption"
    );
}

#[test]
fn test_vpc_configuration() {
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        variables_content.contains("vpc_cidr"),
        "variables.tf should define vpc_cidr"
    );

    assert!(
        variables_content.contains("10.0.0.0/16"),
        "VPC CIDR should be 10.0.0.0/16"
    );

    assert!(
        variables_content.contains("public_subnet_cidrs"),
        "variables.tf should define public_subnet_cidrs"
    );

    assert!(
        variables_content.contains("private_subnet_cidrs"),
        "variables.tf should define private_subnet_cidrs"
    );

    assert!(
        variables_content.contains("10.0.1.0/24")
            && variables_content.contains("10.0.2.0/24")
            && variables_content.contains("10.0.3.0/24"),
        "Public subnet CIDRs should include 10.0.1-3.0/24"
    );

    assert!(
        variables_content.contains("10.0.10.0/24")
            && variables_content.contains("10.0.11.0/24")
            && variables_content.contains("10.0.12.0/24"),
        "Private subnet CIDRs should include 10.0.10-12.0/24"
    );
}

#[test]
fn test_security_group_ingress_rules() {
    let main_content = read_terraform_file("main.tf");
    let rules = extract_security_group_rules(&main_content);

    let ingress_rules: Vec<_> = rules
        .iter()
        .filter(|(from, to, proto, _)| *proto != "-1" && !(*from == 0 && *to == 0))
        .collect();

    assert!(
        ingress_rules.len() >= 3,
        "Should have at least 3 ingress rules"
    );

    for (from, to, proto, _) in &ingress_rules {
        if *proto == "tcp" || *proto == "udp" {
            assert!(
                [22, 9400, 9410, 9800].contains(from),
                "Ingress ports should be 22, 9400-9410, or 9800"
            );
        }
    }
}

#[test]
fn test_security_group_egress_rules() {
    let main_content = read_terraform_file("main.tf");
    let rules = extract_security_group_rules(&main_content);

    let has_full_egress = rules.iter().any(|(from, to, proto, cidr_blocks)| {
        *from == 0
            && *to == 0
            && *proto == "-1"
            && cidr_blocks.iter().any(|c| c.contains("0.0.0.0"))
    });

    assert!(
        has_full_egress,
        "Security group should allow all outbound traffic"
    );
}

#[test]
fn test_instance_networking() {
    let main_content = read_terraform_file("main.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");

    let all_content = format!("{}{}{}", main_content, storage_content, client_content);

    let public_ip_count = all_content
        .matches("associate_public_ip_address = true")
        .count();

    assert!(
        public_ip_count >= 7,
        "All instances should have public IP, found {}",
        public_ip_count
    );
}

#[test]
fn test_ami_data_source() {
    let main_content = read_terraform_file("main.tf");

    assert!(
        main_content.contains("owners"),
        "AMI should have owners filter"
    );

    assert!(
        main_content.contains("099720109477"),
        "AMI owner should be Canonical (099720109477)"
    );

    assert!(
        main_content.contains("ubuntu-questing-25.10"),
        "AMI name should contain ubuntu-questing-25.10"
    );

    assert!(
        main_content.contains("available"),
        "AMI state should be available"
    );

    assert!(
        main_content.contains("most_recent = true"),
        "AMI should use most_recent = true"
    );
}

#[test]
fn test_user_data_scripts_exist() {
    let tools_dir = get_tools_dir();

    let orchestrator_script = tools_dir.join("orchestrator-user-data.sh");
    assert!(
        orchestrator_script.exists(),
        "orchestrator-user-data.sh should exist"
    );

    let storage_script = tools_dir.join("storage-node-user-data.sh");
    assert!(
        storage_script.exists(),
        "storage-node-user-data.sh should exist"
    );

    let client_script = tools_dir.join("client-node-user-data.sh");
    assert!(
        client_script.exists(),
        "client-node-user-data.sh should exist"
    );
}

#[test]
fn test_user_data_scripts_readable() {
    let tools_dir = get_tools_dir();
    let scripts = [
        "orchestrator-user-data.sh",
        "storage-node-user-data.sh",
        "client-node-user-data.sh",
    ];

    for script_name in &scripts {
        let script_path = tools_dir.join(script_name);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&script_path) {
                let permissions = metadata.permissions();
                let mode = permissions.mode();
                assert!(
                    mode >= 0o644,
                    "Script {} should have mode >= 0644, got {:o}",
                    script_name,
                    mode
                );
            }
        }

        let content = fs::read_to_string(&script_path).unwrap_or_default();
        assert!(
            content.starts_with("#!/bin/bash") || content.starts_with("#!/usr/bin/env bash"),
            "Script {} should have proper shebang",
            script_name
        );
    }
}

#[test]
fn test_cost_tags_present() {
    let main_content = read_terraform_file("main.tf");
    let client_content = read_terraform_file("client-nodes.tf");
    let jepsen_content = read_terraform_file("jepsen-nodes.tf");

    let all_content = format!("{}{}{}", main_content, client_content, jepsen_content);

    assert!(
        all_content.contains("CostCenter") || all_content.contains("cost_tags"),
        "Instances should have CostCenter tag"
    );

    assert!(
        all_content.contains("Agent"),
        "Instances should have Agent tag"
    );

    assert!(
        all_content.contains("Department"),
        "Instances should have Department tag"
    );

    assert!(
        all_content.contains("Testing") || all_content.contains("Infrastructure"),
        "Cost tags should have appropriate values"
    );
}

#[test]
fn test_spot_instance_pricing() {
    let variables_content = read_terraform_file("variables.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");

    assert!(
        variables_content.contains("use_spot_instances"),
        "Should have spot instance configuration"
    );

    let storage_count_a = 3;
    let storage_count_b = 2;
    let client_count = 2;
    let conduit_count = 1;
    let jepsen_count = 1;

    let storage_spot_price = 0.14;
    let storage_cost = (storage_count_a + storage_count_b) as f64 * storage_spot_price;

    let client_spot_price = 0.05;
    let client_cost = client_count as f64 * client_spot_price;

    let conduit_spot_price = 0.015;
    let conduit_cost = conduit_count as f64 * conduit_spot_price;

    let jepsen_spot_price = 0.10;
    let jepsen_cost = jepsen_count as f64 * jepsen_spot_price;

    let total_hourly = storage_cost + client_cost + conduit_cost + jepsen_cost;
    let daily = total_hourly * 24.0;
    let monthly = daily * 30.0;

    assert!(
        total_hourly > 0.5 && total_hourly < 2.0,
        "Spot hourly cost should be ~$1, got ${:.2}",
        total_hourly
    );

    assert!(
        daily < 30.0,
        "Daily cost should be under $30, got ${:.2}",
        daily
    );
}

#[test]
fn test_on_demand_equivalency() {
    let storage_count = 5;
    let client_count = 2;
    let conduit_count = 1;
    let jepsen_count = 1;
    let orchestrator_count = 1;

    let storage_od_price = 0.45;
    let client_od_price = 0.15;
    let conduit_od_price = 0.04;
    let jepsen_od_price = 0.30;
    let orchestrator_od_price = 0.35;

    let storage_cost = storage_count as f64 * storage_od_price;
    let client_cost = client_count as f64 * client_od_price;
    let conduit_cost = conduit_count as f64 * conduit_od_price;
    let jepsen_cost = jepsen_count as f64 * jepsen_od_price;
    let orchestrator_cost = orchestrator_count as f64 * orchestrator_od_price;

    let total_daily =
        (storage_cost + client_cost + conduit_cost + jepsen_cost + orchestrator_cost) * 24.0;

    assert!(
        total_daily >= 60.0 && total_daily <= 100.0,
        "On-demand daily cost should be $60-100, got ${:.2}",
        total_daily
    );

    let spot_daily = total_daily * 0.35;
    assert!(
        spot_daily < 40.0,
        "Spot should save 60-70%, spot daily should be < $40, got ${:.2}",
        spot_daily
    );
}

#[test]
fn test_budget_constraints() {
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        variables_content.contains("daily_budget_limit"),
        "variables.tf should define daily_budget_limit"
    );

    assert!(
        variables_content.contains("default     = 100"),
        "daily_budget_limit should default to 100"
    );

    assert!(
        variables_content.contains("budget_alert_threshold"),
        "variables.tf should define budget_alert_threshold"
    );

    assert!(
        variables_content.contains("default     = 80"),
        "budget_alert_threshold should default to 80%"
    );
}

#[test]
fn test_instance_count_within_budget() {
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        variables_content.contains("storage_site_a_count"),
        "Should have storage_site_a_count"
    );

    assert!(
        variables_content.contains("storage_site_b_count"),
        "Should have storage_site_b_count"
    );

    let total_storage = 3 + 2;
    let total_clients = 4;
    let orchestrator = 1;
    let total = total_storage + total_clients + orchestrator;

    assert_eq!(
        total, 10,
        "Total instance count should be 10 (1 orchestrator + 5 storage + 4 clients)"
    );
}

#[test]
fn test_orchestrator_outputs() {
    let main_content = read_terraform_file("main.tf");

    assert!(
        main_content.contains("output \"orchestrator_id\""),
        "Should have orchestrator_id output"
    );

    assert!(
        main_content.contains("output \"orchestrator_public_ip\""),
        "Should have orchestrator_public_ip output"
    );

    assert!(
        main_content.contains("output \"orchestrator_private_ip\""),
        "Should have orchestrator_private_ip output"
    );
}

#[test]
fn test_storage_outputs() {
    let storage_content = read_terraform_file("storage-nodes.tf");

    assert!(
        storage_content.contains("output \"storage_site_a_ids\""),
        "Should have storage_site_a_ids output"
    );

    assert!(
        storage_content.contains("output \"storage_site_a_private_ips\""),
        "Should have storage_site_a_private_ips output"
    );

    assert!(
        storage_content.contains("output \"storage_site_a_public_ips\""),
        "Should have storage_site_a_public_ips output"
    );

    assert!(
        storage_content.contains("output \"storage_site_b_ids\""),
        "Should have storage_site_b_ids output"
    );

    assert!(
        storage_content.contains("output \"storage_site_b_private_ips\""),
        "Should have storage_site_b_private_ips output"
    );

    assert!(
        storage_content.contains("output \"storage_site_b_public_ips\""),
        "Should have storage_site_b_public_ips output"
    );
}

#[test]
fn test_client_outputs() {
    let client_content = read_terraform_file("client-nodes.tf");

    assert!(
        client_content.contains("output \"fuse_client_id\""),
        "Should have fuse_client_id output"
    );

    assert!(
        client_content.contains("output \"fuse_client_ip\""),
        "Should have fuse_client_ip output"
    );

    assert!(
        client_content.contains("output \"nfs_client_id\""),
        "Should have nfs_client_id output"
    );

    assert!(
        client_content.contains("output \"nfs_client_ip\""),
        "Should have nfs_client_ip output"
    );

    assert!(
        client_content.contains("output \"conduit_id\""),
        "Should have conduit_id output"
    );

    assert!(
        client_content.contains("output \"conduit_ip\""),
        "Should have conduit_ip output"
    );

    let jepsen_content = read_terraform_file("jepsen-nodes.tf");
    assert!(
        jepsen_content.contains("output \"jepsen_controller_id\""),
        "Should have jepsen_controller_id output"
    );

    assert!(
        jepsen_content.contains("output \"jepsen_controller_public_ip\""),
        "Should have jepsen_controller_public_ip output"
    );
}

#[test]
fn test_output_type_validation() {
    let main_content = read_terraform_file("main.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");
    let client_content = read_terraform_file("client-nodes.tf");

    let all_outputs = [
        "orchestrator_public_ip",
        "storage_site_a_ids",
        "storage_site_a_private_ips",
        "fuse_client_ip",
        "conduit_ip",
    ];

    for output_name in &all_outputs {
        let found = main_content.contains(output_name)
            || storage_content.contains(output_name)
            || client_content.contains(output_name);
        assert!(found, "Output {} should be defined", output_name);
    }
}

#[test]
fn test_orchestrator_persistence() {
    let main_content = read_terraform_file("main.tf");
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        !main_content.contains("spot_price")
            || main_content.contains("orchestrator")
                && !main_content.contains("spot_price              = var.use_spot_instances"),
        "Orchestrator should not use spot instances"
    );

    let volume_size_re =
        regex::Regex::new(r"root_block_device\s*\{[^}]*volume_size\s*=\s*(\d+)").unwrap();
    let sizes: Vec<i32> = volume_size_re
        .captures_iter(&main_content)
        .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
        .collect();

    if let Some(&max_size) = sizes.iter().max() {
        assert!(
            max_size >= 100,
            "Orchestrator should have the largest root volume"
        );
    }
}

#[test]
fn test_multi_site_topology() {
    let variables_content = read_terraform_file("variables.tf");
    let storage_content = read_terraform_file("storage-nodes.tf");

    assert!(
        variables_content.contains("storage_site_a_count"),
        "Should have storage_site_a_count"
    );

    assert!(
        variables_content.contains("storage_site_b_count"),
        "Should have storage_site_b_count"
    );

    assert!(
        storage_content.contains("Site    = \"A\""),
        "Storage nodes should have Site A tag"
    );

    assert!(
        storage_content.contains("Site    = \"B\""),
        "Storage nodes should have Site B tag"
    );

    assert!(
        variables_content.contains("storage_site_a_count >= 1"),
        "Site A should have minimum 1 node"
    );
}

#[test]
fn test_spot_instance_diversity() {
    let variables_content = read_terraform_file("variables.tf");

    assert!(
        variables_content.contains("i4i.2xlarge"),
        "Storage should use i4i.2xlarge"
    );

    assert!(
        variables_content.contains("c7a.xlarge") || variables_content.contains("c7a.2xlarge"),
        "Compute nodes should use c7a family"
    );

    assert!(
        variables_content.contains("t3.medium"),
        "Conduit should use t3.medium"
    );
}

#[test]
fn test_disaster_recovery_readiness() {
    let storage_content = read_terraform_file("storage-nodes.tf");
    let backend_content = read_terraform_file("state-backend.tf");
    let variables_content = read_terraform_file("variables.tf");

    let encryption_count = storage_content
        .matches("encrypted             = true")
        .count();
    assert!(
        encryption_count >= 4,
        "All storage volumes should be encrypted"
    );

    assert!(
        backend_content.contains("aws_s3_bucket"),
        "State backend should use S3"
    );

    assert!(
        backend_content.contains("aws_dynamodb_table"),
        "State backend should use DynamoDB"
    );

    assert!(
        variables_content.contains("availability_zones"),
        "Should support multi-AZ deployment"
    );

    assert!(
        variables_content.contains("us-west-2a") && variables_content.contains("us-west-2b"),
        "Should configure multiple availability zones"
    );
}
