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
