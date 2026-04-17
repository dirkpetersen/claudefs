# ClaudeFS Environment Main - Uses modular Terraform structure

terraform {
  required_version = ">= 1.6"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  backend "s3" {
    bucket         = "claudefs-terraform-state"
    key            = "dev/terraform.tfstate"
    region         = "us-west-2"
    encrypt        = true
    dynamodb_table = "claudefs-terraform-locks"
  }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = var.common_tags
  }
}

module "network" {
  source = "../modules/network"

  project_tag          = var.project_tag
  common_tags          = var.common_tags
  create_vpc           = var.create_vpc
  vpc_id               = var.vpc_id
  vpc_cidr             = var.vpc_cidr
  availability_zones   = var.availability_zones
  public_subnet_cidrs  = var.public_subnet_cidrs
  private_subnet_cidrs = var.private_subnet_cidrs
  enable_nat_gateway   = var.enable_nat_gateway
}

module "iam_roles" {
  source = "../../."

  project_tag        = var.project_tag
  common_tags        = var.common_tags
  production_mode    = var.production_mode
  artifact_bucket_name = var.artifact_bucket_name
  data_bucket_name   = var.data_bucket_name
}

module "claudefs_cluster" {
  source = "../modules/claudefs-cluster"

  project_tag           = var.project_tag
  common_tags           = var.common_tags
  vpc_id                = module.network.vpc_id
  cluster_cidr          = var.cluster_cidr
  ssh_cidr_blocks       = var.ssh_cidr_blocks
  ssh_key_name          = var.ssh_key_name
  orchestrator_iam_profile = module.iam_roles.orchestrator_instance_profile
  spot_iam_profile      = module.iam_roles.spot_node_instance_profile
  orchestrator_subnet_id = var.create_vpc ? module.network.public_subnet_ids[0] : var.subnet_id
  orchestrator_instance_type = var.orchestrator_instance_type
  orchestrator_root_volume_type = "gp3"
  orchestrator_root_volume_size = 100
  enable_conduit        = true
  conduit_instance_type = var.conduit_instance_type
  enable_jepsen         = false
  jepsen_instance_type  = var.jepsen_instance_type
  use_spot_instances    = var.use_spot_instances
  assign_orchestrator_public_ip = true
  assign_conduit_public_ip = true
}

module "storage_nodes" {
  source = "../modules/storage-nodes"

  project_tag              = var.project_tag
  common_tags              = var.common_tags
  ssh_key_name             = var.ssh_key_name
  spot_iam_profile         = module.iam_roles.spot_node_instance_profile
  security_group_ids       = [module.claudefs_cluster.security_group_id]
  subnet_ids               = var.create_vpc ? module.network.private_subnet_ids : [var.subnet_id]
  instance_type            = var.storage_instance_type
  root_volume_type         = "gp3"
  root_volume_size         = 50
  data_volume_type         = "gp3"
  data_volume_size         = 1875
  site_a_min_size          = var.site_a_min_size
  site_a_desired_capacity  = var.site_a_desired_capacity
  site_a_max_size          = var.site_a_max_size
  site_b_min_size          = var.site_b_min_size
  site_b_desired_capacity  = var.site_b_desired_capacity
  site_b_max_size          = var.site_b_max_size
  scale_cooldown           = 300
  scale_up_cooldown        = var.scale_up_cooldown
  scale_down_cooldown      = var.scale_down_cooldown
  scale_up_cpu_threshold   = var.scale_up_cpu_threshold
  scale_down_cpu_threshold = var.scale_down_cpu_threshold
  enable_disk_scaling      = var.enable_disk_scaling
  use_spot_instances       = var.use_spot_instances
  on_demand_percentage     = var.on_demand_percentage
}

module "client_nodes" {
  source = "../modules/client-nodes"

  project_tag           = var.project_tag
  common_tags           = var.common_tags
  ssh_key_name          = var.ssh_key_name
  spot_iam_profile      = module.iam_roles.spot_node_instance_profile
  subnet_id             = var.create_vpc ? module.network.public_subnet_ids[0] : var.subnet_id
  security_group_ids    = [module.claudefs_cluster.security_group_id]
  fuse_client_instance_type  = var.fuse_client_instance_type
  nfs_client_instance_type   = var.nfs_client_instance_type
  enable_fuse_client    = true
  enable_nfs_client     = true
  assign_public_ip      = true
}

module "monitoring" {
  source = "../modules/monitoring"

  project_tag           = var.project_tag
  environment           = var.environment
  common_tags           = var.common_tags
  cluster_cidr          = var.cluster_cidr
  orchestrator_instance_id = module.claudefs_cluster.orchestrator_id
  storage_asg_name      = module.storage_nodes.site_a_asg_name
  enable_monitoring     = var.enable_monitoring
  enable_cloudwatch_logs = var.enable_cloudwatch_logs
  log_retention_days    = var.log_retention_days
  enable_sns_alerts     = var.enable_sns_alerts
}

output "cluster_info" {
  description = "Cluster deployment information"
  value = {
    orchestrator = {
      id         = module.claudefs_cluster.orchestrator_id
      public_ip  = module.claudefs_cluster.orchestrator_public_ip
      private_ip = module.claudefs_cluster.orchestrator_private_ip
    }
    storage_site_a = {
      asg_name   = module.storage_nodes.site_a_asg_name
      min_size   = module.storage_nodes.site_a_min_size
      max_size   = module.storage_nodes.site_a_max_size
      desired    = module.storage_nodes.site_a_desired_capacity
    }
    storage_site_b = {
      asg_name   = module.storage_nodes.site_b_asg_name
      min_size   = module.storage_nodes.site_b_min_size
      max_size   = module.storage_nodes.site_b_max_size
      desired    = module.storage_nodes.site_b_desired_capacity
    }
    client_nodes = {
      fuse = {
        id       = module.client_nodes.fuse_client_id
        public_ip = module.client_nodes.fuse_client_public_ip
      }
      nfs = {
        id       = module.client_nodes.nfs_client_id
        public_ip = module.client_nodes.nfs_client_public_ip
      }
    }
    network = {
      vpc_id          = module.network.vpc_id
      public_subnets  = module.network.public_subnet_ids
      private_subnets = module.network.private_subnet_ids
    }
    security_group = module.claudefs_cluster.security_group_id
    monitoring      = module.monitoring.cloudwatch_log_group_name
  }
}

output "ssh_commands" {
  description = "SSH commands for connecting to nodes"
  value = {
    orchestrator = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ubuntu@${module.claudefs_cluster.orchestrator_public_ip}"
    storage      = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ubuntu@<storage-node-ip>"
    fuse_client  = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ubuntu@${module.client_nodes.fuse_client_public_ip}"
    nfs_client   = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ubuntu@${module.client_nodes.nfs_client_public_ip}"
  }
}