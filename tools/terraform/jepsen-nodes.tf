# ClaudeFS Jepsen Test Infrastructure
# Distributed consistency and fault-tolerance testing

# Jepsen controller for orchestrating chaos tests
resource "aws_instance" "jepsen_controller" {
  ami                         = data.aws_ami.ubuntu_latest.id
  instance_type               = var.jepsen_instance_type
  key_name                    = var.ssh_key_name
  iam_instance_profile        = var.spot_iam_profile
  subnet_id                   = var.subnet_id
  vpc_security_group_ids      = [aws_security_group.cluster.id]
  associate_public_ip_address = true

  # Jepsen controller is CPU-intensive, use spot for cost savings
  spot_price              = var.use_spot_instances ? var.spot_max_price : null
  instance_interruption_behavior = var.use_spot_instances ? "terminate" : null

  # Root volume (50 GB gp3)
  root_block_device {
    volume_type           = "gp3"
    volume_size           = 50
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-jepsen-controller-root"
    }
  }

  # Jepsen test results and logs
  ebs_block_device {
    device_name           = "/dev/sdf"
    volume_type           = "gp3"
    volume_size           = 100
    delete_on_termination = true
    encrypted             = true
    tags = {
      Name = "${var.project_tag}-jepsen-results"
    }
  }

  user_data = file("${path.module}/../jepsen-user-data.sh")

  tags = merge(
    {
      Name    = "${var.project_tag}-jepsen-controller"
      Role    = "jepsen"
      Purpose = "Distributed consistency testing"
    },
    local.cost_tags
  )

  depends_on = [aws_security_group.cluster, aws_instance.orchestrator]
}

# Outputs for Jepsen controller
output "jepsen_controller_id" {
  description = "Jepsen controller instance ID"
  value       = aws_instance.jepsen_controller.id
}

output "jepsen_controller_public_ip" {
  description = "Jepsen controller public IP address"
  value       = aws_instance.jepsen_controller.public_ip
}

output "jepsen_controller_private_ip" {
  description = "Jepsen controller private IP address"
  value       = aws_instance.jepsen_controller.private_ip
}

output "jepsen_controller_dns" {
  description = "Jepsen controller DNS name"
  value       = aws_instance.jepsen_controller.public_dns
}

# CloudWatch dashboard for Jepsen test monitoring
resource "aws_cloudwatch_dashboard" "jepsen_tests" {
  dashboard_name = "${var.project_tag}-jepsen-tests"

  dashboard_body = jsonencode({
    widgets = [
      {
        type = "metric"
        properties = {
          metrics = [
            ["AWS/EC2", "CPUUtilization", { stat = "Average", label = "CPU" }],
            [".", "NetworkIn", { stat = "Sum", label = "Network In" }],
            [".", "NetworkOut", { stat = "Sum", label = "Network Out" }]
          ]
          period = 60
          stat   = "Average"
          region = var.aws_region
          title  = "Jepsen Controller Metrics"
        }
      }
    ]
  })
}

# Local variables for cost tagging
locals {
  cost_tags = {
    CostCenter = "Testing"
    Agent      = "A11"
    Department = "Infrastructure"
  }
}
