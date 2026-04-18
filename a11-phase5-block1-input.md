# A11: Phase 5 Block 1 — Terraform Infrastructure Tests (OpenCode Prompt)

**Date:** 2026-04-18 Session 10
**Agent:** A11 Infrastructure & CI
**Model:** minimax-m2p5
**Task:** Generate 36 comprehensive Terraform validation tests in Rust

---

## Context

ClaudeFS is a distributed POSIX file system. Phase 5 Block 1 focuses on Terraform automation for test cluster provisioning.

**Existing Terraform Infrastructure:**
- tools/terraform/main.tf — AWS provider, orchestrator instance, security groups
- tools/terraform/storage-nodes.tf — 5 i4i.2xlarge nodes (2 sites)
- tools/terraform/client-nodes.tf — FUSE + NFS clients, conduit, Jepsen controller
- tools/terraform/jepsen-nodes.tf — Jepsen test orchestrator (newly created)
- tools/terraform/variables.tf — 50+ configuration variables
- tools/terraform/iam-roles.tf — IAM instance profiles
- tools/terraform/state-backend.tf — S3 remote state configuration
- tools/terraform/outputs.tf — Terraform outputs (IPs, IDs, DNS)

**Test Specifications:** docs/A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md (complete spec with 36 tests)

---

## Task

Create a comprehensive Rust test module that validates all aspects of the Terraform infrastructure configuration.

**File Path:** `crates/claudefs-tests/src/terraform_infrastructure_tests.rs`

**Target:** 600+ lines of Rust test code, 36 passing tests

---

## Test Groups (36 tests total)

### Group 1: Terraform Syntax & Validation (6 tests)

1. **test_terraform_syntax_valid**
   - Verify `terraform validate` passes for all .tf files in tools/terraform/
   - Should exit with code 0
   - Parse output to ensure no validation errors

2. **test_terraform_format_consistent**
   - Run `terraform fmt -check` to verify formatting consistency
   - Should exit with code 0 (no formatting issues)

3. **test_terraform_variables_defined**
   - Parse all .tf files for variable references
   - Verify each referenced variable is declared in variables.tf
   - No undefined variable references allowed

4. **test_terraform_outputs_valid**
   - Validate all output blocks in outputs.tf and jepsen-nodes.tf
   - Verify output syntax and references
   - All referenced resources must exist

5. **test_terraform_providers_configured**
   - Verify AWS provider is configured in main.tf
   - Check provider version constraint: >= 5.0
   - Verify required_version >= 1.6

6. **test_terraform_backend_configured**
   - Verify S3 backend is configured in state-backend.tf
   - Check S3 bucket name is set
   - Check DynamoDB lock table is configured

---

### Group 2: Resource Definitions (6 tests)

7. **test_orchestrator_instance_defined**
   - Verify aws_instance.orchestrator resource exists
   - Validate: instance_type = "c7a.2xlarge"
   - Validate: associate_public_ip_address = true
   - Validate: root_block_device.volume_type = "gp3"
   - Validate: root_block_device.volume_size = 100

8. **test_storage_nodes_configuration**
   - Verify aws_instance.storage_site_a defined with count > 0
   - Verify aws_instance.storage_site_b defined with count > 0
   - Validate Site A default: 3 nodes, min 1, max 10
   - Validate Site B default: 2 nodes, min 1, max 10
   - Validate instance_type = "i4i.2xlarge"

9. **test_client_nodes_configuration**
   - Verify all 4 client node resources exist:
     - aws_instance.fuse_client
     - aws_instance.nfs_client
     - aws_instance.cloud_conduit
     - aws_instance.jepsen_controller
   - Validate FUSE instance_type = "c7a.xlarge"
   - Validate NFS instance_type = "c7a.xlarge"
   - Validate conduit instance_type = "t3.medium"
   - Validate Jepsen instance_type = "c7a.xlarge"

10. **test_security_group_rules**
    - Verify aws_security_group.cluster exists
    - Check ingress rules:
      - RPC: 9400-9410 (TCP + UDP) from cluster_cidr
      - Prometheus: 9800 (TCP) from cluster_cidr
      - SSH: 22 (TCP) from ssh_cidr_blocks
    - Check egress rules:
      - Allow all outbound (protocol -1)

11. **test_instance_iam_profiles**
    - Verify all instances have iam_instance_profile set
    - Orchestrator: "cfs-orchestrator-profile"
    - All other instances: "cfs-spot-node-profile"

12. **test_instance_tagging**
    - Verify all instances have required tags:
      - Name (unique per instance)
      - Role (orchestrator/storage/client/jepsen/conduit)
      - Site (A/B for storage nodes)
    - Verify cost tags present:
      - CostCenter = "Testing"
      - Agent = "A11"
      - Department = "Infrastructure"

---

### Group 3: Storage & Volume Configuration (4 tests)

13. **test_storage_volume_encryption**
    - Verify all EBS volumes have encrypted = true
    - Check root_block_device across all instances
    - Check ebs_block_device for storage nodes

14. **test_storage_volume_sizing**
    - Verify volume sizes:
      - Orchestrator root: 100 GB
      - Storage node root: 50 GB
      - Storage node data: 1875 GB
      - Client root: 50 GB
      - Jepsen root: 50 GB
      - Jepsen results: 100 GB
      - Conduit: 20 GB

15. **test_storage_delete_on_termination**
    - Verify all volumes have delete_on_termination = true
    - Prevent orphaned volumes after termination

16. **test_storage_spot_price_configuration**
    - Verify use_spot_instances variable = true
    - Verify spot_max_price handling
    - Verify instance_interruption_behavior = "terminate"

---

### Group 4: Networking & Security (4 tests)

17. **test_vpc_configuration**
    - Verify VPC CIDR blocks:
      - VPC CIDR: 10.0.0.0/16
      - Public subnets: 10.0.1-3.0/24 (no overlap)
      - Private subnets: 10.0.10-12.0/24 (no overlap)

18. **test_security_group_ingress_rules**
    - Verify only required inbound traffic:
      - ClaudeFS RPC (9400-9410)
      - Prometheus (9800)
      - SSH (22)
    - No unnecessary open ports

19. **test_security_group_egress_rules**
    - Verify outbound traffic allowed:
      - All protocols (protocol -1)
      - Necessary for AWS API calls

20. **test_instance_networking**
    - Verify all instances have:
      - associate_public_ip_address = true
      - subnet_id set
      - vpc_security_group_ids includes cluster security group

---

### Group 5: AMI & User Data (3 tests)

21. **test_ami_data_source**
    - Verify data.aws_ami.ubuntu_latest:
      - Owner: Canonical (099720109477)
      - Filter: "ubuntu/images/hvm-ssd-gp3/ubuntu-questing-25.10*"
      - Filter: state = "available"
      - most_recent = true

22. **test_user_data_scripts_exist**
    - Verify user data script files exist:
      - tools/orchestrator-user-data.sh
      - tools/storage-node-user-data.sh
      - tools/client-node-user-data.sh
      - tools/jepsen-user-data.sh

23. **test_user_data_scripts_readable**
    - Verify each script:
      - Is readable (mode >= 0644)
      - Has shebang (#!/bin/bash or #!/usr/bin/env bash)
      - No bash syntax errors (`bash -n` passes)

---

### Group 6: Cost Estimation & Tagging (5 tests)

24. **test_cost_tags_present**
    - Verify cost tracking tags on all instances:
      - CostCenter = "Testing"
      - Agent = "A11"
      - Department = "Infrastructure"

25. **test_spot_instance_pricing**
    - Verify spot pricing calculation:
      - Storage nodes: 5 × $0.14/hr = $0.70/hr
      - Clients: 2 × $0.05/hr = $0.10/hr
      - Conduit: 1 × $0.015/hr = $0.015/hr
      - Jepsen: 1 × $0.10/hr = $0.10/hr
      - Daily estimate: ~$24 (spot)
      - Monthly estimate: ~$720 (spot)

26. **test_on_demand_equivalency**
    - Verify on-demand equivalent costs:
      - Same configuration on-demand: $60-80/day
      - Spot discount: 60-70% savings

27. **test_budget_constraints**
    - Verify configuration fits budget:
      - daily_budget_limit = 100 (USD)
      - cluster cost (~$24/day) < budget
      - budget_alert_threshold = 80%

28. **test_instance_count_within_budget**
    - Verify instance count:
      - Total: 10 nodes (1 persistent + 9 preemptible)
      - Cost fits $100/day budget
      - Spot savings > 50%

---

### Group 7: Outputs & Data Sources (4 tests)

29. **test_orchestrator_outputs**
    - Verify outputs defined:
      - output "orchestrator_id"
      - output "orchestrator_public_ip"
      - output "orchestrator_private_ip"

30. **test_storage_outputs**
    - Verify outputs:
      - output "storage_site_a_ids"
      - output "storage_site_a_private_ips"
      - output "storage_site_a_public_ips"
      - output "storage_site_b_ids"
      - output "storage_site_b_private_ips"
      - output "storage_site_b_public_ips"

31. **test_client_outputs**
    - Verify outputs:
      - output "fuse_client_id", "fuse_client_ip"
      - output "nfs_client_id", "nfs_client_ip"
      - output "conduit_id", "conduit_ip"
      - output "jepsen_controller_id", "jepsen_controller_public_ip"

32. **test_output_type_validation**
    - Verify output types:
      - IP addresses are strings
      - IDs are strings
      - Collections are lists where applicable

---

### Group 8: Integration & Production Readiness (4 tests)

33. **test_orchestrator_persistence**
    - Verify orchestrator is on-demand (not spot)
    - Has largest root volume (100 GB)
    - Has persistent tagging

34. **test_multi_site_topology**
    - Verify multi-site configuration:
      - Site A: 3 storage nodes (Raft quorum)
      - Site B: 2 storage nodes (replication)
      - Supports different AZs

35. **test_spot_instance_diversity**
    - Verify instance type diversity:
      - Storage: i4i.2xlarge (NVMe-optimized)
      - Compute: c7a.xlarge, c7a.2xlarge (CPU-optimized)
      - Relay: t3.medium (burstable, cost-effective)

36. **test_disaster_recovery_readiness**
    - Verify DR posture:
      - Volume encryption enabled
      - State backup location configured
      - Multi-AZ deployment possible

---

## Implementation Details

### File Locations

```
Source:
- tools/terraform/*.tf (all Terraform files)
- tools/cfs-terraform.sh (CLI wrapper)
- docs/A11-PHASE5-PLAN.md (architecture)
- docs/A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md (test spec)

Output:
- crates/claudefs-tests/src/terraform_infrastructure_tests.rs
```

### Test Dependencies

```toml
# Add to Cargo.toml if needed
[dev-dependencies]
tempfile = "3.8"
serde_json = "1.0"
```

### Test Pattern Examples

```rust
#[test]
fn test_terraform_syntax_valid() {
    // Run: terraform validate
    let output = Command::new("terraform")
        .args(&["validate"])
        .current_dir("tools/terraform")
        .output()
        .expect("Failed to run terraform validate");

    assert_eq!(output.status.code(), Some(0),
        "Terraform validation failed:\n{}",
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_terraform_variables_defined() {
    // Parse terraform files for variable references
    // Verify each is declared in variables.tf
    let tf_files = /* read all .tf files */;
    let declared_vars = parse_variables_tf();

    for referenced_var in extract_variable_references(&tf_files) {
        assert!(declared_vars.contains(&referenced_var),
            "Variable {} referenced but not declared", referenced_var);
    }
}
```

### Running Tests

```bash
# All Terraform infrastructure tests
cargo test --test terraform_infrastructure_tests -- --nocapture

# Single test
cargo test terraform_infrastructure_tests::test_terraform_syntax_valid

# With logging
RUST_LOG=debug cargo test terraform_infrastructure_tests
```

---

## Success Criteria

All tests MUST:
- ✅ Compile without errors
- ✅ Pass with 100% success rate
- ✅ Execute in <2 minutes total
- ✅ Zero clippy warnings
- ✅ Follow existing test conventions (naming, structure)
- ✅ Include documentation comments

---

## References

- docs/A11-PHASE5-PLAN.md
- docs/A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md
- tools/terraform/ — All infrastructure code
- CLAUDE.md — Project guidelines
- CHANGELOG.md — Progress tracking

---

**Prompt Created:** 2026-04-18 Session 10
**Ready for OpenCode:** Yes
**Model:** minimax-m2p5
**Target Output:** terraform_infrastructure_tests.rs (600+ LOC, 36 tests)
