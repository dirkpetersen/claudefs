#!/usr/bin/env bash
# cfs-test-cluster.sh — Run ClaudeFS test suites on the cluster
#
# Runs POSIX test suites, integration tests, and performance benchmarks
# on the provisioned test cluster.
#
# Usage:
#   cfs-test-cluster.sh [--suite unit|posix|fio|all] [--key KEY]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REGION="us-west-2"
PROJECT_TAG="claudefs"
KEY_NAME="${CFS_KEY_NAME:-}"
RESULTS_DIR="/tmp/cfs-test-results-$(date +%Y%m%d-%H%M%S)"

die() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "==> $*"; }
warn() { echo "WARNING: $*" >&2; }

ssh_key_arg() {
  if [[ -n "$KEY_NAME" ]]; then
    for suffix in "" ".pem" ".cer"; do
      local path="$HOME/.ssh/${KEY_NAME}${suffix}"
      if [[ -f "$path" ]]; then echo "-i $path"; return; fi
    done
  fi
}

get_client_ip() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:Role,Values=client" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[0].Instances[0].PublicIpAddress' \
    --output text --region "$REGION" 2>/dev/null | head -1
}

get_storage_ips() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:Role,Values=storage" \
      "Name=tag:Site,Values=A" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].PrivateIpAddress' \
    --output text --region "$REGION" 2>/dev/null
}

run_unit_tests() {
  info "Running unit tests locally (no cluster needed)..."
  mkdir -p "$RESULTS_DIR"

  cd "$REPO_DIR"
  cargo test --workspace 2>&1 | tee "$RESULTS_DIR/unit-tests.log"

  local pass fail
  pass=$(grep "test result: ok" "$RESULTS_DIR/unit-tests.log" | awk '{sum += $4} END {print sum+0}')
  fail=$(grep "test result: FAILED" "$RESULTS_DIR/unit-tests.log" | awk '{sum += $8} END {print sum+0}')

  info "Unit tests: $pass passed, $fail failed"
  echo "Results saved to $RESULTS_DIR/unit-tests.log"

  [[ "$fail" -eq 0 ]]
}

run_posix_tests() {
  local client_ip
  client_ip=$(get_client_ip)

  if [[ -z "$client_ip" || "$client_ip" == "None" ]]; then
    warn "No FUSE client node found. Run: cfs-dev up"
    return 1
  fi

  local key_arg
  key_arg=$(ssh_key_arg)

  info "Running pjdfstest on FUSE client $client_ip..."
  mkdir -p "$RESULTS_DIR"

  ssh -o StrictHostKeyChecking=no $key_arg "ubuntu@${client_ip}" \
    "sudo pjdfstest -s -R /mnt/claudefs 2>&1 | tee /tmp/pjdfstest.log; cat /tmp/pjdfstest.log" \
    > "$RESULTS_DIR/pjdfstest.log" 2>&1

  local passed failed
  passed=$(grep -c "ok$" "$RESULTS_DIR/pjdfstest.log" 2>/dev/null || echo 0)
  failed=$(grep -c "FAILED$" "$RESULTS_DIR/pjdfstest.log" 2>/dev/null || echo 0)

  info "pjdfstest: $passed passed, $failed failed"
  echo "Results saved to $RESULTS_DIR/pjdfstest.log"
}

run_fio_bench() {
  local client_ip
  client_ip=$(get_client_ip)

  if [[ -z "$client_ip" || "$client_ip" == "None" ]]; then
    warn "No client node found"
    return 1
  fi

  local key_arg
  key_arg=$(ssh_key_arg)

  info "Running FIO benchmarks on $client_ip..."
  mkdir -p "$RESULTS_DIR"

  # Sequential read/write
  ssh -o StrictHostKeyChecking=no $key_arg "ubuntu@${client_ip}" "cat > /tmp/fio-seq.ini << 'EOF'
[global]
ioengine=libaio
direct=1
runtime=60
time_based
size=4g
directory=/mnt/claudefs
numjobs=4

[seq-write]
rw=write
bs=1m
iodepth=32

[seq-read]
rw=read
bs=1m
iodepth=32
EOF
fio /tmp/fio-seq.ini --output-format=json 2>&1" > "$RESULTS_DIR/fio-seq.json"

  info "FIO sequential results saved to $RESULTS_DIR/fio-seq.json"

  # Random 4K read/write
  ssh -o StrictHostKeyChecking=no $key_arg "ubuntu@${client_ip}" "cat > /tmp/fio-rand.ini << 'EOF'
[global]
ioengine=libaio
direct=1
runtime=60
time_based
size=1g
directory=/mnt/claudefs
numjobs=4

[rand-write-4k]
rw=randwrite
bs=4k
iodepth=32

[rand-read-4k]
rw=randread
bs=4k
iodepth=32
EOF
fio /tmp/fio-rand.ini --output-format=json 2>&1" > "$RESULTS_DIR/fio-rand.json"

  info "FIO random 4K results saved to $RESULTS_DIR/fio-rand.json"
  info "Parse with: cat $RESULTS_DIR/fio-rand.json | python3 -m json.tool"
}

cmd_help() {
  cat << 'HELP'
cfs-test-cluster — Run ClaudeFS test suites on cluster

Usage: cfs-test-cluster.sh <suite> [options]

Suites:
  unit      Run local unit tests (no cluster needed) — 984 tests
  posix     Run pjdfstest on FUSE client node
  fio       Run FIO benchmarks on FUSE client node
  all       Run all suites

Options:
  --key KEY     SSH key pair name (or set CFS_KEY_NAME env var)

Results are saved to /tmp/cfs-test-results-YYYYMMDD-HHMMSS/

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
    *) break ;;
  esac
done

case "$COMMAND" in
  unit)   run_unit_tests ;;
  posix)  run_posix_tests ;;
  fio)    run_fio_bench ;;
  all)
    run_unit_tests
    run_posix_tests
    run_fio_bench
    ;;
  help|--help|-h) cmd_help ;;
  *)       die "Unknown suite: $COMMAND. Run 'cfs-test-cluster.sh help'" ;;
esac
