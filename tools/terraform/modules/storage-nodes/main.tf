# Storage Nodes Module - Auto Scaling Group
# Provides elastic storage node cluster with auto-scaling

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

resource "aws_launch_template" "storage_node" {
  name_prefix   = "${var.project_tag}-storage-"
  image_id      = data.aws_ami.ubuntu_latest.id
  instance_type = var.instance_type
  key_name      = var.ssh_key_name

  iam_instance_profile {
    name = var.spot_iam_profile
  }

  vpc_security_group_ids = var.security_group_ids

  root_block_device {
    volume_type = var.root_volume_type
    volume_size = var.root_volume_size
    encrypted   = true
    delete_on_termination = true
  }

  block_device_mappings {
    device_name = "/dev/sdf"
    ebs {
      volume_type = var.data_volume_type
      volume_size = var.data_volume_size
      encrypted   = true
      delete_on_termination = true
    }
  }

  user_data = base64encode(var.user_data)

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  monitoring {
    enabled = true
  }

  tag_specifications {
    resource_type = "instance"
    tags = merge(var.common_tags, {
      Role = "storage"
    })
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_autoscaling_group" "storage_site_a" {
  name                = "${var.project_tag}-storage-site-a"
  vpc_zone_identifier = var.subnet_ids
  desired_capacity    = var.site_a_desired_capacity
  min_size            = var.site_a_min_size
  max_size            = var.site_a_max_size
  default_cooldown    = var.scale_cooldown

  launch_template {
    id      = aws_launch_template.storage_node.id
    version = "$Latest"
  }

  tag {
    key                 = "Name"
    value               = "${var.project_tag}-storage-site-a"
    propagate_at_launch = true
  }

  tag {
    key                 = "Role"
    value               = "storage"
    propagate_at_launch = true
  }

  tag {
    key                 = "Site"
    value               = "A"
    propagate_at_launch = true
  }

  tag {
    key                 = "Project"
    value               = var.project_tag
    propagate_at_launch = true
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_autoscaling_group" "storage_site_b" {
  count               = var.site_b_desired_capacity > 0 ? 1 : 0
  name                = "${var.project_tag}-storage-site-b"
  vpc_zone_identifier = var.subnet_ids
  desired_capacity    = var.site_b_desired_capacity
  min_size            = var.site_b_min_size
  max_size            = var.site_b_max_size
  default_cooldown    = var.scale_cooldown

  launch_template {
    id      = aws_launch_template.storage_node.id
    version = "$Latest"
  }

  tag {
    key                 = "Name"
    value               = "${var.project_tag}-storage-site-b"
    propagate_at_launch = true
  }

  tag {
    key                 = "Role"
    value               = "storage"
    propagate_at_launch = true
  }

  tag {
    key                 = "Site"
    value               = "B"
    propagate_at_launch = true
  }

  tag {
    key                 = "Project"
    value               = var.project_tag
    propagate_at_launch = true
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_autoscaling_policy" "scale_up_cpu" {
  name                   = "${var.project_tag}-scale-up-cpu"
  scaling_adjustment     = 1
  adjustment_type        = "ChangeInCapacity"
  cooldown               = var.scale_up_cooldown
  autoscaling_group_name = aws_autoscaling_group.storage_site_a.name
}

resource "aws_autoscaling_policy" "scale_down_cpu" {
  name                   = "${var.project_tag}-scale-down-cpu"
  scaling_adjustment     = -1
  adjustment_type        = "ChangeInCapacity"
  cooldown               = var.scale_down_cooldown
  autoscaling_group_name = aws_autoscaling_group.storage_site_a.name
}

resource "aws_cloudwatch_metric_alarm" "scale_up_cpu" {
  alarm_name          = "${var.project_tag}-scale-up-cpu"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "5"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = "60"
  statistic           = "Average"
  threshold           = var.scale_up_cpu_threshold

  dimensions = {
    AutoScalingGroupName = aws_autoscaling_group.storage_site_a.name
  }

  alarm_actions = [aws_autoscaling_policy.scale_up_cpu.arn]
}

resource "aws_cloudwatch_metric_alarm" "scale_down_cpu" {
  alarm_name          = "${var.project_tag}-scale-down-cpu"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = "15"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = "60"
  statistic           = "Average"
  threshold           = var.scale_down_cpu_threshold

  dimensions = {
    AutoScalingGroupName = aws_autoscaling_group.storage_site_a.name
  }

  alarm_actions = [aws_autoscaling_policy.scale_down_cpu.arn]
}

resource "aws_cloudwatch_metric_alarm" "scale_up_disk" {
  count                = var.enable_disk_scaling ? 1 : 0
  alarm_name          = "${var.project_tag}-scale-up-disk"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "3"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = "300"
  statistic           = "Average"
  threshold           = var.scale_up_disk_threshold

  dimensions = {
    AutoScalingGroupName = aws_autoscaling_group.storage_site_a.name
  }

  alarm_actions = [aws_autoscaling_policy.scale_up_cpu.arn]
}

resource "aws_autoscaling_scheduled_action" "maintenance_scale" {
  count                  = var.enable_scheduled_scaling ? 1 : 0
  name                   = "${var.project_tag}-maintenance-scale"
  autoscaling_group_name = aws_autoscaling_group.storage_site_a.name
  min_size              = var.site_a_min_size
  desired_capacity      = var.site_a_desired_capacity
  max_size              = var.site_a_max_size

  recurrence = var.scheduled_recurrence
}