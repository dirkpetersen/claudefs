# ClaudeFS Cluster Module Variables

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

variable "vpc_id" {
  description = "VPC ID for security groups"
  type        = string
}

variable "cluster_cidr" {
  description = "CIDR block for cluster internal communication"
  type        = string
  default     = "10.0.0.0/8"
}

variable "ssh_cidr_blocks" {
  description = "CIDR blocks for SSH access"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "grafana_cidr_blocks" {
  description = "CIDR blocks for Grafana access"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "replication_cidr_blocks" {
  description = "CIDR blocks for cross-site replication"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "ssh_key_name" {
  description = "EC2 key pair name"
  type        = string
}

variable "orchestrator_iam_profile" {
  description = "IAM instance profile for orchestrator"
  type        = string
}

variable "spot_iam_profile" {
  description = "IAM instance profile for spot instances"
  type        = string
}

variable "orchestrator_subnet_id" {
  description = "Subnet ID for orchestrator"
  type        = string
}

variable "orchestrator_instance_type" {
  description = "Instance type for orchestrator"
  type        = string
  default     = "c7a.2xlarge"
}

variable "orchestrator_root_volume_type" {
  description = "Root volume type for orchestrator"
  type        = string
  default     = "gp3"
}

variable "orchestrator_root_volume_size" {
  description = "Root volume size for orchestrator in GB"
  type        = number
  default     = 100
}

variable "orchestrator_user_data" {
  description = "User data script for orchestrator"
  type        = string
  default     = ""
}

variable "assign_orchestrator_public_ip" {
  description = "Assign public IP to orchestrator"
  type        = bool
  default     = true
}

variable "conduit_instance_type" {
  description = "Instance type for conduit"
  type        = string
  default     = "t3.medium"
}

variable "conduit_root_volume_size" {
  description = "Root volume size for conduit in GB"
  type        = number
  default     = 20
}

variable "conduit_user_data" {
  description = "User data script for conduit"
  type        = string
  default     = ""
}

variable "jepsen_instance_type" {
  description = "Instance type for Jepsen controller"
  type        = string
  default     = "c7a.xlarge"
}

variable "jepsen_user_data" {
  description = "User data script for Jepsen"
  type        = string
  default     = ""
}

variable "use_spot_instances" {
  description = "Use spot instances for conduit and Jepsen"
  type        = bool
  default     = true
}

variable "spot_max_price" {
  description = "Maximum spot price"
  type        = string
  default     = ""
}

variable "enable_conduit" {
  description = "Enable conduit node"
  type        = bool
  default     = true
}

variable "enable_jepsen" {
  description = "Enable Jepsen controller"
  type        = bool
  default     = false
}

variable "assign_conduit_public_ip" {
  description = "Assign public IP to conduit"
  type        = bool
  default     = true
}