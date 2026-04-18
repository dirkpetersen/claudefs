# A11: Phase 5 Block 1 — Terraform Infrastructure Tests

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING — Ready for OpenCode
**Test Target:** 36 tests covering Terraform validation and infrastructure provisioning

---

## Test Scope

### Test Module: terraform_infrastructure_tests.rs (600+ LOC)

Located in: `crates/claudefs-tests/src/terraform_infrastructure_tests.rs`

---

## Test Groups (36 tests total)

### Group 1: Terraform Syntax & Validation (6 tests)

1. **test_terraform_syntax_valid**
   - Check: `terraform validate` exits with 0
   - Scope: All .tf files in tools/terraform/
   - Assertion: All files validate cleanly

2. **test_terraform_format_consistent**
   - Check: `terraform fmt -check` finds no formatting issues
   - Scope: All .tf files
   - Assertion: All files are properly formatted

3. **test_terraform_variables_defined**
   - Check: All referenced variables in .tf files are declared in variables.tf
   - Scope: Parse all variable references
   - Assertion: No undefined variable references

4. **test_terraform_outputs_valid**
   - Check: All outputs are syntactically correct
   - Scope: outputs.tf, jepsen-nodes.tf
   - Assertion: All outputs can be evaluated without errors

5. **test_terraform_providers_configured**
   - Check: AWS provider is configured with required version
   - Scope: main.tf provider block
   - Assertion: Provider version is >= 5.0

6. **test_terraform_backend_configured**
   - Check: S3 backend is properly configured for remote state
   - Scope: state-backend.tf
   - Assertion: S3 bucket and DynamoDB table specified

---

### Group 2: Resource Definitions (6 tests)

7. **test_orchestrator_instance_defined**
   - Check: aws_instance.orchestrator exists with correct config
   - Validation:
     - instance_type = c7a.2xlarge
     - key_name is set
     - root_block_device.volume_type = gp3
     - associate_public_ip_address = true
   - Assertion: All required fields present

8. **test_storage_nodes_configuration**
   - Check: Both storage_site_a and storage_site_b defined
   - Validation:
     - Site A count: default 3, min 1, max 10
     - Site B count: default 2, min 1, max 10
     - instance_type = i4i.2xlarge
     - spot price handling enabled
   - Assertion: Both sites properly configured

9. **test_client_nodes_configuration**
   - Check: FUSE, NFS, conduit, Jepsen clients all defined
   - Validation:
     - FUSE: c7a.xlarge, 50GB root
     - NFS: c7a.xlarge, 50GB root
     - Conduit: t3.medium, 20GB root
     - Jepsen: c7a.xlarge, 50GB + 100GB volumes
   - Assertion: All client configurations correct

10. **test_security_group_rules**
    - Check: Security group has required ingress/egress rules
    - Validation:
      - RPC ports: 9400-9410 (TCP + UDP)
      - Prometheus: 9800 (TCP)
      - SSH: 22 (TCP)
      - Egress: Allow all
    - Assertion: All expected rules present

11. **test_instance_iam_profiles**
    - Check: All instances have IAM instance profiles
    - Validation:
      - orchestrator: cfs-orchestrator-profile
      - storage/client nodes: cfs-spot-node-profile
    - Assertion: All profiles assigned correctly

12. **test_instance_tagging**
    - Check: All instances have required tags
    - Validation:
      - Name tag present
      - Role tag (orchestrator/storage/client/jepsen)
      - Site tag for storage nodes (A/B)
      - Cost tags (CostCenter, Agent, Department)
    - Assertion: All required tags present

---

### Group 3: Storage & Volume Configuration (4 tests)

13. **test_storage_volume_encryption**
    - Check: All EBS volumes are encrypted
    - Scope: root_block_device and ebs_block_device across all instances
    - Validation:
      - encrypted = true for all volumes
      - volume_type = gp3 (except where specified)
    - Assertion: All volumes encrypted

14. **test_storage_volume_sizing**
    - Check: Volume sizes are appropriate
    - Validation:
      - Orchestrator root: 100 GB
      - Storage nodes root: 50 GB
      - Storage nodes data: 1875 GB (i4i physical)
      - Client nodes: 50 GB
      - Jepsen: 50GB root + 100GB results
      - Conduit: 20 GB
    - Assertion: All sizes match specification

15. **test_storage_delete_on_termination**
    - Check: Delete-on-termination flags correct
    - Validation:
      - Root volumes: delete_on_termination = true
      - Data volumes: delete_on_termination = true
    - Assertion: No orphaned volumes on termination

16. **test_storage_spot_price_configuration**
    - Check: Spot price configuration for cost savings
    - Validation:
      - use_spot_instances = true
      - spot_max_price = "" (on-demand if empty)
      - instance_interruption_behavior = "terminate"
    - Assertion: Spot pricing properly configured

---

### Group 4: Networking & Security (4 tests)

17. **test_vpc_configuration**
    - Check: VPC settings are valid
    - Validation:
      - VPC ID defaults to "vpc-default" if not specified
      - CIDR: 10.0.0.0/16 for VPC
      - Public subnets: 10.0.1-3.0/24
      - Private subnets: 10.0.10-12.0/24
    - Assertion: CIDR blocks don't overlap

18. **test_security_group_ingress_rules**
    - Check: Ingress rules allow only required traffic
    - Validation:
      - ClaudeFS RPC (9400-9410) from cluster_cidr
      - Prometheus (9800) from cluster_cidr
      - SSH (22) from ssh_cidr_blocks
      - No unnecessary egress restrictions
    - Assertion: Security posture correct

19. **test_security_group_egress_rules**
    - Check: Egress allows necessary outbound traffic
    - Validation:
      - AWS API calls (S3, CloudWatch, etc.)
      - All outbound traffic allowed (protocol -1)
      - No egress denials
    - Assertion: Instances can reach AWS services

20. **test_instance_networking**
    - Check: All instances have network configuration
    - Validation:
      - associate_public_ip_address = true for all
      - subnet_id specified
      - vpc_security_group_ids includes cluster SG
    - Assertion: Network connectivity configured

---

### Group 5: AMI & User Data (3 tests)

21. **test_ami_data_source**
    - Check: AMI data source correctly filters
    - Validation:
      - Owner: Canonical (099720109477)
      - Name pattern: ubuntu/images/hvm-ssd-gp3/ubuntu-questing-25.10*
      - State: available
    - Assertion: AMI lookup is correct

22. **test_user_data_scripts_exist**
    - Check: User data script files exist
    - Validation:
      - orchestrator-user-data.sh exists
      - storage-node-user-data.sh exists
      - client-node-user-data.sh exists
      - jepsen-user-data.sh exists
    - Assertion: All scripts present

23. **test_user_data_scripts_readable**
    - Check: User data scripts are readable and executable
    - Validation:
      - File permissions: at least 0644
      - Shebang line present
      - No syntax errors (bash -n)
    - Assertion: Scripts are valid bash

---

### Group 6: Cost Estimation & Tagging (5 tests)

24. **test_cost_tags_present**
    - Check: All instances have cost tracking tags
    - Validation:
      - CostCenter tag = "Testing"
      - Agent tag = "A11"
      - Department tag = "Infrastructure"
    - Assertion: Cost tagging complete

25. **test_spot_instance_pricing**
    - Check: Spot instance pricing calculation
    - Calculation:
      - 5 storage nodes: 5 × $0.14/hr = $0.70/hr
      - 2 clients: 2 × $0.05/hr = $0.10/hr
      - 1 conduit: 1 × $0.015/hr = $0.015/hr
      - 1 jepsen: 1 × $0.10/hr = $0.10/hr
      - Total: ~$1.015/hr (~$24/day, ~$720/month)
    - Assertion: Cost estimate accurate

26. **test_on_demand_equivalency**
    - Check: On-demand equivalent costs
    - Calculation:
      - Same configuration on-demand ≈ $60-80/day
    - Assertion: 60-70% spot discount verified

27. **test_budget_constraints**
    - Check: Configuration respects daily budget
    - Validation:
      - daily_budget_limit = 100 (USD)
      - daily_cluster_cost ≈ $24 (spot) well under limit
      - budget_alert_threshold = 80%
    - Assertion: Budget constraints sufficient

28. **test_instance_count_within_budget**
    - Check: Total instance count within cost targets
    - Validation:
      - 1 orchestrator (on-demand, persistent)
      - 9 preemptible nodes (8 storage + clients, 1 jepsen)
      - Total: 10 nodes within $100/day budget
    - Assertion: Instance count validated

---

### Group 7: Outputs & Data Sources (4 tests)

29. **test_orchestrator_outputs**
    - Check: Orchestrator outputs are defined
    - Validation:
      - orchestrator_id
      - orchestrator_public_ip
      - orchestrator_private_ip
    - Assertion: All outputs defined and referenced

30. **test_storage_outputs**
    - Check: Storage node outputs
    - Validation:
      - storage_site_a_ids, storage_site_a_private_ips, storage_site_a_public_ips
      - storage_site_b_ids, storage_site_b_private_ips, storage_site_b_public_ips
    - Assertion: Site-aware outputs present

31. **test_client_outputs**
    - Check: Client and test node outputs
    - Validation:
      - fuse_client_id, fuse_client_ip
      - nfs_client_id, nfs_client_ip
      - conduit_id, conduit_ip
      - jepsen_controller_id, jepsen_controller_public_ip
    - Assertion: All client outputs defined

32. **test_output_type_validation**
    - Check: Output types are correct
    - Validation:
      - IPs are strings
      - IDs are strings
      - Collections are lists/maps where applicable
    - Assertion: Output types correct

---

### Group 8: Integration & Production Readiness (4 tests)

33. **test_orchestrator_persistence**
    - Check: Orchestrator is configured for persistence
    - Validation:
      - No spot price configured (on-demand always)
      - Persistent tagging
      - Large root volume (100 GB)
    - Assertion: Orchestrator persistent

34. **test_multi_site_topology**
    - Check: Multi-site configuration for replication
    - Validation:
      - Site A: 3 nodes (Raft quorum)
      - Site B: 2 nodes (replication site)
      - Different AZs for fault tolerance
    - Assertion: Multi-site topology validated

35. **test_spot_instance_diversity**
    - Check: Spot instance types are diverse
    - Validation:
      - Storage: i4i.2xlarge (NVMe)
      - Compute: c7a.xlarge, c7a.2xlarge (CPU-optimized)
      - Relay: t3.medium (burstable)
    - Assertion: Instance diversity good

36. **test_disaster_recovery_readiness**
    - Check: Infrastructure supports failure scenarios
    - Validation:
      - Backups enabled (volumes encrypted, snapshots ready)
      - Multi-AZ deployment possible (variables support it)
      - State backup location configured
    - Assertion: DR posture acceptable

---

## Test Implementation

### Test File Structure

```rust
#[cfg(test)]
mod terraform_infrastructure_tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    // Group 1: Syntax tests
    #[test]
    fn test_terraform_syntax_valid() {
        // Read all .tf files in tools/terraform/
        // Run: terraform validate
        // Assert: exit code 0
    }

    // ... (35 more tests)

    // Helper functions
    fn read_terraform_file(path: &str) -> String { ... }
    fn run_terraform_command(args: &[&str]) -> Result<String, String> { ... }
    fn parse_terraform_json(content: &str) -> serde_json::Value { ... }
}
```

### Build & Test Commands

```bash
# Run all Terraform infrastructure tests
cargo test --test terraform_infrastructure_tests -- --nocapture

# Run specific test group
cargo test terraform_infrastructure_tests::test_terraform_syntax_valid

# Run with logging
RUST_LOG=debug cargo test terraform_infrastructure_tests
```

---

## Success Criteria

- ✅ All 36 tests compile without errors
- ✅ All 36 tests pass (100% pass rate)
- ✅ Zero clippy warnings in new test code
- ✅ Test execution time <2 minutes total
- ✅ Documentation complete (this file)
- ✅ Terraform state backup verified
- ✅ Cost estimates accurate (±5%)

---

## OpenCode Delegation

**Ready for OpenCode implementation with minimax-m2p5**

Input: `docs/A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md` (this file)

Expected output:
- `crates/claudefs-tests/src/terraform_infrastructure_tests.rs` (600+ LOC)
- 36 passing tests
- Zero clippy warnings
- Integration with existing test suite

---

## References

- docs/A11-PHASE5-PLAN.md — Full Phase 5 architecture
- tools/terraform/ — Terraform source files
- tools/cfs-terraform.sh — Terraform CLI wrapper
- CHANGELOG.md — Phase 5 progress tracking

---

**Document:** A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md
**Created:** 2026-04-18 Session 10
**Status:** 🟡 PLANNING — Ready for OpenCode
