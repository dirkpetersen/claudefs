# ClaudeFS Terraform Variables

variable "aws_region" {
  description = "AWS region for cluster deployment"
  type        = string
  default     = "us-west-2"
}

variable "environment" {
  description = "Environment name (dev, staging, prod)"
  type        = string
  default     = "dev"
}

variable "project_tag" {
  description = "Project tag for AWS resources"
  type        = string
  default     = "claudefs"
}

variable "vpc_id" {
  description = "VPC ID for cluster deployment"
  type        = string
  default     = "vpc-default"  # Use default VPC if not specified
}

variable "subnet_id" {
  description = "Subnet ID for instance placement"
  type        = string
  default     = ""  # Uses default subnet if not specified
}

variable "cluster_cidr" {
  description = "CIDR block for cluster internal communication"
  type        = string
  default     = "10.0.0.0/8"
}

variable "ssh_cidr_blocks" {
  description = "CIDR blocks allowed for SSH access"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "ssh_key_name" {
  description = "EC2 key pair name for SSH access"
  type        = string
}

variable "orchestrator_instance_type" {
  description = "Instance type for orchestrator node"
  type        = string
  default     = "c7a.2xlarge"
}

# Storage node configuration
variable "storage_instance_type" {
  description = "Instance type for storage nodes"
  type        = string
  default     = "i4i.2xlarge"
}

variable "storage_site_a_count" {
  description = "Number of storage nodes in site A (Raft quorum)"
  type        = number
  default     = 3
  validation {
    condition     = var.storage_site_a_count >= 1 && var.storage_site_a_count <= 10
    error_message = "Storage site A count must be between 1 and 10."
  }
}

variable "storage_site_b_count" {
  description = "Number of storage nodes in site B (replication)"
  type        = number
  default     = 2
  validation {
    condition     = var.storage_site_b_count >= 1 && var.storage_site_b_count <= 10
    error_message = "Storage site B count must be between 1 and 10."
  }
}

# Client node configuration
variable "fuse_client_instance_type" {
  description = "Instance type for FUSE client"
  type        = string
  default     = "c7a.xlarge"
}

variable "nfs_client_instance_type" {
  description = "Instance type for NFS/SMB client"
  type        = string
  default     = "c7a.xlarge"
}

# Conduit and Jepsen
variable "conduit_instance_type" {
  description = "Instance type for cloud conduit relay"
  type        = string
  default     = "t3.medium"
}

variable "jepsen_instance_type" {
  description = "Instance type for Jepsen controller"
  type        = string
  default     = "c7a.xlarge"
}

# IAM roles
variable "orchestrator_iam_profile" {
  description = "IAM instance profile for orchestrator"
  type        = string
  default     = "cfs-orchestrator-profile"
}

variable "spot_iam_profile" {
  description = "IAM instance profile for spot instances"
  type        = string
  default     = "cfs-spot-node-profile"
}

# Spot configuration
variable "use_spot_instances" {
  description = "Use spot instances for cost savings"
  type        = bool
  default     = true
}

variable "spot_max_price" {
  description = "Maximum price per hour for spot instances"
  type        = string
  default     = ""  # On-demand price if empty
}

# AWS Budget configuration
variable "daily_budget_limit" {
  description = "Daily AWS spending limit in USD"
  type        = number
  default     = 100
}

variable "budget_alert_threshold" {
  description = "Percentage of budget to trigger alerts (0-100)"
  type        = number
  default     = 80
}

# Tagging and naming
variable "common_tags" {
  description = "Common tags applied to all resources"
  type        = map(string)
  default = {
    Owner       = "claudefs-team"
    ManagedBy   = "terraform"
    CreatedDate = "2026-03-01"
  }
}

# VPC Configuration
variable "create_vpc" {
  description = "Create a new VPC instead of using existing"
  type        = bool
  default     = false
}

# Subnet configuration
variable "availability_zones" {
  description = "List of availability zones for deployment"
  type        = list(string)
  default     = ["us-west-2a", "us-west-2b", "us-west-2c"]
}

variable "public_subnet_cidrs" {
  description = "CIDR blocks for public subnets"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
}

variable "private_subnet_cidrs" {
  description = "CIDR blocks for private subnets"
  type        = list(string)
  default     = ["10.0.10.0/24", "10.0.11.0/24", "10.0.12.0/24"]
}

# Storage node specific availability zones
variable "storage_site_a_azs" {
  description = "Availability zones for Site A storage nodes"
  type        = list(string)
  default     = ["us-west-2a", "us-west-2b"]
}

variable "storage_site_b_azs" {
  description = "Availability zones for Site B storage nodes"
  type        = list(string)
  default     = ["us-west-2b", "us-west-2c"]
}

# Network
variable "vpc_cidr" {
  description = "CIDR block for VPC"
  type        = string
  default     = "10.0.0.0/16"
}

variable "enable_nat_gateway" {
  description = "Enable NAT Gateway for private subnets"
  type        = bool
  default     = true
}

# ASG Scaling Configuration
variable "site_a_min_size" {
  description = "Minimum size for Site A ASG"
  type        = number
  default     = 3
}

variable "site_a_desired_capacity" {
  description = "Desired capacity for Site A ASG"
  type        = number
  default     = 5
}

variable "site_a_max_size" {
  description = "Maximum size for Site A ASG"
  type        = number
  default     = 10
}

variable "site_b_min_size" {
  description = "Minimum size for Site B ASG"
  type        = number
  default     = 1
}

variable "site_b_desired_capacity" {
  description = "Desired capacity for Site B ASG"
  type        = number
  default     = 2
}

variable "site_b_max_size" {
  description = "Maximum size for Site B ASG"
  type        = number
  default     = 5
}

variable "scale_up_cpu_threshold" {
  description = "CPU threshold for scale up (percentage)"
  type        = number
  default     = 70
}

variable "scale_down_cpu_threshold" {
  description = "CPU threshold for scale down (percentage)"
  type        = number
  default     = 20
}

variable "scale_up_cooldown" {
  description = "Cooldown period for scale up in seconds"
  type        = number
  default     = 300
}

variable "scale_down_cooldown" {
  description = "Cooldown period for scale down in seconds"
  type        = number
  default     = 900
}

variable "enable_disk_scaling" {
  description = "Enable disk-based scaling triggers"
  type        = bool
  default     = false
}

variable "on_demand_percentage" {
  description = "Percentage of on-demand instances in ASG"
  type        = number
  default     = 0
}

# Monitoring
variable "enable_monitoring" {
  description = "Enable CloudWatch monitoring"
  type        = bool
  default     = true
}

variable "enable_cloudwatch_logs" {
  description = "Enable CloudWatch logs"
  type        = bool
  default     = true
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 7
}

variable "enable_sns_alerts" {
  description = "Enable SNS alerts"
  type        = bool
  default     = false
}

variable "enable_prometheus" {
  description = "Enable Prometheus integration"
  type        = bool
  default     = false
}

# State Backend
variable "create_state_backend" {
  description = "Create state backend resources"
  type        = bool
  default     = true
}

variable "state_bucket_name" {
  description = "Existing S3 bucket for state"
  type        = string
  default     = ""
}

variable "force_destroy_state" {
  description = "Force destroy S3 state bucket"
  type        = bool
  default     = false
}

# IAM
variable "production_mode" {
  description = "Use production IAM policies (restricted)"
  type        = bool
  default     = false
}

variable "artifact_bucket_name" {
  description = "S3 bucket for artifacts"
  type        = string
  default     = "claudefs-artifacts"
}

variable "data_bucket_name" {
  description = "S3 bucket for data tiering"
  type        = string
  default     = "claudefs-data"
}
