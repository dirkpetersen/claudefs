# Storage Nodes Module Outputs

output "site_a_asg_name" {
  description = "Site A Auto Scaling Group name"
  value       = aws_autoscaling_group.storage_site_a.name
}

output "site_a_asg_arn" {
  description = "Site A Auto Scaling Group ARN"
  value       = aws_autoscaling_group.storage_site_a.arn
}

output "site_a_desired_capacity" {
  description = "Site A current desired capacity"
  value       = aws_autoscaling_group.storage_site_a.desired_capacity
}

output "site_a_min_size" {
  description = "Site A minimum size"
  value       = aws_autoscaling_group.storage_site_a.min_size
}

output "site_a_max_size" {
  description = "Site A maximum size"
  value       = aws_autoscaling_group.storage_site_a.max_size
}

output "site_b_asg_name" {
  description = "Site B Auto Scaling Group name"
  value       = var.site_b_desired_capacity > 0 ? aws_autoscaling_group.storage_site_b[0].name : ""
}

output "site_b_asg_arn" {
  description = "Site B Auto Scaling Group ARN"
  value       = var.site_b_desired_capacity > 0 ? aws_autoscaling_group.storage_site_b[0].arn : ""
}

output "site_b_desired_capacity" {
  description = "Site B current desired capacity"
  value       = var.site_b_desired_capacity > 0 ? aws_autoscaling_group.storage_site_b[0].desired_capacity : 0
}

output "launch_template_id" {
  description = "Launch template ID"
  value       = aws_launch_template.storage_node.id
}

output "launch_template_name" {
  description = "Launch template name"
  value       = aws_launch_template.storage_node.name
}

output "scale_up_policy_arn" {
  description = "Scale-up policy ARN"
  value       = aws_autoscaling_policy.scale_up_cpu.arn
}

output "scale_down_policy_arn" {
  description = "Scale-down policy ARN"
  value       = aws_autoscaling_policy.scale_down_cpu.arn
}

output "total_storage_nodes" {
  description = "Total number of storage nodes (Site A + Site B)"
  value       = aws_autoscaling_group.storage_site_a.desired_capacity + var.site_b_desired_capacity
}