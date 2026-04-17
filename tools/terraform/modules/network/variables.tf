# Network Module Variables

variable "project_tag" {
  description = "Project tag for naming resources"
  type        = string
  default     = "claudefs"
}

variable "common_tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}

variable "create_vpc" {
  description = "Create new VPC or use existing"
  type        = bool
  default     = true
}

variable "vpc_id" {
  description = "Existing VPC ID when create_vpc is false"
  type        = string
  default     = ""
}

variable "vpc_cidr" {
  description = "CIDR block for new VPC"
  type        = string
  default     = "10.0.0.0/16"
}

variable "availability_zones" {
  description = "Availability zones for subnets"
  type        = list(string)
  default     = []
}

variable "public_subnet_cidrs" {
  description = "CIDR blocks for public subnets"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24"]
}

variable "private_subnet_cidrs" {
  description = "CIDR blocks for private subnets"
  type        = list(string)
  default     = ["10.0.10.0/24", "10.0.11.0/24", "10.0.12.0/24"]
}

variable "enable_nat_gateway" {
  description = "Enable NAT Gateway for private subnets"
  type        = bool
  default     = true
}