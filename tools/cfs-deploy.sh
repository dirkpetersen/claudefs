#!/usr/bin/env bash
# cfs-deploy.sh — ClaudeFS build and deploy to test cluster
#
# Builds release binaries and deploys to cluster nodes.
# Requires: cfs-dev cluster already provisioned (cfs-dev up)
#
# Usage:
#   cfs-deploy.sh [--target all|storage|client] [--key KEY] [--phase N]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REGION="us-west-2"
PROJECT_TAG="claudefs"
KEY_NAME="${CFS_KEY_NAME:-}"

die() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "==> $*"; }
warn() { echo "WARNING: $*" >&2; }

# --- Helpers ---

ssh_key_arg() {
  if [[ -n "$KEY_NAME" ]]; then
    for suffix in "" ".pem" ".cer"; do
      local path="$HOME/.ssh/${KEY_NAME}${suffix}"
      if [[ -f "$path" ]]; then echo "-i $path"; return; fi
    done
  fi
}

get_instances_by_role() {
  local role="$1"
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:Role,Values=$role" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].PublicIpAddress' \
    --output text --region "$REGION" 2>/dev/null
}

build_release() {
  info "Building ClaudeFS in release mode..."
  cd "$REPO_DIR"
  cargo build --workspace --release 2>&1 | tail -5
  info "Build complete: $(ls target/release/cfs-* 2>/dev/null | wc -l) binaries"
}

deploy_to_host() {
  local host="$1"
  local role="$2"
  local key_arg
  key_arg=$(ssh_key_arg)

  info "Deploying to $role node: $host"

  # Create target directory
  ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 $key_arg "ubuntu@${host}" \
    "sudo mkdir -p /opt/claudefs/bin && sudo chown ubuntu:ubuntu /opt/claudefs/bin"

  # Copy binaries based on role
  case "$role" in
    storage)
      for bin in cfs-storage cfs-meta; do
        if [[ -f "$REPO_DIR/target/release/$bin" ]]; then
          scp -o StrictHostKeyChecking=no $key_arg \
            "$REPO_DIR/target/release/$bin" "ubuntu@${host}:/opt/claudefs/bin/$bin"
          info "  Deployed $bin"
        fi
      done
      ;;
    client)
      for bin in cfs-fuse; do
        if [[ -f "$REPO_DIR/target/release/$bin" ]]; then
          scp -o StrictHostKeyChecking=no $key_arg \
            "$REPO_DIR/target/release/$bin" "ubuntu@${host}:/opt/claudefs/bin/$bin"
          info "  Deployed $bin"
        fi
      done
      ;;
    *)
      # Deploy all binaries
      for bin in "$REPO_DIR"/target/release/cfs-*; do
        [[ -f "$bin" ]] || continue
        local name
        name=$(basename "$bin")
        scp -o StrictHostKeyChecking=no $key_arg \
          "$bin" "ubuntu@${host}:/opt/claudefs/bin/$name"
        info "  Deployed $name"
      done
      ;;
  esac

  info "  Done: $host"
}

# --- Commands ---

cmd_build() {
  build_release
}

cmd_deploy() {
  local target="${1:-all}"
  local built=false

  # Build first
  if [[ "$built" != "true" ]]; then
    build_release
    built=true
  fi

  case "$target" in
    storage)
      info "Deploying to storage nodes..."
      local hosts
      hosts=$(get_instances_by_role "storage")
      if [[ -z "$hosts" ]]; then
        warn "No storage nodes found"
        return 1
      fi
      for host in $hosts; do
        deploy_to_host "$host" "storage"
      done
      ;;
    client)
      info "Deploying to client nodes..."
      local hosts
      hosts=$(get_instances_by_role "client")
      if [[ -z "$hosts" ]]; then
        warn "No client nodes found"
        return 1
      fi
      for host in $hosts; do
        deploy_to_host "$host" "client"
      done
      ;;
    all)
      info "Deploying to all cluster nodes..."
      local all_hosts
      all_hosts=$(aws ec2 describe-instances \
        --filters \
          "Name=tag:project,Values=$PROJECT_TAG" \
          "Name=instance-state-name,Values=running" \
        --query 'Reservations[].Instances[].[PublicIpAddress,Tags[?Key==`Role`].Value|[0]]' \
        --output text --region "$REGION" 2>/dev/null)

      if [[ -z "$all_hosts" ]]; then
        warn "No cluster nodes found. Run: cfs-dev up"
        return 1
      fi

      while IFS=$'\t' read -r ip role; do
        [[ -n "$ip" && "$ip" != "None" ]] || continue
        deploy_to_host "$ip" "${role:-storage}"
      done <<< "$all_hosts"
      ;;
    *)
      die "Unknown target: $target. Use: all, storage, client"
      ;;
  esac

  info "Deployment complete!"
}

cmd_verify() {
  info "Verifying deployment across cluster..."
  local all_hosts
  all_hosts=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].PublicIpAddress' \
    --output text --region "$REGION" 2>/dev/null)

  if [[ -z "$all_hosts" ]]; then
    warn "No cluster nodes found"
    return 1
  fi

  local key_arg
  key_arg=$(ssh_key_arg)

  for host in $all_hosts; do
    echo -n "  $host: "
    if ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 $key_arg "ubuntu@${host}" \
      "ls /opt/claudefs/bin/ 2>/dev/null | tr '\n' ' '" 2>/dev/null; then
      echo ""
    else
      echo "(unreachable)"
    fi
  done
}

cmd_help() {
  cat << 'HELP'
cfs-deploy — ClaudeFS build and deploy to test cluster

Usage: cfs-deploy.sh <command> [options]

Commands:
  build                     Build release binaries locally
  deploy [target] [--key KEY]
      Build and deploy to cluster.
      target: all (default) | storage | client
  verify
      Check deployed binaries on all cluster nodes

Environment:
  CFS_KEY_NAME    Default SSH key pair name
HELP
}

# --- Main ---

COMMAND="${1:-help}"
shift || true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key) KEY_NAME="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    *) break ;;
  esac
done

TARGET="${TARGET:-all}"

case "$COMMAND" in
  build)   cmd_build ;;
  deploy)  cmd_deploy "$TARGET" ;;
  verify)  cmd_verify ;;
  help|--help|-h) cmd_help ;;
  *)       die "Unknown command: $COMMAND. Run 'cfs-deploy.sh help'" ;;
esac
