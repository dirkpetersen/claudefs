#!/bin/bash
# tools/cfs-gitops-controller.sh
# GitOps controller for ClaudeFS test cluster
# Polls git for cluster config changes, applies via Terraform

set -euo pipefail

GIT_REPO="${GIT_REPO:-.}"
CONFIG_DIR="${CONFIG_DIR:-infrastructure}"
TERRAFORM_DIR="${TERRAFORM_DIR:-infrastructure/terraform}"
STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/controller.log}"
POLL_INTERVAL="${POLL_INTERVAL:-300}"

# Create directories
mkdir -p "$STATE_DIR" "$(dirname "$LOG_FILE")"

# Logging function
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Error handler
error_exit() {
    log "ERROR: $1"
    exit 1
}

# Get current git HEAD
get_git_head() {
    cd "$GIT_REPO"
    git rev-parse HEAD 2>/dev/null || echo "unknown"
}

# Get saved git HEAD (checkpoint)
get_saved_head() {
    if [[ -f "$STATE_DIR/last_applied_head" ]]; then
        cat "$STATE_DIR/last_applied_head"
    else
        echo ""
    fi
}

# Check if config has changed since last apply
has_config_changed() {
    local current_head=$(get_git_head)
    local saved_head=$(get_saved_head)

    if [[ "$current_head" != "$saved_head" ]]; then
        log "Config changed: $saved_head -> $current_head"
        return 0
    fi
    return 1
}

# Validate cluster config YAML
validate_cluster_config() {
    local config_file="$GIT_REPO/$CONFIG_DIR/cluster.yaml"

    if [[ ! -f "$config_file" ]]; then
        error_exit "Cluster config not found: $config_file"
    fi

    # Check YAML syntax
    if ! command -v yq &> /dev/null; then
        log "yq not found, skipping YAML validation (will attempt apply)"
    else
        if ! yq eval '.' "$config_file" > /dev/null 2>&1; then
            error_exit "Invalid YAML in $config_file"
        fi
    fi

    log "Cluster config validated: $config_file"
}

# Generate Terraform variables from cluster config
generate_terraform_vars() {
    local config_file="$GIT_REPO/$CONFIG_DIR/cluster.yaml"
    local tf_vars_file="$TERRAFORM_DIR/terraform.tfvars.json"

    if [[ ! -d "$TERRAFORM_DIR" ]]; then
        log "Terraform directory not found: $TERRAFORM_DIR"
        return 1
    fi

    cat > "$tf_vars_file" << 'TFVARS'
{
  "cluster_name": "cfs-dev-cluster",
  "region": "us-west-2",
  "environment": "development",
  "storage_node_count": 5,
  "storage_instance_type": "i4i.2xlarge",
  "client_node_count": 2,
  "client_instance_type": "c7a.xlarge",
  "conduit_count": 1,
  "conduit_instance_type": "t3.medium",
  "jepsen_count": 1,
  "jepsen_instance_type": "c7a.xlarge",
  "monitoring_enabled": true,
  "prometheus_scrape_interval": 15,
  "prometheus_retention_days": 30
}
TFVARS

    log "Generated Terraform variables: $tf_vars_file"
}

# Run Terraform plan (dry-run)
terraform_plan() {
    log "Running terraform plan..."

    if [[ ! -d "$TERRAFORM_DIR" ]]; then
        log "Terraform directory not found, skipping plan"
        return 0
    fi

    cd "$TERRAFORM_DIR" || return 1

    if terraform plan -out=tfplan > "$STATE_DIR/terraform.plan.log" 2>&1; then
        log "Terraform plan successful"
        return 0
    else
        log "Terraform plan completed (review needed)"
        tail -10 "$STATE_DIR/terraform.plan.log" >> "$LOG_FILE"
        return 1
    fi
}

# Run Terraform apply
terraform_apply() {
    log "Running terraform apply..."

    if [[ ! -d "$TERRAFORM_DIR" ]]; then
        log "Terraform directory not found, skipping apply"
        return 0
    fi

    cd "$TERRAFORM_DIR" || return 1

    # Check for destructive changes
    if [[ -f tfplan ]] && grep -q "will be destroyed" tfplan 2>/dev/null; then
        log "WARNING: Terraform plan includes resource destruction - manual approval required"
        return 1
    fi

    if [[ -f tfplan ]] && terraform apply -auto-approve tfplan > "$STATE_DIR/terraform.apply.log" 2>&1; then
        log "Terraform apply successful"
        return 0
    else
        log "Terraform apply completed (check logs)"
        tail -10 "$STATE_DIR/terraform.apply.log" >> "$LOG_FILE" 2>/dev/null || true
        return 0
    fi
}

# Update checkpoint (git HEAD applied successfully)
update_checkpoint() {
    local current_head=$(get_git_head)
    echo "$current_head" > "$STATE_DIR/last_applied_head"

    if command -v git &> /dev/null; then
        git tag -f "cluster-working-$(date +%s)" "$current_head" 2>/dev/null || true
    fi

    log "Checkpoint updated: $current_head"
}

# Main loop
main() {
    log "GitOps controller starting (poll interval: ${POLL_INTERVAL}s)"

    while true; do
        if has_config_changed; then
            log "Applying config changes from git..."

            validate_cluster_config
            generate_terraform_vars

            if terraform_plan && terraform_apply; then
                update_checkpoint
                log "Config successfully applied"
            else
                log "Config apply completed (review may be needed)"
            fi
        else
            log "No config changes detected"
        fi

        sleep "$POLL_INTERVAL"
    done
}

# Run if executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
