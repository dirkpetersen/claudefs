# ClaudeFS Storage Nodes (Site A and Site B)

# Storage nodes for Site A (Raft quorum, 3 nodes)
resource "aws_instance" "storage_site_a" {
  count                       = var.storage_site_a_count
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.storage_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster.id]
  associate_public_ip_address = true

  # Use spot instances if enabled
  spot_price              = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  # Root volume (50 GB gp3)
  root_block_device {
    volume_type           = "gp3"
    volume_size           = 50
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-storage-a-${count.index + 1}-root"
    }
  }

  # NVMe data volume (simulated with EBS gp3 in AWS without Nitro NVMe)
  # In production, use i4i instances which have local NVMe
  ebs_block_device {
    device_name           = "/dev/sdf"
    volume_type           = "gp3"
    volume_size           = 1875
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-storage-a-${count.index + 1}-data"
    }
  }

  # User data script
  user_data = file("${path.module}/../storage-node-user-data.sh")

  tags = {
    Name    = "${var.project_tag}-storage-a-${count.index + 1}"
    Role    = "storage"
    Site    = "A"
    NodeNum = count.index + 1
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Storage nodes for Site B (replication, 2 nodes)
resource "aws_instance" "storage_site_b" {
  count                       = var.storage_site_b_count
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.storage_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster.id]
  associate_public_ip_address = true

  spot_price              = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  root_block_device {
    volume_type           = "gp3"
    volume_size           = 50
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-storage-b-${count.index + 1}-root"
    }
  }

  ebs_block_device {
    device_name           = "/dev/sdf"
    volume_type           = "gp3"
    volume_size           = 1875
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-storage-b-${count.index + 1}-data"
    }
  }

  user_data = file("${path.module}/../storage-node-user-data.sh")

  tags = {
    Name    = "${var.project_tag}-storage-b-${count.index + 1}"
    Role    = "storage"
    Site    = "B"
    NodeNum = count.index + 1
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Outputs for storage nodes
output "storage_site_a_ids" {
  description = "Storage site A instance IDs"
  value       = aws_instance.storage_site_a[*].id
}

output "storage_site_a_private_ips" {
  description = "Storage site A private IP addresses"
  value       = aws_instance.storage_site_a[*].private_ip
}

output "storage_site_a_public_ips" {
  description = "Storage site A public IP addresses"
  value       = aws_instance.storage_site_a[*].public_ip
}

output "storage_site_b_ids" {
  description = "Storage site B instance IDs"
  value       = aws_instance.storage_site_b[*].id
}

output "storage_site_b_private_ips" {
  description = "Storage site B private IP addresses"
  value       = aws_instance.storage_site_b[*].private_ip
}

output "storage_site_b_public_ips" {
  description = "Storage site B public IP addresses"
  value       = aws_instance.storage_site_b[*].public_ip
}
