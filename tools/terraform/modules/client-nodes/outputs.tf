# Client Nodes Module Outputs

output "fuse_client_id" {
  description = "FUSE client instance ID"
  value       = var.enable_fuse_client ? aws_instance.fuse_client[0].id : ""
}

output "fuse_client_public_ip" {
  description = "FUSE client public IP"
  value       = var.enable_fuse_client ? aws_instance.fuse_client[0].public_ip : ""
}

output "fuse_client_private_ip" {
  description = "FUSE client private IP"
  value       = var.enable_fuse_client ? aws_instance.fuse_client[0].private_ip : ""
}

output "nfs_client_id" {
  description = "NFS client instance ID"
  value       = var.enable_nfs_client ? aws_instance.nfs_client[0].id : ""
}

output "nfs_client_public_ip" {
  description = "NFS client public IP"
  value       = var.enable_nfs_client ? aws_instance.nfs_client[0].public_ip : ""
}

output "nfs_client_private_ip" {
  description = "NFS client private IP"
  value       = var.enable_nfs_client ? aws_instance.nfs_client[0].private_ip : ""
}