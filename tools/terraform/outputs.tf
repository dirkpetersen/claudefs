# ClaudeFS Infrastructure Outputs

# Cluster summary
output "cluster_info" {
  description = "Cluster topology and connection information"
  value = {
    orchestrator = {
      id          = aws_instance.orchestrator.id
      public_ip   = aws_instance.orchestrator.public_ip
      private_ip  = aws_instance.orchestrator.private_ip
      role        = "orchestrator"
      instance_type = var.orchestrator_instance_type
    }
    storage_site_a = {
      count        = length(aws_instance.storage_site_a)
      ids          = aws_instance.storage_site_a[*].id
      public_ips   = aws_instance.storage_site_a[*].public_ip
      private_ips  = aws_instance.storage_site_a[*].private_ip
      role         = "storage"
      site         = "A"
      instance_type = var.storage_instance_type
    }
    storage_site_b = {
      count        = length(aws_instance.storage_site_b)
      ids          = aws_instance.storage_site_b[*].id
      public_ips   = aws_instance.storage_site_b[*].public_ip
      private_ips  = aws_instance.storage_site_b[*].private_ip
      role         = "storage"
      site         = "B"
      instance_type = var.storage_instance_type
    }
    clients = {
      fuse = {
        id        = aws_instance.fuse_client.id
        public_ip = aws_instance.fuse_client.public_ip
      }
      nfs = {
        id        = aws_instance.nfs_client.id
        public_ip = aws_instance.nfs_client.public_ip
      }
    }
    conduit = {
      id        = aws_instance.cloud_conduit.id
      public_ip = aws_instance.cloud_conduit.public_ip
    }
    jepsen = {
      id        = aws_instance.jepsen_controller.id
      public_ip = aws_instance.jepsen_controller.public_ip
    }
  }
}

# Connection strings for SSH
output "ssh_commands" {
  description = "SSH commands for connecting to each node"
  value = {
    orchestrator = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${aws_instance.orchestrator.public_ip}"
    storage_a = [
      for i, ip in aws_instance.storage_site_a[*].public_ip :
      "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${ip} # storage-a-${i + 1}"
    ]
    storage_b = [
      for i, ip in aws_instance.storage_site_b[*].public_ip :
      "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${ip} # storage-b-${i + 1}"
    ]
    fuse_client = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${aws_instance.fuse_client.public_ip}"
    nfs_client  = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${aws_instance.nfs_client.public_ip}"
    conduit     = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${aws_instance.cloud_conduit.public_ip}"
    jepsen      = "ssh -i ~/.ssh/${var.ssh_key_name}.pem ec2-user@${aws_instance.jepsen_controller.public_ip}"
  }
}

# Cluster private network information for configuration
output "cluster_private_ips" {
  description = "Private IP addresses for cluster internal communication"
  value = {
    orchestrator_ip = aws_instance.orchestrator.private_ip
    storage_a_ips   = aws_instance.storage_site_a[*].private_ip
    storage_b_ips   = aws_instance.storage_site_b[*].private_ip
    conduit_ip      = aws_instance.cloud_conduit.private_ip
    jepsen_ip       = aws_instance.jepsen_controller.private_ip
  }
}

# Security group information
output "security_info" {
  description = "Security group and network configuration"
  value = {
    security_group_id = aws_security_group.cluster.id
    security_group_name = aws_security_group.cluster.name
    cluster_cidr = var.cluster_cidr
  }
}

# Deployment statistics
output "deployment_stats" {
  description = "Cluster deployment statistics"
  value = {
    total_instances = 1 + var.storage_site_a_count + var.storage_site_b_count + 4
    orchestrator_count = 1
    storage_count = var.storage_site_a_count + var.storage_site_b_count
    client_count = 4
    use_spot_instances = var.use_spot_instances
    estimated_daily_cost = var.use_spot_instances ? "~$20-26 USD" : "~$80-100 USD"
  }
}
