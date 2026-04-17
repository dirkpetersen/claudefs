# Client Nodes Module Variables

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

variable "subnet_id" {
  description = "Subnet ID for client nodes"
  type        = string
}

variable "security_group_ids" {
  description = "Security group IDs for client nodes"
  type        = list(string)
}

variable "fuse_client_instance_type" {
  description = "Instance type for FUSE client"
  type        = string
  default     = "c7a.xlarge"
}

variable "nfs_client_instance_type" {
  description = "Instance type for NFS client"
  type        = string
  default     = "c7a.xlarge"
}

variable "fuse_client_root_volume_size" {
  description = "Root volume size for FUSE client in GB"
  type        = number
  default     = 50
}

variable "nfs_client_root_volume_size" {
  description = "Root volume size for NFS client in GB"
  type        = number
  default     = 50
}

variable "fuse_client_user_data" {
  description = "User data script for FUSE client"
  type        = string
  default     = ""
}

variable "nfs_client_user_data" {
  description = "User data script for NFS client"
  type        = string
  default     = ""
}

variable "enable_fuse_client" {
  description = "Enable FUSE client node"
  type        = bool
  default     = true
}

variable "enable_nfs_client" {
  description = "Enable NFS client node"
  type        = bool
  default     = true
}

variable "assign_public_ip" {
  description = "Assign public IP to clients"
  type        = bool
  default     = true
}