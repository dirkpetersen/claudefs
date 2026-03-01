# ClaudeFS Phase 2 Test Cluster Infrastructure
# AWS provider configuration and main cluster setup

terraform {
  required_version = ">= 1.6"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  # Uncomment to use remote state
  # backend "s3" {
  #   bucket         = "claudefs-terraform-state"
  #   key            = "phase2/terraform.tfstate"
  #   region         = "us-west-2"
  #   encrypt        = true
  #   dynamodb_table = "terraform-locks"
  # }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project     = var.project_tag
      Phase       = "2"
      Environment = var.environment
      Terraform   = "true"
    }
  }
}

# Data source for latest Ubuntu 25.10 AMI
data "aws_ami" "ubuntu_latest" {
  most_recent = true
  owners      = ["099720109477"] # Canonical

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-questing-25.10-amd64-server-*"]
  }

  filter {
    name   = "state"
    values = ["available"]
  }
}

# Security group for cluster internal communication
resource "aws_security_group" "cluster" {
  name_prefix = "${var.project_tag}-cluster-"
  description = "ClaudeFS cluster security group"
  vpc_id      = var.vpc_id

  # Internal cluster communication
  ingress {
    from_port   = 9400
    to_port     = 9410
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "ClaudeFS internal RPC"
  }

  ingress {
    from_port   = 9400
    to_port     = 9410
    protocol    = "udp"
    cidr_blocks = [var.cluster_cidr]
    description = "ClaudeFS SWIM gossip"
  }

  # Monitoring
  ingress {
    from_port   = 9800
    to_port     = 9800
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "Prometheus metrics"
  }

  # SSH
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = var.ssh_cidr_blocks
    description = "SSH access"
  }

  # Egress: allow all
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Allow all outbound traffic"
  }

  tags = {
    Name = "${var.project_tag}-cluster-sg"
  }
}

# Orchestrator instance (persistent)
resource "aws_instance" "orchestrator" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.orchestrator_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.orchestrator_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster.id]
  associate_public_ip_address = true

  # Root volume
  root_block_device {
    volume_type           = "gp3"
    volume_size           = 100
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-orchestrator-root"
    }
  }

  # User data script
  user_data = file("${path.module}/../orchestrator-user-data.sh")

  tags = {
    Name = "${var.project_tag}-orchestrator"
    Role = "orchestrator"
  }

  depends_on = [aws_security_group.cluster]
}

# Output orchestrator details
output "orchestrator_id" {
  description = "Orchestrator instance ID"
  value       = aws_instance.orchestrator.id
}

output "orchestrator_public_ip" {
  description = "Orchestrator public IP address"
  value       = aws_instance.orchestrator.public_ip
}

output "orchestrator_private_ip" {
  description = "Orchestrator private IP address"
  value       = aws_instance.orchestrator.private_ip
}

output "security_group_id" {
  description = "Cluster security group ID"
  value       = aws_security_group.cluster.id
}

output "latest_ubuntu_ami" {
  description = "Latest Ubuntu 25.10 AMI ID"
  value       = data.aws_ami.ubuntu_latest.id
}
