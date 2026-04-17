# A11 Phase 4 Block 1: Infrastructure-as-Code Enhancement for Production Deployment

## Current State
- Phase 3 Terraform modules exist at tools/terraform/ with basic infrastructure
- Orchestrator node, storage nodes, client nodes, conduit, and Jepsen nodes are provisioned
- Need to enhance for Phase 4: auto-scaling, modular structure, remote state backend

## Objectives
1. Create modular Terraform structure with proper separation of concerns
2. Add auto-scaling group for storage nodes with graceful node management
3. Enable remote state backend (S3 + DynamoDB) for production deployments
4. Add comprehensive CloudWatch/Prometheus integration hooks
5. Create environment-specific configurations (dev, staging, prod)
6. Add proper IAM roles and policies for all node types
7. Support multi-region deployment patterns

## Deliverables

### 1. Module Structure
Create modular Terraform layout:
- `tools/terraform/modules/claudefs-cluster/` - Main cluster VPC and security
- `tools/terraform/modules/storage-nodes/` - Storage node auto-scaling group
- `tools/terraform/modules/client-nodes/` - FUSE and NFS client provisioning
- `tools/terraform/modules/conduit/` - Cloud conduit relay node
- `tools/terraform/modules/monitoring/` - CloudWatch and monitoring setup
- `tools/terraform/modules/network/` - VPC, subnets, routing

### 2. Remote State Backend Configuration
- Create S3 bucket for Terraform state (encrypted, versioned)
- Create DynamoDB table for state locking
- Enable state file encryption at rest
- Set up bucket policies for secure access
- Document state file backup procedures

### 3. Auto-Scaling Configuration
- Create Launch Template for storage nodes (EBS optimization, NVMe passthrough tuning)
- Create Auto Scaling Group with:
  - Min: 3 nodes (Raft quorum in site A)
  - Desired: 5 nodes
  - Max: 10 nodes
  - Graceful scale-down with data rebalancing
  - CloudWatch metrics for scale triggers
- Scale-up: when average CPU > 70% for 5 min or disk > 80%
- Scale-down: when average CPU < 20% for 15 min and disk < 30%

### 4. IAM Roles and Policies
- Refine orchestrator IAM role with minimal required permissions:
  - EC2 full control for development (can be restricted further for production)
  - S3 for artifact storage
  - Secrets Manager for API keys
  - CloudWatch for metrics
  - Bedrock for AI orchestration
- Create spot node IAM role with reduced permissions:
  - S3 access for data tiering
  - CloudWatch agent permissions
  - Secrets Manager for encryption keys
  - EC2 describe for cluster discovery
- IAM policy documents stored as separate JSON files

### 5. Environment Configuration
- Create terraform.dev.tfvars for development
- Create terraform.prod.tfvars for production (smaller orchestrator, more storage nodes)
- Support multiple environments with -var-file flag
- Include cost controls and resource limits per environment

### 6. Monitoring Integration
- Add CloudWatch agent configuration to user-data scripts
- Prometheus scrape config templates
- Grafana provisioning hooks (data sources, dashboards)
- Alert rules for critical infrastructure metrics

### 7. Production Safety Features
- Enable API rate limiting on AWS API calls
- Add AWS Config rules for compliance
- CloudTrail logging for audit trail
- Backup and disaster recovery resources
- Multi-region support (can deploy to us-west-2 or us-east-1, etc.)

## Implementation Details

### File Structure to Create
```
tools/terraform/
├── environments/
│   ├── dev/
│   │   ├── main.tf
│   │   └── terraform.tfvars
│   ├── staging/
│   │   ├── main.tf
│   │   └── terraform.tfvars
│   └── prod/
│       ├── main.tf
│       └── terraform.tfvars
├── modules/
│   ├── claudefs-cluster/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   └── outputs.tf
│   ├── storage-nodes/
│   │   ├── main.tf (ASG with launch template)
│   │   ├── variables.tf
│   │   └── outputs.tf
│   ├── client-nodes/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   └── outputs.tf
│   ├── conduit/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   └── outputs.tf
│   ├── monitoring/
│   │   ├── main.tf (CloudWatch, Prometheus integration)
│   │   ├── variables.tf
│   │   └── outputs.tf
│   └── network/
│       ├── main.tf (VPC, subnets, routing)
│       ├── variables.tf
│       └── outputs.tf
├── main.tf (orchestrator, state backend)
├── variables.tf (global variables)
├── outputs.tf (aggregated outputs)
├── state-backend.tf (S3 + DynamoDB for state)
└── iam-roles.tf (IAM role definitions)
```

### Key Configuration Parameters

#### Storage Node Auto-Scaling Group
- Launch template with:
  - Ubuntu 25.10 AMI (kernel 6.20+)
  - EBS gp3 NVMe optimization settings
  - io_uring kernel parameters
  - Tuning: max_map_count, swappiness, vm.dirty_writeback_centisecs
  - Monitoring agent pre-installed
- Scaling policies:
  - Scale-up target: 70% CPU average
  - Scale-down target: 20% CPU average
  - Cooldown period: 5 minutes (scale-up), 15 minutes (scale-down)

#### CloudWatch Metrics
- Custom metrics namespace: claudefs/infrastructure
- Metrics to monitor:
  - ASG current capacity vs desired capacity
  - Storage node disk usage (alert at >85%)
  - Storage node memory usage (alert at >90%)
  - Cross-AZ traffic volume
  - Spot interruption rate

#### Security Groups
- Cluster internal: TCP 9400-9410 (RPC), UDP 9400-9410 (SWIM)
- Monitoring: TCP 9800 (Prometheus), TCP 3000 (Grafana)
- SSH: Limited to specified CIDR blocks
- Replication: TCP 5051-5052 (gRPC cross-site)

## Success Criteria
1. Terraform plan and apply succeeds without errors
2. Storage node ASG creates 3-5 nodes with proper tagging
3. All nodes join the cluster and pass health checks
4. CloudWatch metrics appear in AWS console within 5 minutes
5. Auto-scaling policies trigger and scale nodes correctly
6. Remote state backend operational and accessible
7. Environment separation (dev/staging/prod) functions correctly
8. Full cluster provisioned and operational in <15 minutes from `terraform apply`

## Dependencies
- AWS account with appropriate permissions
- Terraform >= 1.6 installed locally
- SSH key already created in AWS EC2
- IAM roles pre-created (cfs-orchestrator-role, cfs-spot-node-role)
- VPC and subnet already exist (or create new)

## Testing
After `terraform apply`:
1. SSH into orchestrator and verify ClaudeFS processes running
2. SSH into storage node and verify NVMe drives recognized
3. Run `cfs-test-orchestrator.sh` to verify cluster operational
4. Trigger scale-up by simulating load, watch ASG spin new nodes
5. Monitor CloudWatch dashboard for metrics appearing

## Rollback Procedure
- Run `terraform destroy` to remove all resources (except orchestrator if desired)
- State file backed up to S3, can restore previous version
- Manual cleanup of dangling resources via AWS console if needed
