#!/bin/bash
# tools/cfs-checkpoint-manager.sh
# Creates and manages cluster state snapshots for rollback

set -euo pipefail

STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
S3_BACKUP_BUCKET="${S3_BACKUP_BUCKET:-cfs-terraform-backups}"
RETENTION_DAYS="${RETENTION_DAYS:-7}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/checkpoint.log}"

mkdir -p "$STATE_DIR/checkpoints" "$(dirname "$LOG_FILE")"

log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Create checkpoint after successful deployment
create_checkpoint() {
    local checkpoint_id="cluster-working-$(date +%s)"
    local checkpoint_dir="$STATE_DIR/checkpoints/$checkpoint_id"

    mkdir -p "$checkpoint_dir"

    log "Creating checkpoint: $checkpoint_id"

    # 1. Save current git HEAD
    if command -v git &> /dev/null && [[ -d .git ]]; then
        git rev-parse HEAD > "$checkpoint_dir/git-head.txt" 2>/dev/null || echo "unknown" > "$checkpoint_dir/git-head.txt"
    else
        echo "unknown" > "$checkpoint_dir/git-head.txt"
    fi

    # 2. Save cluster config snapshot
    if [[ -f "infrastructure/cluster.yaml" ]]; then
        cp "infrastructure/cluster.yaml" "$checkpoint_dir/cluster.yaml"
    fi

    # 3. Save components if they exist
    if [[ -d "infrastructure/components" ]]; then
        mkdir -p "$checkpoint_dir/components"
        cp -r "infrastructure/components/"* "$checkpoint_dir/components/" 2>/dev/null || true
    fi

    # 4. Create git tag for this checkpoint
    if command -v git &> /dev/null && [[ -d .git ]]; then
        git tag -a "$checkpoint_id" -m "Checkpoint: cluster working state" HEAD 2>/dev/null || true
    fi

    log "Checkpoint created: $checkpoint_dir"

    # 5. S3 backup if AWS available
    if command -v aws &> /dev/null && [[ -n "$S3_BACKUP_BUCKET" ]]; then
        tar czf "/tmp/$checkpoint_id.tar.gz" "$checkpoint_dir" 2>/dev/null || true
        aws s3 cp "/tmp/$checkpoint_id.tar.gz" "s3://$S3_BACKUP_BUCKET/checkpoints/" 2>/dev/null || true
        rm -f "/tmp/$checkpoint_id.tar.gz"
        log "Checkpoint backed up to S3: s3://$S3_BACKUP_BUCKET/checkpoints/$checkpoint_id.tar.gz"
    fi
}

# List available checkpoints
list_checkpoints() {
    log "Available checkpoints:"

    if [[ -d "$STATE_DIR/checkpoints" ]]; then
        ls -lh "$STATE_DIR/checkpoints/" 2>/dev/null | tail -n +2 || log "No checkpoints found"
    else
        log "No checkpoints directory found"
    fi

    if command -v git &> /dev/null && [[ -d .git ]]; then
        log "Git tags:"
        git tag -l "cluster-working-*" 2>/dev/null | sort -r | head -5 || true
    fi
}

# Get most recent working checkpoint
get_latest_checkpoint() {
    if [[ -d "$STATE_DIR/checkpoints" ]]; then
        ls -td "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null | head -1 || echo ""
    else
        echo ""
    fi
}

# Cleanup old checkpoints (keep last 5)
cleanup_old_checkpoints() {
    log "Cleaning up old checkpoints (keeping last 5)..."

    if [[ ! -d "$STATE_DIR/checkpoints" ]]; then
        return 0
    fi

    local checkpoint_dirs=($(ls -dt "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null || true))

    if [[ ${#checkpoint_dirs[@]} -gt 5 ]]; then
        for ((i=5; i<${#checkpoint_dirs[@]}; i++)); do
            local old_checkpoint="${checkpoint_dirs[$i]}"
            log "Deleting old checkpoint: $old_checkpoint"
            rm -rf "$old_checkpoint"
        done
    fi
}

# Validate checkpoint integrity
validate_checkpoint() {
    local checkpoint_dir=$1

    if [[ ! -d "$checkpoint_dir" ]]; then
        log "ERROR: Checkpoint directory not found: $checkpoint_dir"
        return 1
    fi

    log "Validating checkpoint: $checkpoint_dir"

    # Check at least one required file present
    if [[ -f "$checkpoint_dir/git-head.txt" ]] || [[ -f "$checkpoint_dir/cluster.yaml" ]]; then
        log "Checkpoint validation passed"
        return 0
    else
        log "ERROR: Checkpoint missing required files"
        return 1
    fi
}

# Main
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-list}" in
        "create")
            create_checkpoint
            cleanup_old_checkpoints
            ;;
        "list")
            list_checkpoints
            ;;
        "validate")
            validate_checkpoint "${2:-.}"
            ;;
        "latest")
            get_latest_checkpoint
            ;;
        *)
            echo "Usage: $0 {create|list|validate|latest <checkpoint_dir>}"
            exit 1
            ;;
    esac
fi
