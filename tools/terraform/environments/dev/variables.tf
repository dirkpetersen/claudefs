# Environment Variables for Dev

variable "aws_region" {}
variable "environment" {}
variable "project_tag" {}
variable "common_tags" {}

variable "vpc_cidr" {}
variable "create_vpc" { default = true }
variable "vpc_id" { default = "" }
variable "subnet_id" { default = "" }
variable "cluster_cidr" { default = "10.0.0.0/16" }
variable "ssh_cidr_blocks" { default = ["0.0.0.0/0"] }
variable "ssh_key_name" {}
variable "availability_zones" {}
variable "public_subnet_cidrs" {}
variable "private_subnet_cidrs" {}
variable "enable_nat_gateway" { default = true }

variable "orchestrator_instance_type" {}
variable "storage_instance_type" {}
variable "fuse_client_instance_type" {}
variable "nfs_client_instance_type" {}
variable "conduit_instance_type" {}
variable "jepsen_instance_type" {}

variable "site_a_min_size" {}
variable "site_a_desired_capacity" {}
variable "site_a_max_size" {}
variable "site_b_min_size" {}
variable "site_b_desired_capacity" {}
variable "site_b_max_size" {}

variable "scale_up_cpu_threshold" { default = 70 }
variable "scale_down_cpu_threshold" { default = 20 }
variable "scale_up_cooldown" { default = 300 }
variable "scale_down_cooldown" { default = 900 }
variable "enable_disk_scaling" { default = false }

variable "use_spot_instances" { default = true }
variable "on_demand_percentage" { default = 0 }

variable "enable_monitoring" { default = true }
variable "enable_cloudwatch_logs" { default = true }
variable "log_retention_days" { default = 7 }
variable "enable_sns_alerts" { default = false }
variable "enable_prometheus" { default = false }

variable "production_mode" { default = false }
variable "artifact_bucket_name" { default = "claudefs-artifacts" }
variable "data_bucket_name" { default = "claudefs-data" }

output "environment_info" {
  value = "Dev Environment - ClaudeFS Cluster"
}