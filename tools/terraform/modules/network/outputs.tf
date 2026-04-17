# Network Module Outputs

output "vpc_id" {
  description = "VPC ID"
  value       = aws_vpc.selected[0].id
}

output "vpc_cidr" {
  description = "VPC CIDR block"
  value       = aws_vpc.selected[0].cidr_block
}

output "public_subnet_ids" {
  description = "Public subnet IDs"
  value       = aws_subnet.public[*].id
}

output "private_subnet_ids" {
  description = "Private subnet IDs for storage nodes"
  value       = aws_subnet.private_storage[*].id
}

output "availability_zones" {
  description = "Availability zones used"
  value       = local.azs
}

output "nat_gateway_id" {
  description = "NAT Gateway ID"
  value       = var.create_vpc && var.enable_nat_gateway ? aws_nat_gateway.main[0].id : ""
}

output "internet_gateway_id" {
  description = "Internet Gateway ID"
  value       = var.create_vpc ? aws_internet_gateway.main[0].id : ""
}