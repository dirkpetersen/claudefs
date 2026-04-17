# Monitoring Module - CloudWatch and Prometheus integration

resource "aws_cloudwatch_log_group" "claudefs" {
  count             = var.enable_cloudwatch_logs ? 1 : 0
  name              = "/aws/claudefs/${var.project_tag}"
  retention_in_days = var.log_retention_days

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-logs"
  })
}

resource "aws_cloudwatch_metric_alarm" "orchestrator_cpu" {
  count                = var.enable_monitoring ? 1 : 0
  alarm_name          = "${var.project_tag}-orchestrator-cpu"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "2"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = "300"
  statistic           = "Average"
  threshold           = 80

  dimensions = {
    InstanceId = var.orchestrator_instance_id
  }

  alarm_actions = var.alarm_actions
  ok_actions    = var.alarm_actions
}

resource "aws_cloudwatch_metric_alarm" "orchestrator_memory" {
  count                = var.enable_monitoring ? 1 : 0
  alarm_name          = "${var.project_tag}-orchestrator-memory"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "2"
  metric_name         = "MemoryUtilization"
  namespace           = "AWS/EC2"
  period              = "300"
  statistic           = "Average"
  threshold           = 90

  dimensions = {
    InstanceId = var.orchestrator_instance_id
  }

  alarm_actions = var.alarm_actions
  ok_actions    = var.alarm_actions
}

resource "aws_cloudwatch_metric_alarm" "storage_disk_high" {
  count                = var.enable_monitoring ? 1 : 0
  alarm_name          = "${var.project_tag}-storage-disk-high"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "3"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = "300"
  statistic           = "Maximum"
  threshold           = 85

  dimensions = {
    AutoScalingGroupName = var.storage_asg_name
  }

  alarm_actions = var.alarm_actions
  ok_actions    = var.alarm_actions
}

resource "aws_sns_topic" "alerts" {
  count = var.enable_sns_alerts ? 1 : 0
  name  = "${var.project_tag}-alerts"

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-alerts-topic"
  })
}

resource "aws_sns_topic_subscription" "email_alerts" {
  count     = var.enable_sns_alerts && length(var.alert_email_addresses) > 0 ? length(var.alert_email_addresses) : 0
  topic_arn = aws_sns_topic.alerts[0].arn
  protocol  = "email"
  endpoint  = var.alert_email_addresses[count.index]
}

resource "aws_iam_role" "cloudwatch_agent" {
  count       = var.enable_cloudwatch_agent ? 1 : 0
  name        = "${var.project_tag}-cloudwatch-agent-role"
  description = "Role for CloudWatch agent to send metrics"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "ec2.amazonaws.com"
      }
    }]
  })
}

resource "aws_iam_role_policy" "cloudwatch_agent_policy" {
  count      = var.enable_cloudwatch_agent ? 1 : 0
  name       = "${var.project_tag}-cloudwatch-agent-policy"
  role       = aws_iam_role.cloudwatch_agent[0].id
  policy     = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "cloudwatch:PutMetricData",
        "logs:CreateLogGroup",
        "logs:CreateLogStream",
        "logs:PutLogEvents"
      ]
      Resource = "*"
    }]
  })
}

data "template_file" "prometheus_config" {
  count    = var.enable_prometheus ? 1 : 0
  template = file("${path.module}/templates/prometheus.yml.tpl")

  vars = {
    scrape_interval   = var.prometheus_scrape_interval
    storage_endpoints = join(",", var.prometheus_storage_targets)
    cluster_cidr      = var.cluster_cidr
  }
}

resource "aws_s3_bucket" "prometheus_data" {
  count       = var.enable_prometheus ? 1 : 0
  bucket      = "${var.project_tag}-prometheus-${var.environment}"
  acl         = "private"

  tags = merge(var.common_tags, {
    Name = "${var.project_tag}-prometheus-data"
  })
}

resource "aws_s3_bucket_server_side_encryption_configuration" "prometheus_data" {
  count  = var.enable_prometheus ? 1 : 0
  bucket = aws_s3_bucket.prometheus_data[0].id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}