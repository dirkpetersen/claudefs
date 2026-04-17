# Monitoring Module Outputs

output "cloudwatch_log_group_name" {
  description = "CloudWatch log group name"
  value       = var.enable_cloudwatch_logs ? aws_cloudwatch_log_group.claudefs[0].name : ""
}

output "cloudwatch_agent_role_arn" {
  description = "CloudWatch agent IAM role ARN"
  value       = var.enable_cloudwatch_agent ? aws_iam_role.cloudwatch_agent[0].arn : ""
}

output "alerts_topic_arn" {
  description = "SNS alerts topic ARN"
  value       = var.enable_sns_alerts ? aws_sns_topic.alerts[0].arn : ""
}

output "prometheus_config" {
  description = "Prometheus configuration"
  value       = var.enable_prometheus ? data.template_file.prometheus_config[0].rendered : ""
}

output "prometheus_bucket_name" {
  description = "Prometheus S3 bucket name"
  value       = var.enable_prometheus ? aws_s3_bucket.prometheus_data[0].id : ""
}