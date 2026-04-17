# ClaudeFS Cluster Module
# Provides security groups and cluster-level configuration

data "aws_ami" "ubuntu_latest" {
  most_recent = true
  owners      = ["099720109477"]

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-questing-25.10-amd64-server-*"]
  }

  filter {
    name   = "state"
    values = ["available"]
  }
}

# Cluster internal security group
resource "aws_security_group" "cluster_internal" {
  name_prefix = "${var.project_tag}-cluster-internal-"
  description = "ClaudeFS cluster internal communication"
  vpc_id      = var.vpc_id

  # Internal cluster RPC (TCP)
  ingress {
    from_port   = 9400
    to_port     = 9410
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "ClaudeFS internal RPC"
  }

  # SWIM gossip protocol (UDP)
  ingress {
    from_port   = 9400
    to_port     = 9410
    protocol    = "udp"
    cidr_blocks = [var.cluster_cidr]
    description = "ClaudeFS SWIM gossip"
  }

  # Prometheus metrics
  ingress {
    from_port   = 9800
    to_port     = 9800
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "Prometheus metrics"
  }

  # Grafana
  ingress {
    from_port   = 3000
    to_port     = 3000
    protocol    = "tcp"
    cidr_blocks = var.grafana_cidr_blocks
    description = "Grafana dashboard"
  }

  # Cross-site replication
  ingress {
    from_port   = 5051
    to_port     = 5052
    protocol    = "tcp"
    cidr_blocks = var.replication_cidr_blocks
    description = "Cross-site replication"
  }

  # SSH access
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = var.ssh_cidr_blocks
    description = "SSH access"
  }

  # NFS/SMB for clients
  ingress {
    from_port   = 2049
    to_port     = 2049
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "NFS access"
  }

  ingress {
    from_port   = 445
    to_port     = 445
    protocol    = "tcp"
    cidr_blocks = [var.cluster_cidr]
    description = "SMB access"
  }

  # Egress: allow all
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Allow all outbound"
  }

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-cluster-sg"
  })
}

# Orchestrator instance (persistent)
resource "aws_instance" "orchestrator" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.orchestrator_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.orchestrator_iam_profile
  subnet_id                   = var.orchestrator_subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster_internal.id]
  associate_public_ip_address = var.assign_orchestrator_public_ip

  root_block_device {
    volume_type           = var.orchestrator_root_volume_type
    volume_size           = var.orchestrator_root_volume_size
    delete_on_termination = true
    encrypted             = true
  }

  user_data = var.orchestrator_user_data

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-orchestrator"
    Role = "orchestrator"
  })
}

# Conduit instance for cross-site replication
resource "aws_instance" "conduit" {
  count = var.enable_conduit ? 1 : 0

  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.conduit_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.orchestrator_subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster_internal.id]
  associate_public_ip_address = var.assign_conduit_public_ip

  spot_price                  = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.conduit_root_volume_size
    delete_on_termination = true
    encrypted             = true
  }

  user_data = var.conduit_user_data

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-conduit"
    Role = "conduit"
  })
}

# Jepsen controller for distributed testing
resource "aws_instance" "jepsen" {
  count = var.enable_jepsen ? 1 : 0

  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.jepsen_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.orchestrator_subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster_internal.id]
  associate_public_ip_address = true

  spot_price                  = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  root_block_device {
    volume_type           = "gp3"
    volume_size           = 50
    delete_on_termination = true
    encrypted             = true
  }

  user_data = var.jepsen_user_data

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-jepsen"
    Role = "jepsen"
  })
}