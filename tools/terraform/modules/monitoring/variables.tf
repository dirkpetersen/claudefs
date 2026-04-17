# Monitoring Module Variables

variable "project_tag" {
  description = "Project tag for naming"
  type        = string
  default     = "claudefs"
}

variable "environment" {
  description = "Environment name"
  type        = string
  default     = "dev"
}

variable "common_tags" {
  description = "Common tags for resources"
  type        = map(string)
  default     = {}
}

variable "cluster_cidr" {
  description = "Cluster CIDR for Prometheus discovery"
  type        = string
  default     = "10.0.0.0/8"
}

variable "orchestrator_instance_id" {
  description = "Orchestrator instance ID for monitoring"
  type        = string
  default     = ""
}

variable "storage_asg_name" {
  description = "Storage ASG name for monitoring"
  type        = string
  default     = ""
}

variable "enable_monitoring" {
  description = "Enable CloudWatch monitoring"
  type        = bool
  default     = true
}

variable "enable_cloudwatch_logs" {
  description = "Enable CloudWatch logs"
  type        = bool
  default     = true
}

variable "enable_cloudwatch_agent" {
  description = "Enable CloudWatch agent IAM role"
  type        = bool
  default     = true
}

variable "enable_sns_alerts" {
  description = "Enable SNS alerts"
  type        = bool
  default     = false
}

variable "enable_prometheus" {
  description = "Enable Prometheus integration"
  type        = bool
  default     = false
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 7
}

variable "alarm_actions" {
  description = "CloudWatch alarm actions"
  type        = list(string)
  default     = []
}

variable "alert_email_addresses" {
  description = "Email addresses for SNS alerts"
  type        = list(string)
  default     = []
}

variable "prometheus_scrape_interval" {
  description = "Prometheus scrape interval"
  type        = string
  default     = "15s"
}

variable "prometheus_storage_targets" {
  description = "Prometheus storage node targets"
  type        = list(string)
  default     = []
}