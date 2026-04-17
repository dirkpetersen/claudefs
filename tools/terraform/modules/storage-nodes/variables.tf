# Storage Nodes Module Variables

variable "project_tag" {
  description = "Project tag for naming"
  type        = string
  default     = "claudefs"
}

variable "common_tags" {
  description = "Common tags for resources"
  type        = map(string)
  default     = {}
}

variable "ssh_key_name" {
  description = "EC2 key pair name"
  type        = string
}

variable "spot_iam_profile" {
  description = "IAM instance profile for spot instances"
  type        = string
}

variable "security_group_ids" {
  description = "Security group IDs for storage nodes"
  type        = list(string)
}

variable "subnet_ids" {
  description = "Subnet IDs for storage nodes"
  type        = list(string)
}

variable "user_data" {
  description = "User data script for storage nodes"
  type        = string
  default     = ""
}

variable "instance_type" {
  description = "Instance type for storage nodes"
  type        = string
  default     = "i4i.2xlarge"
}

# Root volume configuration
variable "root_volume_type" {
  description = "Root volume type"
  type        = string
  default     = "gp3"
}

variable "root_volume_size" {
  description = "Root volume size in GB"
  type        = number
  default     = 50
}

variable "root_volume_iops" {
  description = "Root volume IOPS"
  type        = number
  default     = 3000
}

variable "root_volume_throughput" {
  description = "Root volume throughput in MB/s"
  type        = number
  default     = 125
}

# Data volume configuration
variable "data_volume_type" {
  description = "Data volume type"
  type        = string
  default     = "gp3"
}

variable "data_volume_size" {
  description = "Data volume size in GB"
  type        = number
  default     = 1875
}

variable "data_volume_iops" {
  description = "Data volume IOPS"
  type        = number
  default     = 16000
}

variable "data_volume_throughput" {
  description = "Data volume throughput in MB/s"
  type        = number
  default     = 1000
}

# Site A (Raft quorum) scaling configuration
variable "site_a_min_size" {
  description = "Minimum number of storage nodes in Site A"
  type        = number
  default     = 3
}

variable "site_a_desired_capacity" {
  description = "Desired number of storage nodes in Site A"
  type        = number
  default     = 5
}

variable "site_a_max_size" {
  description = "Maximum number of storage nodes in Site A"
  type        = number
  default     = 10
}

# Site B (replication) scaling configuration
variable "site_b_min_size" {
  description = "Minimum number of storage nodes in Site B"
  type        = number
  default     = 1
}

variable "site_b_desired_capacity" {
  description = "Desired number of storage nodes in Site B"
  type        = number
  default     = 2
}

variable "site_b_max_size" {
  description = "Maximum number of storage nodes in Site B"
  type        = number
  default     = 5
}

# Scaling policies
variable "scale_cooldown" {
  description = "Default cooldown period in seconds"
  type        = number
  default     = 300
}

variable "scale_up_cooldown" {
  description = "Scale-up cooldown period in seconds"
  type        = number
  default     = 300
}

variable "scale_down_cooldown" {
  description = "Scale-down cooldown period in seconds"
  type        = number
  default     = 900
}

variable "scale_up_cpu_threshold" {
  description = "CPU threshold for scale-up (percentage)"
  type        = number
  default     = 70
}

variable "scale_down_cpu_threshold" {
  description = "CPU threshold for scale-down (percentage)"
  type        = number
  default     = 20
}

variable "scale_up_disk_threshold" {
  description = "Disk usage threshold for scale-up (percentage)"
  type        = number
  default     = 80
}

# Spot instance configuration
variable "use_spot_instances" {
  description = "Use spot instances for cost savings"
  type        = bool
  default     = true
}

variable "on_demand_percentage" {
  description = "Percentage of on-demand instances above base"
  type        = number
  default     = 0
}

variable "spot_instance_pools" {
  description = "Number of spot instance pools"
  type        = number
  default     = 2
}

# Advanced options
variable "enable_disk_scaling" {
  description = "Enable disk-based scaling triggers"
  type        = bool
  default     = false
}

variable "enable_scheduled_scaling" {
  description = "Enable scheduled scaling actions"
  type        = bool
  default     = false
}

variable "scheduled_recurrence" {
  description = "Cron schedule for scheduled scaling"
  type        = string
  default     = "cron(0 6 * * ? *)"
}