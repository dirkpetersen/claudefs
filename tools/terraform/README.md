# ClaudeFS Terraform Infrastructure

This directory contains Terraform configurations for provisioning and managing ClaudeFS infrastructure on AWS.

## Structure

```
tools/terraform/
├── modules/              # Reusable Terraform modules
│   ├── network/         # VPC, subnets, routing
│   ├── claudefs-cluster/    # Cluster-wide resources (security groups, bastion)
│   ├── storage-nodes/   # Storage node ASG with auto-scaling
│   ├── client-nodes/    # FUSE and NFS client nodes
│   └── monitoring/      # CloudWatch and monitoring setup
├── environments/        # Environment-specific configurations
│   ├── dev/            # Development environment
│   ├── staging/        # Staging environment
│   └── prod/           # Production environment
├── main.tf             # Root configuration
├── variables.tf        # Global variables
├── outputs.tf          # Global outputs
├── state-backend.tf    # S3 + DynamoDB backend configuration
└── README.md          # This file
```

## Quick Start

### Prerequisites

1. **AWS Account**: Active AWS account with appropriate permissions
2. **Terraform**: Version 1.6+
3. **AWS CLI**: Configured with credentials
4. **SSH Key**: Pre-created EC2 key pair (use `--key-name` parameter)

### Deployment

#### Development Environment

```bash
cd tools/terraform
terraform init
terraform plan -var-file="environments/dev/terraform.tfvars" -var="ssh_key_name=your-key-name"
terraform apply -var-file="environments/dev/terraform.tfvars" -var="ssh_key_name=your-key-name"
```

#### Staging Environment

```bash
terraform plan -var-file="environments/staging/terraform.tfvars" -var="ssh_key_name=your-key-name"
terraform apply -var-file="environments/staging/terraform.tfvars" -var="ssh_key_name=your-key-name"
```

#### Production Environment

```bash
terraform plan -var-file="environments/prod/terraform.tfvars" -var="ssh_key_name=your-key-name"
terraform apply -var-file="environments/prod/terraform.tfvars" -var="ssh_key_name=your-key-name"
```

## State Management

### Remote State Backend

Terraform state is stored in S3 with DynamoDB locking (created by `state-backend.tf`).

**First-time setup:**
1. Run `terraform apply` to create S3 bucket and DynamoDB table
2. Update `backend.tf` with bucket and table names
3. Run `terraform init` to migrate state to remote

**State file location:**
- S3 bucket: `claudefs-terraform-state-${ACCOUNT_ID}-${REGION}`
- State file key: `${ENVIRONMENT}/terraform.tfstate`

### State Locking

DynamoDB automatically handles state locking to prevent concurrent modifications.

## Infrastructure Components

### Network Module
- VPC with configurable CIDR
- Public and private subnets (multi-AZ)
- NAT gateway for private subnet egress
- Route tables and routing configuration
- Internet gateway

### Cluster Module
- Security groups for:
  - Internal cluster communication (TCP/UDP 9400-9410)
  - Prometheus monitoring (TCP 9800)
  - SSH access
  - gRPC replication (TCP 5051-5052)

### Storage Nodes Module
- **Launch Template**: Ubuntu 25.10 with optimized kernel
- **Auto Scaling Groups**:
  - Site A (Raft quorum): 3-5 nodes
  - Site B (replication): 2-4 nodes
- **Scaling Policies**:
  - CPU-based: Scale up at 70%, down at 20%
  - Disk-based: Scale up when usage >80%
- **CloudWatch Alarms**: Configured for scale-up/down triggers

### Client Nodes Module
- FUSE client for ClaudeFS mount
- NFS client for protocol gateway testing
- Both run POSIX test suites

## Environment Configurations

### Development (dev)
- Smaller orchestrator (c7a.2xlarge)
- 3 storage nodes (site A) + 2 (site B)
- Spot instances enabled for cost savings
- Budget limit: $100/day
- SSH allowed from anywhere (0.0.0.0/0)

### Staging (staging)
- Standard orchestrator (c7a.2xlarge)
- 5 storage nodes (site A) + 3 (site B)
- Spot instances enabled
- Budget limit: $150/day
- SSH restricted to private networks (10.0.0.0/8)

### Production (prod)
- Large orchestrator (c7a.4xlarge)
- 5 storage nodes per site (multi-AZ)
- On-demand instances for stability
- Budget limit: $500/day
- SSH restricted to private networks only
- Additional tags for cost allocation

## Customization

### Change Number of Storage Nodes

Edit the environment tfvars file:

```bash
# Increase site A to 7 nodes, site B to 5 nodes
storage_site_a_count = 7
storage_site_b_count = 5
```

Then apply:

```bash
terraform apply -var-file="environments/prod/terraform.tfvars" -var="ssh_key_name=your-key"
```

### Change Instance Types

```bash
# Upgrade storage nodes
storage_instance_type = "i4i.4xlarge"

# Upgrade client nodes
fuse_client_instance_type = "c7a.2xlarge"
nfs_client_instance_type = "c7a.2xlarge"
```

### Modify Scaling Policies

Edit the storage-nodes module variables:

```bash
cpu_scale_up_threshold = 80       # Scale up at 80% CPU
cpu_scale_down_threshold = 10     # Scale down at 10% CPU
disk_scale_up_threshold = 90      # Scale up at 90% disk
```

## Monitoring and Cost

### CloudWatch Dashboards

Monitoring is configured in the monitoring module with dashboards for:
- Cluster health
- Storage node capacity
- Auto-scaling activity
- Cost tracking

### Cost Estimates

| Environment | Estimated Daily Cost | Spot Savings |
|------------|---------------------|-------------|
| dev        | $10-15              | 60-70%      |
| staging    | $25-35              | 50-60%      |
| prod       | $80-100             | N/A (on-demand) |

## Troubleshooting

### Terraform Init Fails

Ensure AWS credentials are configured:
```bash
aws sts get-caller-identity
```

### ASG Fails to Create

Check security group rules allow required ports:
- TCP 9400-9410 (internal RPC)
- TCP 5051-5052 (replication)

### Nodes Won't Join Cluster

1. SSH into a node and check logs:
   ```bash
   journalctl -u cfs-storage
   tail -f /var/log/cfs/storage.log
   ```

2. Verify security groups allow communication between nodes

3. Check Raft quorum has majority:
   ```bash
   cfs status
   ```

### Scaling Policies Not Triggering

1. Verify CloudWatch metrics are being sent:
   ```bash
   aws cloudwatch get-metric-statistics \
     --namespace AWS/EC2 \
     --metric-name CPUUtilization \
     --start-time 2026-04-17T00:00:00Z \
     --end-time 2026-04-18T00:00:00Z \
     --period 300 \
     --statistics Average
   ```

2. Check alarm configuration:
   ```bash
   aws autoscaling describe-auto-scaling-groups \
     --auto-scaling-group-names claudefs-storage-site-a
   ```

## Cleanup

### Destroy Development Environment

```bash
terraform destroy -var-file="environments/dev/terraform.tfvars" -var="ssh_key_name=your-key"
```

### Destroy All Environments

```bash
# WARNING: This is destructive!
for env in dev staging prod; do
  terraform destroy -var-file="environments/$env/terraform.tfvars" -var="ssh_key_name=your-key"
done
```

## Best Practices

1. **State Management**: Always keep state files backed up in S3
2. **Secrets**: Never commit sensitive data (API keys, passwords) to Git
3. **Testing**: Use `terraform plan` before applying changes
4. **Versioning**: Tag releases in Git for reproducible infrastructure
5. **Documentation**: Update this README when making structural changes

## References

- [Terraform AWS Provider](https://registry.terraform.io/providers/hashicorp/aws/latest)
- [ClaudeFS Architecture Decisions](../../docs/decisions.md)
- [ClaudeFS Deployment Guide](../../docs/DEPLOYMENT-GUIDE.md)
