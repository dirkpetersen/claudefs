# A11 Phase 4 Block 1: Continue Terraform Module Generation

## Status
OpenCode has created:
- ✅ network module (VPC, subnets, routing)
- ✅ claudefs-cluster module (partial)

## Need to Create (Critical)

### 1. Storage Nodes Module with Auto-Scaling Group (MOST IMPORTANT)

Create `tools/terraform/modules/storage-nodes/main.tf` with:

**Storage Node Launch Template:**
- AMI: Ubuntu 25.10 (kernel 6.20+) via data source (aws_ami ubuntu-questing)
- Instance type: configurable (default i4i.2xlarge)
- IAM instance profile: cfs-spot-node-profile
- EBS optimization enabled
- Root volume: 500GB gp3, encrypted
- User data: storage-node-user-data.sh from tools/
- Tags: Name, Role=storage-site-A (or site-B)
- Security groups: cluster SG + internal monitoring SG

**Storage Node ASG (Site A - Raft Quorum):**
- Min capacity: 3
- Desired capacity: 3
- Max capacity: 5
- Availability zones: 2+ for HA
- Termination policy: OldestInstance (graceful)
- Tags: site=A, role=storage, propagate_at_launch=true

**Storage Node ASG (Site B - Replication):**
- Min capacity: 2
- Desired capacity: 2
- Max capacity: 4
- Availability zones: different zone from site A if possible
- Tags: site=B, role=storage, propagate_at_launch=true

**CloudWatch Alarms for Scaling:**
- CPU Utilization (scale-up at 70%, scale-down at 20%)
- EBS Volume Queue Length (scale-up at high values)
- Disk usage from CloudWatch agent (scale up when disk >80%)

**Outputs:**
- asg_site_a_name
- asg_site_b_name
- launch_template_id
- storage_node_ips (from ASG)

### 2. State Backend Configuration (S3 + DynamoDB)

Create `tools/terraform/state-backend.tf` (in root):

**S3 Bucket for State:**
- Bucket name: claudefs-terraform-state-${aws_account_id}-${region}
- Versioning: enabled
- Encryption: AES256
- Block public access: all enabled
- Server-side encryption: enabled
- Bucket policy: restrict to IAM role

**DynamoDB Table for Locking:**
- Table name: claudefs-terraform-locks
- Billing mode: PAY_PER_REQUEST (low usage, pay as you go)
- Primary key: LockID (string)
- TTL: enabled on locks (for safety)
- DynamoDB policy: restrict to IAM role

**Backend Configuration Template:**
Write `tools/terraform/backend.tf.template`:
```hcl
terraform {
  backend "s3" {
    bucket         = "claudefs-terraform-state-${ACCOUNT_ID}-${REGION}"
    key            = "${ENVIRONMENT}/terraform.tfstate"
    region         = "${REGION}"
    encrypt        = true
    dynamodb_table = "claudefs-terraform-locks"
  }
}
```

### 3. Environment Configurations

Create:
- `tools/terraform/environments/dev/terraform.tfvars`
- `tools/terraform/environments/staging/terraform.tfvars`
- `tools/terraform/environments/prod/terraform.tfvars`

Each should include:
- environment name
- aws_region
- orchestrator_instance_type
- storage_site_a_count
- storage_site_b_count
- use_spot_instances
- daily_budget_limit
- common_tags

### 4. Global Variables Update

Update `tools/terraform/variables.tf` to include:
- availability_zones (list, for multi-AZ deployment)
- create_vpc (bool, to reuse existing VPC)
- public_subnet_cidrs (list)
- private_subnet_cidrs (list)
- storage_site_a_azs (list, specific AZs for site A)
- storage_site_b_azs (list, specific AZs for site B)

## Implementation Order
1. Storage nodes module (ASG, launch template, alarms)
2. State backend (S3, DynamoDB)
3. Global variables update
4. Environment configurations (dev, staging, prod)

## Success Criteria
- Terraform validate succeeds
- ASG configurations allow 3-5 node scaling
- State backend S3 bucket is encrypted and versioned
- Environment configs can be used with: terraform apply -var-file="environments/dev/terraform.tfvars"
