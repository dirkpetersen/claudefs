# IAM Roles and Policies for ClaudeFS

# Orchestrator IAM Role
resource "aws_iam_role" "orchestrator" {
  name = "${var.project_tag}-orchestrator-role"

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

resource "aws_iam_role_policy" "orchestrator_s3" {
  name = "${var.project_tag}-orchestrator-s3"
  role = aws_iam_role.orchestrator.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
          "s3:ListBucket"
        ]
        Resource = [
          "arn:aws:s3:::${var.artifact_bucket_name}/*",
          "arn:aws:s3:::${var.artifact_bucket_name}"
        ]
      }
    ]
  })
}

resource "aws_iam_role_policy" "orchestrator_secrets" {
  name = "${var.project_tag}-orchestrator-secrets"
  role = aws_iam_role.orchestrator.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "secretsmanager:GetSecretValue",
        "secretsmanager:DescribeSecret"
      ]
      Resource = "arn:aws:secretsmanager:*:*:secret:${var.project_tag}/*"
    }]
  })
}

resource "aws_iam_role_policy" "orchestrator_cloudwatch" {
  name = "${var.project_tag}-orchestrator-cloudwatch"
  role = aws_iam_role.orchestrator.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "cloudwatch:PutMetricData",
        "cloudwatch:GetMetricStatistics",
        "logs:CreateLogGroup",
        "logs:CreateLogStream",
        "logs:PutLogEvents"
      ]
      Resource = "*"
    }]
  })
}

resource "aws_iam_role_policy" "orchestrator_ec2" {
  name = "${var.project_tag}-orchestrator-ec2"
  role = aws_iam_role.orchestrator.id

  policy = var.production_mode ? jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "ec2:DescribeInstances",
        "ec2:DescribeTags",
        "ec2:DescribeSecurityGroups",
        "ec2:DescribeSubnets"
      ]
      Resource = "*"
    }]
  }) : jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = ["ec2:*"]
      Resource = "*"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "orchestrator_base" {
  role       = aws_iam_role.orchestrator.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "orchestrator" {
  name = "${var.project_tag}-orchestrator-profile"
  role = aws_iam_role.orchestrator.name
}

# Spot Node IAM Role (reduced permissions)
resource "aws_iam_role" "spot_node" {
  name = "${var.project_tag}-spot-node-role"

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

resource "aws_iam_role_policy" "spot_node_s3" {
  name = "${var.project_tag}-spot-node-s3"
  role = aws_iam_role.spot_node.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:PutObject",
          "s3:ListBucket"
        ]
        Resource = [
          "arn:aws:s3:::${var.data_bucket_name}/*",
          "arn:aws:s3:::${var.data_bucket_name}"
        ]
      }
    ]
  })
}

resource "aws_iam_role_policy" "spot_node_secrets" {
  name = "${var.project_tag}-spot-node-secrets"
  role = aws_iam_role.spot_node.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "secretsmanager:GetSecretValue"
      ]
      Resource = "arn:aws:secretsmanager:*:*:secret:${var.project_tag}/encryption/*"
    }]
  })
}

resource "aws_iam_role_policy" "spot_node_ec2" {
  name = "${var.project_tag}-spot-node-ec2"
  role = aws_iam_role.spot_node.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "ec2:DescribeInstances",
        "ec2:DescribeTags"
      ]
      Resource = "*"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "spot_node_base" {
  role       = aws_iam_role.spot_node.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "spot_node" {
  name = "${var.project_tag}-spot-node-profile"
  role = aws_iam_role.spot_node.name
}

output "orchestrator_role_arn" {
  description = "Orchestrator IAM role ARN"
  value       = aws_iam_role.orchestrator.arn
}

output "orchestrator_instance_profile" {
  description = "Orchestrator instance profile name"
  value       = aws_iam_instance_profile.orchestrator.name
}

output "spot_node_role_arn" {
  description = "Spot node IAM role ARN"
  value       = aws_iam_role.spot_node.arn
}

output "spot_node_instance_profile" {
  description = "Spot node instance profile name"
  value       = aws_iam_instance_profile.spot_node.name
}