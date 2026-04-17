# Client Nodes Module - FUSE and NFS clients

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

resource "aws_instance" "fuse_client" {
  count                       = var.enable_fuse_client ? 1 : 0
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.fuse_client_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = var.security_group_ids
  associate_public_ip_address = var.assign_public_ip

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.fuse_client_root_volume_size
    delete_on_termination = true
    encrypted             = true
  }

  user_data = var.fuse_client_user_data

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-fuse-client"
    Role = "client"
    Type = "fuse"
  })
}

resource "aws_instance" "nfs_client" {
  count                       = var.enable_nfs_client ? 1 : 0
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.nfs_client_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = var.security_group_ids
  associate_public_ip_address = var.assign_public_ip

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.nfs_client_root_volume_size
    delete_on_termination = true
    encrypted             = true
  }

  user_data = var.nfs_client_user_data

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-nfs-client"
    Role = "client"
    Type = "nfs-smb"
  })
}