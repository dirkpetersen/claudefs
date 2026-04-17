# ClaudeFS Cluster Module Outputs

output "security_group_id" {
  description = "Cluster security group ID"
  value       = aws_security_group.cluster_internal.id
}

output "orchestrator_id" {
  description = "Orchestrator instance ID"
  value       = aws_instance.orchestrator.id
}

output "orchestrator_public_ip" {
  description = "Orchestrator public IP"
  value       = aws_instance.orchestrator.public_ip
}

output "orchestrator_private_ip" {
  description = "Orchestrator private IP"
  value       = aws_instance.orchestrator.private_ip
}

output "conduit_id" {
  description = "Conduit instance ID"
  value       = var.enable_conduit ? aws_instance.conduit[0].id : ""
}

output "conduit_public_ip" {
  description = "Conduit public IP"
  value       = var.enable_conduit ? aws_instance.conduit[0].public_ip : ""
}

output "jepsen_id" {
  description = "Jepsen instance ID"
  value       = var.enable_jepsen ? aws_instance.jepsen[0].id : ""
}

output "jepsen_public_ip" {
  description = "Jepsen public IP"
  value       = var.enable_jepsen ? aws_instance.jepsen[0].public_ip : ""
}

output "ubuntu_ami_id" {
  description = "Latest Ubuntu 25.10 AMI ID"
  value       = data.aws_ami.ubuntu_latest.id
}