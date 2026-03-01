# ClaudeFS Client and Test Nodes

# FUSE client node for POSIX validation tests
resource "aws_instance" "fuse_client" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.fuse_client_instance_type
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
      Name = "${var.project_tag}-fuse-client-root"
    }
  }

  user_data = file("${path.module}/../client-node-user-data.sh")

  tags = {
    Name = "${var.project_tag}-fuse-client"
    Role = "client"
    Type = "fuse"
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# NFS/SMB client node for multi-protocol testing
resource "aws_instance" "nfs_client" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.nfs_client_instance_type
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
      Name = "${var.project_tag}-nfs-client-root"
    }
  }

  user_data = file("${path.module}/../client-node-user-data.sh")

  tags = {
    Name = "${var.project_tag}-nfs-client"
    Role = "client"
    Type = "nfs-smb"
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Cloud conduit for cross-site replication relay
resource "aws_instance" "cloud_conduit" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.conduit_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster.id]
  associate_public_ip_address = true

  spot_price              = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  root_block_device {
    volume_type           = "gp3"
    volume_size           = 20
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-conduit-root"
    }
  }

  user_data = file("${path.module}/../storage-node-user-data.sh")

  tags = {
    Name = "${var.project_tag}-cloud-conduit"
    Role = "conduit"
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Jepsen controller for distributed consistency testing
resource "aws_instance" "jepsen_controller" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.jepsen_instance_type
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
      Name = "${var.project_tag}-jepsen-root"
    }
  }

  user_data = file("${path.module}/../client-node-user-data.sh")

  tags = {
    Name = "${var.project_tag}-jepsen-controller"
    Role = "jepsen"
  }

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Outputs
output "fuse_client_id" {
  description = "FUSE client instance ID"
  value       = aws_instance.fuse_client.id
}

output "fuse_client_ip" {
  description = "FUSE client public IP"
  value       = aws_instance.fuse_client.public_ip
}

output "nfs_client_id" {
  description = "NFS client instance ID"
  value       = aws_instance.nfs_client.id
}

output "nfs_client_ip" {
  description = "NFS client public IP"
  value       = aws_instance.nfs_client.public_ip
}

output "conduit_id" {
  description = "Cloud conduit instance ID"
  value       = aws_instance.cloud_conduit.id
}

output "conduit_ip" {
  description = "Cloud conduit public IP"
  value       = aws_instance.cloud_conduit.public_ip
}

output "jepsen_id" {
  description = "Jepsen controller instance ID"
  value       = aws_instance.jepsen_controller.id
}

output "jepsen_ip" {
  description = "Jepsen controller public IP"
  value       = aws_instance.jepsen_controller.public_ip
}
