#!/usr/bin/env bash

# ClaudeFS Terraform Automation CLI
# Wrapper for Terraform commands with cost tracking, validation, and automation

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/terraform"
LOG_FILE="/var/log/cfs-terraform.log"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
DEFAULT_ENVIRONMENT="dev"
DEFAULT_REGION="us-west-2"
TF_VARS_FILE="${TF_DIR}/terraform.tfvars"

# Helper functions
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"
}

info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# Terraform helpers
tf_init() {
    info "Initializing Terraform..."
    cd "$TF_DIR"
    terraform init -upgrade
    success "Terraform initialized"
}

tf_validate() {
    info "Validating Terraform configuration..."
    cd "$TF_DIR"
    terraform validate
    success "Terraform configuration valid"
}

tf_fmt() {
    info "Formatting Terraform files..."
    cd "$TF_DIR"
    terraform fmt -recursive .
    success "Terraform files formatted"
}

tf_plan() {
    local env="${1:-$DEFAULT_ENVIRONMENT}"
    local out_file="/tmp/terraform-${env}.plan"

    info "Planning Terraform for environment: $env"
    cd "$TF_DIR"

    terraform plan \
        -var-file="environments/${env}/terraform.tfvars" \
        -out="$out_file" \
        -lock=true

    info "Terraform plan saved to: $out_file"

    # Show resource count
    local resource_count=$(terraform show "$out_file" | grep -c "^  #" || echo "0")
    info "Planned resources: $resource_count"

    echo "$out_file"
}

tf_apply() {
    local env="${1:-$DEFAULT_ENVIRONMENT}"
    local plan_file="$2"

    if [[ -z "$plan_file" ]] || [[ ! -f "$plan_file" ]]; then
        error "Plan file not found: $plan_file"
        return 1
    fi

    info "Applying Terraform for environment: $env"
    read -p "Are you sure? (yes/no): " confirmation
    if [[ "$confirmation" != "yes" ]]; then
        error "Aborted"
        return 1
    fi

    cd "$TF_DIR"
    terraform apply "$plan_file"
    success "Terraform apply complete"

    # Save state backup
    backup_state
}

tf_destroy() {
    local env="${1:-$DEFAULT_ENVIRONMENT}"

    warn "About to destroy all resources in environment: $env"
    read -p "Type 'destroy' to confirm: " confirmation
    if [[ "$confirmation" != "destroy" ]]; then
        error "Aborted"
        return 1
    fi

    info "Destroying Terraform resources..."
    cd "$TF_DIR"

    terraform destroy \
        -var-file="environments/${env}/terraform.tfvars" \
        -auto-approve

    success "Terraform destroy complete"

    # Save state backup
    backup_state
}

tf_output() {
    local env="${1:-$DEFAULT_ENVIRONMENT}"
    local output_name="${2:-}"

    cd "$TF_DIR"

    if [[ -n "$output_name" ]]; then
        terraform output -raw "$output_name"
    else
        terraform output
    fi
}

tf_state() {
    local action="${1:-list}"

    cd "$TF_DIR"

    case "$action" in
        list)
            terraform state list
            ;;
        show)
            terraform state show "$2"
            ;;
        push)
            info "Pushing state to S3..."
            terraform state push
            ;;
        pull)
            info "Pulling state from S3..."
            terraform state pull
            ;;
        *)
            error "Unknown state action: $action"
            return 1
            ;;
    esac
}

backup_state() {
    info "Backing up Terraform state..."
    local backup_dir="/var/backups/terraform-state"
    mkdir -p "$backup_dir"

    cd "$TF_DIR"
    if [[ -f "terraform.tfstate" ]]; then
        cp "terraform.tfstate" "$backup_dir/terraform-$(date +%s).tfstate"
    fi

    success "State backed up to: $backup_dir"
}

check_cluster() {
    info "Checking cluster status..."
    cd "$TF_DIR"

    local orchestrator=$(terraform output -raw orchestrator_public_ip 2>/dev/null || echo "N/A")
    local storage_a=$(terraform output -json storage_site_a_public_ips 2>/dev/null | jq -r '.[0]' || echo "N/A")
    local storage_b=$(terraform output -json storage_site_b_public_ips 2>/dev/null | jq -r '.[0]' || echo "N/A")
    local fuse_client=$(terraform output -raw fuse_client_ip 2>/dev/null || echo "N/A")
    local nfs_client=$(terraform output -raw nfs_client_ip 2>/dev/null || echo "N/A")

    echo "Cluster Status:"
    echo "  Orchestrator: $orchestrator"
    echo "  Storage Site A (node 1): $storage_a"
    echo "  Storage Site B (node 1): $storage_b"
    echo "  FUSE Client: $fuse_client"
    echo "  NFS Client: $nfs_client"

    # Try to ping nodes
    info "Checking connectivity..."
    for ip in "$orchestrator" "$storage_a" "$storage_b"; do
        if [[ "$ip" != "N/A" ]]; then
            if ping -c 1 -W 2 "$ip" >/dev/null 2>&1; then
                success "✓ $ip is reachable"
            else
                warn "✗ $ip is not reachable"
            fi
        fi
    done
}

estimate_cost() {
    local env="${1:-$DEFAULT_ENVIRONMENT}"

    info "Estimating monthly cost for environment: $env"
    cd "$TF_DIR"

    # Get instance counts and types
    local storage_a=$(terraform output -json site_a_desired_capacity 2>/dev/null || echo "3")
    local storage_b=$(terraform output -json site_b_desired_capacity 2>/dev/null || echo "2")

    # Cost calculation (approximate, based on current spot pricing)
    # i4i.2xlarge: $0.40/hr on-demand, $0.14/hr spot (60% discount)
    # c7a.xlarge: $0.15/hr on-demand, $0.05/hr spot (65% discount)
    # t3.medium: $0.05/hr on-demand, $0.015/hr spot (70% discount)
    # c7a.2xlarge: $0.30/hr on-demand, $0.10/hr spot (65% discount)

    local storage_hourly=$((storage_a + storage_b))
    storage_hourly=$((storage_hourly * 14)) # 14 cents per hour (spot price)

    local compute_hourly=5 # 2×c7a.xlarge + 1×t3.medium + 1×c7a.2xlarge ≈ $0.25/hr
    local total_hourly=$((storage_hourly + compute_hourly))
    local total_daily=$((total_hourly * 24))
    local total_monthly=$((total_daily * 30))

    echo "Cost Estimate:"
    echo "  Storage nodes: $storage_a + $storage_b = $(($storage_a + $storage_b)) nodes @ \$0.14/hr"
    echo "  Compute nodes: 5 nodes @ \$0.05-0.10/hr"
    echo "  Hourly: \$$total_hourly (spot pricing)"
    echo "  Daily: \$$total_daily"
    echo "  Monthly (30 days): \$$total_monthly"
    echo ""
    echo "With on-demand pricing, monthly cost would be ~\$1,500-2,000"
    echo "With spot instances (current), monthly cost is ~\$500-750"
}

show_usage() {
    cat << EOF
ClaudeFS Terraform Automation CLI

USAGE: $(basename "$0") <command> [options]

COMMANDS:
  init            Initialize Terraform (must run first)
  validate        Validate Terraform configuration
  fmt             Format Terraform files
  plan [env]      Plan changes (env: dev/staging/prod, default: dev)
  apply [env] [file]  Apply plan (requires plan file)
  destroy [env]   Destroy all resources (DESTRUCTIVE)

  output [env]    Show Terraform outputs
  state [action]  Manage Terraform state (list/show/push/pull)
  backup          Backup current state

  status          Show cluster status
  check           Check cluster connectivity
  cost [env]      Estimate monthly costs

  help            Show this help message

ENVIRONMENT:
  TF_DIR=${TF_DIR}
  VARS_FILE=${TF_VARS_FILE}
  LOG_FILE=${LOG_FILE}

EXAMPLES:
  # Initialize Terraform
  $(basename "$0") init

  # Plan infrastructure changes
  $(basename "$0") plan dev

  # Apply plan (interactive confirmation required)
  $(basename "$0") apply dev /tmp/terraform-dev.plan

  # Check cluster status
  $(basename "$0") status

  # Estimate costs
  $(basename "$0") cost dev

EOF
}

# Main command dispatch
main() {
    mkdir -p "$(dirname "$LOG_FILE")"

    if [[ $# -lt 1 ]]; then
        show_usage
        return 1
    fi

    local command="$1"
    shift

    case "$command" in
        init)
            tf_init
            ;;
        validate)
            tf_validate
            ;;
        fmt|format)
            tf_fmt
            ;;
        plan)
            tf_plan "$@"
            ;;
        apply)
            tf_apply "$@"
            ;;
        destroy)
            tf_destroy "$@"
            ;;
        output)
            tf_output "$@"
            ;;
        state)
            tf_state "$@"
            ;;
        backup)
            backup_state
            ;;
        status|check)
            check_cluster
            ;;
        cost)
            estimate_cost "$@"
            ;;
        help|--help|-h)
            show_usage
            ;;
        *)
            error "Unknown command: $command"
            show_usage
            return 1
            ;;
    esac
}

# Ensure we're in the right directory and have Terraform
if ! command -v terraform &> /dev/null; then
    error "Terraform not found. Please install Terraform."
    exit 1
fi

if [[ ! -d "$TF_DIR" ]]; then
    error "Terraform directory not found: $TF_DIR"
    exit 1
fi

main "$@"
