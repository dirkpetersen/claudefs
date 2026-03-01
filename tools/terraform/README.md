# ClaudeFS Infrastructure as Code (Terraform)

This directory contains Terraform modules for provisioning and managing the ClaudeFS Phase 2 test cluster on AWS.

## Overview

The ClaudeFS infrastructure consists of:

- **1 Orchestrator** — persistent management node (c7a.2xlarge)
- **5 Storage servers** — 3 in Site A (Raft quorum), 2 in Site B (replication)
- **2 Client nodes** — FUSE client for POSIX testing, NFS/SMB client for multi-protocol testing
- **1 Cloud conduit** — gRPC relay for cross-site replication
- **1 Jepsen controller** — for distributed consistency testing

Total: **10 nodes**, estimated ~$20-26/day on spot instances.

## Prerequisites

1. **Terraform 1.6+**
   ```bash
   terraform version  # Should be >= 1.6
   ```

2. **AWS CLI configured** with appropriate credentials
   ```bash
   aws configure  # Set up AWS account credentials
   aws sts get-caller-identity  # Verify access
   ```

3. **EC2 Key Pair** created in your AWS region
   ```bash
   aws ec2 create-key-pair --key-name cfs-key --region us-west-2 --query 'KeyMaterial' --output text > ~/.ssh/cfs-key.pem
   chmod 600 ~/.ssh/cfs-key.pem
   ```

4. **IAM Roles** (can be created manually or via Terraform)
   - `cfs-orchestrator-profile` — for orchestrator node
   - `cfs-spot-node-profile` — for spot instances

   See `iam-policies/` directory for policy templates.

5. **VPC and Subnet** (optional — uses default if not specified)

## Quick Start

### 1. Initialize Terraform

```bash
cd tools/terraform
terraform init
```

This initializes Terraform and downloads the AWS provider.

### 2. Create Your Variables File

```bash
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars with your AWS account details and SSH key name
vim terraform.tfvars
```

Key variables to customize:
- `ssh_key_name` — your EC2 key pair name
- `aws_region` — your preferred AWS region (default: us-west-2)
- `orchestrator_iam_profile` — IAM role for orchestrator
- `spot_iam_profile` — IAM role for spot instances

### 3. Plan Deployment

```bash
terraform plan -out=tfplan
```

Review the plan to ensure it matches your expectations. Expect:
- 1 orchestrator instance (c7a.2xlarge, on-demand)
- 5 storage instances (i4i.2xlarge, spot)
- 4 client/test instances (mixed, spot)
- 1 security group

### 4. Apply Configuration

```bash
terraform apply tfplan
```

Terraform will provision all resources. This typically takes 5-10 minutes.

### 5. Verify Deployment

```bash
terraform output cluster_info
terraform output ssh_commands
```

Save the output for reference. The SSH commands are ready to use immediately.

## File Structure

```
terraform/
├── main.tf               # Main cluster configuration, security groups, orchestrator
├── variables.tf          # Variable definitions
├── storage-nodes.tf      # Storage server instances (Site A and Site B)
├── client-nodes.tf       # FUSE, NFS, conduit, and Jepsen nodes
├── outputs.tf            # Output values for cluster information
├── terraform.tfvars.example  # Example configuration (copy to terraform.tfvars)
└── README.md            # This file

iam-policies/            # (Parent directory)
├── orchestrator-role.json   # IAM policy for orchestrator
├── spot-node-role.json      # IAM policy for spot instances
```

## Configuration Options

### Cluster Sizing

Modify `terraform.tfvars` to change cluster size:

```hcl
storage_site_a_count = 3   # Raft quorum (minimum 1, maximum 10)
storage_site_b_count = 2   # Replication site
```

### Instance Types

For different workloads, customize instance types:

```hcl
orchestrator_instance_type = "c7a.2xlarge"  # Orchestrator
storage_instance_type      = "i4i.2xlarge"  # Storage (i4i has local NVMe)
fuse_client_instance_type  = "c7a.xlarge"   # FUSE client
jepsen_instance_type       = "c7a.xlarge"   # Jepsen controller
```

**Note:** i4i instances provide local NVMe storage. For development/testing, alternative instance types work but performance will differ.

### Cost Optimization

```hcl
use_spot_instances = true           # Enable spot instances (default)
spot_max_price     = ""             # Empty = on-demand price, or specify like "0.50"
daily_budget_limit = 100            # Max USD/day
```

Spot instances reduce costs by ~70%. Orchestrator is always on-demand (permanent).

### Network Configuration

```hcl
vpc_id        = ""            # Use default VPC if empty
subnet_id     = ""            # Use default subnet if empty
cluster_cidr  = "10.0.0.0/8"  # Internal cluster CIDR
ssh_cidr_blocks = [
  "203.0.113.0/24"            # Restrict SSH to your office IP
]
```

## Deployment Workflow

### Provision Cluster

```bash
# From tools/terraform directory
terraform apply -auto-approve
```

### Connect to Nodes

```bash
# Get SSH commands
terraform output ssh_commands

# Connect to orchestrator
ssh -i ~/.ssh/cfs-key.pem ec2-user@<orchestrator-ip>

# Connect to storage nodes
ssh -i ~/.ssh/cfs-key.pem ec2-user@<storage-a-1-ip>
```

### Cluster Initialization

Once all nodes are running (status checks complete ~2-3 minutes):

```bash
# SSH to orchestrator
ssh -i ~/.ssh/cfs-key.pem ec2-user@<orchestrator-ip>

# On orchestrator, run cluster bootstrap
cd /home/cfs/claudefs
cfs server \
  --cluster-id "claudefs-phase2" \
  --node-id "storage-a-1" \
  --listen 0.0.0.0:9400 \
  --data-dir /data/nvme0 \
  --seed-nodes "storage-a-1:9400,storage-a-2:9400,storage-a-3:9400" \
  --site-id "site-a"

# Other storage nodes join
cfs server join --seed storage-a-1:9400 --token $CLUSTER_SECRET
```

See `../deployment-runbook.md` for complete cluster initialization steps.

### Tear Down Resources

```bash
# Destroy all resources except orchestrator (useful for testing)
terraform destroy -auto-approve

# Or selectively remove resources
terraform destroy -auto-approve -target=aws_instance.storage_site_b
```

## Monitoring & Debugging

### Check Instance Status

```bash
# From orchestrator
aws ec2 describe-instances --region us-west-2 \
  --filters "Name=tag:project,Values=claudefs" \
  --query 'Reservations[].Instances[].[InstanceId,State.Name,PublicIpAddress,Tags[?Key==`Name`].Value|[0]]' \
  --output table
```

### View Logs

```bash
# Orchestrator bootstrap logs
tail -f /var/log/user-data.log

# CloudWatch logs (if enabled)
aws logs tail /aws/ec2/claudefs --follow
```

### Terraform State

```bash
# Inspect current state
terraform show

# Refresh state from AWS (useful after manual changes)
terraform refresh

# Backup state
cp terraform.tfstate terraform.tfstate.backup
```

## Troubleshooting

### Spot Instances Terminated

If spot instances are terminated unexpectedly:
1. Check spot price history: `aws ec2 describe-spot-price-history`
2. Increase `spot_max_price` or disable spot instances
3. Redeploy: `terraform apply`

### Instances Won't Start

Check security group ingress/egress rules and IAM policies:
```bash
aws ec2 describe-security-groups --group-ids <sg-id>
aws iam get-role-policy --role-name cfs-orchestrator-role --policy-name <policy-name>
```

### User Data Script Failures

SSH to an instance and check:
```bash
tail -100 /var/log/user-data.log
dmesg | tail -20
```

### VPC/Subnet Issues

Verify VPC and subnet exist and are accessible:
```bash
aws ec2 describe-vpcs
aws ec2 describe-subnets --subnet-ids <subnet-id>
```

## Cost Analysis

### Spot Instance Cost Estimation

Based on current AWS pricing (us-west-2):

| Node Type | Count | Instance Type | Spot Cost/hr | Daily Cost |
|-----------|-------|---------------|-------------|-----------|
| Orchestrator | 1 | c7a.2xlarge | $0.35 | $8.40 (on-demand) |
| Storage | 5 | i4i.2xlarge | $0.48 | $5.76 |
| Clients | 4 | c7a.xlarge | $0.17 | $1.63 |
| **Total** | **10** | **Mixed** | **~$1.35/hr** | **~$26/day** |

Estimate updated monthly with AWS pricing changes.

### Budget Enforcement

AWS Budgets (configured in `daily_budget_limit`):
- Alert at 80% of budget
- Hard limit at 100% (cost-monitor script auto-terminates spot instances)

Monitor via:
```bash
aws budgets describe-budget --account-id <account> --budget-name cfs-daily-100
```

## Advanced Topics

### State Management

By default, Terraform state is stored locally (`terraform.tfstate`). For team deployments, use remote state:

1. Create S3 bucket for state:
   ```bash
   aws s3api create-bucket --bucket claudefs-terraform-state --region us-west-2
   aws s3api put-bucket-versioning --bucket claudefs-terraform-state --versioning-configuration Status=Enabled
   ```

2. Create DynamoDB table for state locking:
   ```bash
   aws dynamodb create-table --table-name terraform-locks --attribute-definitions AttributeName=LockID,AttributeType=S --key-schema AttributeName=LockID,KeyType=HASH --provisioned-throughput ReadCapacityUnits=5,WriteCapacityUnits=5
   ```

3. Uncomment backend configuration in `main.tf`:
   ```hcl
   terraform {
     backend "s3" {
       bucket         = "claudefs-terraform-state"
       key            = "phase2/terraform.tfstate"
       region         = "us-west-2"
       encrypt        = true
       dynamodb_table = "terraform-locks"
     }
   }
   ```

4. Migrate state:
   ```bash
   terraform init  # Terraform will prompt to migrate local state
   ```

### Custom User Data

Modify bootstrap scripts to customize node setup:
- `orchestrator-user-data.sh` — orchestrator bootstrap
- `storage-node-user-data.sh` — storage node bootstrap
- `client-node-user-data.sh` — client node bootstrap

### Adding Custom Tags

```hcl
common_tags = {
  Environment = "dev"
  Team        = "platform"
  CostCenter  = "engineering"
}
```

All resources inherit these tags automatically.

## Integration with cfs-dev CLI

The Terraform configuration is complementary to the `cfs-dev` CLI. For most use cases:

```bash
# Use cfs-dev for cluster lifecycle management
cfs-dev up --phase 2         # Provisions and starts agents
cfs-dev status               # Shows cluster status
cfs-dev down                 # Tears down spot instances
cfs-dev destroy              # Tears down everything
```

For advanced infrastructure customization, directly use Terraform:

```bash
terraform plan
terraform apply
terraform destroy
```

## Support & Maintenance

For issues or questions:
1. Check logs: `tail -f /var/log/user-data.log`
2. Review AWS CloudFormation events (Terraform uses CloudFormation under the hood)
3. Open a GitHub issue: https://github.com/dirkpetersen/claudefs/issues

## Next Steps

- See `../deployment-runbook.md` for cluster initialization steps
- See `../phase2-infrastructure.md` for full Phase 2 deployment guide
- See `../management.md` for monitoring setup

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
**License:** MIT
