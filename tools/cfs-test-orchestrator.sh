#!/usr/bin/env bash
# cfs-test-orchestrator.sh — Provision and run multi-node ClaudeFS tests
#
# This script provisions a test cluster (if needed), deploys ClaudeFS,
# runs comprehensive test suites (POSIX, integration, performance),
# collects results, and generates an HTML report.
#
# Orchestrates all test phases:
#   1. Provision: Create 10-node cluster (3 storage, 2 metadata, 2 clients, 1 conduit, 2 spare)
#   2. Deploy: Build and deploy ClaudeFS binaries to all nodes
#   3. Test: Run POSIX suites, integration tests, performance benchmarks
#   4. Report: Collect results and generate HTML
#   5. Cleanup: Tear down test cluster (if --teardown specified)
#
# Usage:
#   cfs-test-orchestrator.sh [--provision] [--deploy] [--test all] [--report] [--teardown]
#   cfs-test-orchestrator.sh --skip-provision --test all  (use existing cluster)
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REGION="us-west-2"
PROJECT_TAG="claudefs"
TEST_CLUSTER_SIZE=10
RESULTS_DIR="/tmp/cfs-multinode-results-$(date +%Y%m%d-%H%M%S)"
LOG_FILE="${RESULTS_DIR}/orchestrator.log"
HTML_REPORT="${RESULTS_DIR}/report.html"

# Test configuration
POSIX_SUITE_TIMEOUT=3600     # 1 hour for pjdfstest
INTEGRATION_TEST_TIMEOUT=1800 # 30 minutes
PERF_BENCH_TIMEOUT=1800      # 30 minutes

die() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "==> $*" | tee -a "$LOG_FILE"; }
warn() { echo "WARNING: $*" >&2 | tee -a "$LOG_FILE"; }
debug() { [[ "${DEBUG:-0}" == "1" ]] && echo "DEBUG: $*" >> "$LOG_FILE"; }

# Initialize
init_results() {
  mkdir -p "$RESULTS_DIR"
  > "$LOG_FILE"
  info "Multi-node test orchestration started at $(date)"
  info "Results directory: $RESULTS_DIR"
}

# --- Phase 1: Provision Test Cluster ---

provision_cluster() {
  info "PHASE 1: Provisioning $TEST_CLUSTER_SIZE-node test cluster..."

  # Check if cluster already exists
  local existing_count
  existing_count=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[] | length(@)' \
    --output text --region "$REGION" 2>/dev/null || echo 0)

  if [[ $existing_count -ge $TEST_CLUSTER_SIZE ]]; then
    info "Cluster already exists with $existing_count nodes, skipping provisioning"
    return 0
  fi

  info "Provisioning nodes (this may take 10-15 minutes)..."

  # Create launch template
  info "Creating launch template..."
  aws ec2 create-launch-template \
    --launch-template-name "claudefs-test-node" \
    --version-description "ClaudeFS test cluster node" \
    --launch-template-data '{
      "ImageId": "ami-0c55b159cbfafe1f0",
      "InstanceType": "c6i.2xlarge",
      "KeyName": "'"${CFS_KEY_NAME:-orchestrator}"'",
      "SecurityGroupIds": ["sg-0123456789abcdef0"],
      "TagSpecifications": [{
        "ResourceType": "instance",
        "Tags": [
          {"Key": "project", "Value": "'"$PROJECT_TAG"'"},
          {"Key": "test-cluster", "Value": "true"},
          {"Key": "managed-by", "Value": "cfs-test-orchestrator"}
        ]
      }]
    }' --region "$REGION" 2>/dev/null || true

  # Launch instances
  local node_config=(
    "3:storage:A"    # 3 storage nodes in Site A
    "2:metadata:A"   # 2 metadata nodes in Site A
    "2:client:A"     # 2 client nodes in Site A
    "1:conduit:AB"   # 1 conduit node for cross-site replication
    "2:spare:A"      # 2 spare nodes
  )

  for config in "${node_config[@]}"; do
    IFS=':' read -r count role site <<< "$config"
    for ((i=1; i<=count; i++)); do
      info "Launching $role node $i in Site $site..."
      aws ec2 run-instances \
        --image-id ami-0c55b159cbfafe1f0 \
        --instance-type c6i.2xlarge \
        --key-name "${CFS_KEY_NAME:-orchestrator}" \
        --tag-specifications \
          "ResourceType=instance,Tags=[
            {Key=project,Value=$PROJECT_TAG},
            {Key=test-cluster,Value=true},
            {Key=Role,Value=$role},
            {Key=Site,Value=$site},
            {Key=managed-by,Value=cfs-test-orchestrator}
          ]" \
        --region "$REGION" \
        --query 'Instances[0].InstanceId' \
        --output text >> "${RESULTS_DIR}/provisioned-instances.txt" 2>/dev/null || true
    done
  done

  info "Waiting for instances to reach running state..."
  aws ec2 wait instance-running \
    --filters "Name=tag:test-cluster,Values=true" \
    --region "$REGION" 2>/dev/null || warn "Some instances may not be running"

  info "Waiting for instance status checks..."
  sleep 60

  local final_count
  final_count=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[] | length(@)' \
    --output text --region "$REGION" 2>/dev/null || echo 0)

  info "✅ PHASE 1 COMPLETE: $final_count nodes provisioned"
}

skip_provision() {
  info "Skipping cluster provisioning (using existing cluster)"
}

# --- Phase 2: Deploy ClaudeFS ---

deploy_cluster() {
  info "PHASE 2: Building and deploying ClaudeFS..."

  # Build release binaries
  info "Building release binaries (this may take 15-20 minutes)..."
  cd "$REPO_DIR"
  cargo build --release --workspace 2>&1 | tee -a "$LOG_FILE" | grep -E "Compiling|Finished" || true

  # Get storage nodes
  local storage_nodes
  storage_nodes=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=tag:Role,Values=storage" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].[PublicIpAddress]' \
    --output text --region "$REGION" 2>/dev/null || echo "")

  local metadata_nodes
  metadata_nodes=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=tag:Role,Values=metadata" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].[PublicIpAddress]' \
    --output text --region "$REGION" 2>/dev/null || echo "")

  [[ -z "$storage_nodes" ]] && warn "No storage nodes found, skipping deployment" && return 1

  # Copy binaries to nodes
  info "Copying binaries to storage nodes..."
  for ip in $storage_nodes; do
    info "Deploying to storage node $ip..."
    scp -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
      "$REPO_DIR/target/release/cfs" \
      "ubuntu@${ip}:/tmp/cfs" 2>/dev/null || warn "Failed to copy to $ip"
  done

  info "Copying binaries to metadata nodes..."
  for ip in $metadata_nodes; do
    info "Deploying to metadata node $ip..."
    scp -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
      "$REPO_DIR/target/release/cfs" \
      "ubuntu@${ip}:/tmp/cfs" 2>/dev/null || warn "Failed to copy to $ip"
  done

  info "✅ PHASE 2 COMPLETE: Binaries deployed"
}

skip_deploy() {
  info "Skipping deployment (using existing binaries on nodes)"
}

# --- Phase 3: Run Test Suites ---

run_posix_tests() {
  info "Running POSIX test suite (pjdfstest)..."

  local client_nodes
  client_nodes=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=tag:Role,Values=client" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].[PublicIpAddress]' \
    --output text --region "$REGION" 2>/dev/null || echo "")

  [[ -z "$client_nodes" ]] && { warn "No client nodes found"; return 1; }

  for ip in $client_nodes; do
    info "Running pjdfstest on client $ip..."
    local test_log="${RESULTS_DIR}/pjdfstest-${ip}.log"

    timeout "$POSIX_SUITE_TIMEOUT" ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
      "ubuntu@${ip}" \
      "cd /tmp && sudo pjdfstest -s -R /mnt/claudefs 2>&1" \
      > "$test_log" 2>&1 || warn "pjdfstest timed out or failed on $ip"

    local passed failed
    passed=$(grep -c "^ok$" "$test_log" 2>/dev/null || echo 0)
    failed=$(grep -c "^FAILED$" "$test_log" 2>/dev/null || echo 0)
    info "pjdfstest on $ip: $passed passed, $failed failed"
  done

  info "✅ POSIX tests complete"
}

run_integration_tests() {
  info "Running integration test suite (local unit tests)..."

  cd "$REPO_DIR"
  timeout "$INTEGRATION_TEST_TIMEOUT" cargo test --workspace --release 2>&1 | tee -a "${RESULTS_DIR}/integration-tests.log" || \
    warn "Integration tests timed out or failed"

  info "✅ Integration tests complete"
}

run_perf_benchmarks() {
  info "Running performance benchmarks..."

  local client_nodes
  client_nodes=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=tag:Role,Values=client" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].[PublicIpAddress]' \
    --output text --region "$REGION" 2>/dev/null || echo "")

  [[ -z "$client_nodes" ]] && { warn "No client nodes found"; return 1; }

  for ip in $client_nodes; do
    info "Running FIO benchmarks on client $ip..."
    local bench_log="${RESULTS_DIR}/fio-${ip}.log"

    timeout "$PERF_BENCH_TIMEOUT" ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
      "ubuntu@${ip}" \
      "cat > /tmp/fio.ini << 'EOF'
[global]
ioengine=libaio
direct=1
runtime=120
time_based
size=10g
directory=/mnt/claudefs

[seq-write]
rw=write
bs=1m
iodepth=32
numjobs=4

[seq-read]
rw=read
bs=1m
iodepth=32
numjobs=4

[rand-4k-write]
rw=randwrite
bs=4k
iodepth=32
numjobs=4

[rand-4k-read]
rw=randread
bs=4k
iodepth=32
numjobs=4
EOF
fio /tmp/fio.ini --output-format=json 2>&1" \
      > "$bench_log" 2>&1 || warn "FIO benchmarks timed out or failed on $ip"

    info "FIO results saved to $bench_log"
  done

  info "✅ Performance benchmarks complete"
}

run_tests() {
  local test_suite="${1:-all}"

  info "PHASE 3: Running test suites ($test_suite)..."

  case "$test_suite" in
    all)
      run_posix_tests
      run_integration_tests
      run_perf_benchmarks
      ;;
    posix)
      run_posix_tests
      ;;
    integration)
      run_integration_tests
      ;;
    perf)
      run_perf_benchmarks
      ;;
    *)
      die "Unknown test suite: $test_suite"
      ;;
  esac

  info "✅ PHASE 3 COMPLETE: All tests run"
}

# --- Phase 4: Generate Report ---

generate_report() {
  info "PHASE 4: Generating HTML report..."

  local test_count=0
  local pass_count=0
  local fail_count=0

  # Count test results
  for log in "$RESULTS_DIR"/*.log; do
    [[ -f "$log" ]] || continue
    test_count=$((test_count + $(grep -c "^ok$" "$log" 2>/dev/null || echo 0)))
    pass_count=$((pass_count + $(grep -c "^ok$" "$log" 2>/dev/null || echo 0)))
    fail_count=$((fail_count + $(grep -c "^FAILED$" "$log" 2>/dev/null || echo 0)))
  done

  # Generate HTML report
  cat > "$HTML_REPORT" << 'EOF'
<!DOCTYPE html>
<html>
<head>
    <title>ClaudeFS Multi-Node Test Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .header { background: #333; color: white; padding: 20px; border-radius: 5px; }
        .section { background: white; margin: 20px 0; padding: 20px; border-radius: 5px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .metric { display: inline-block; margin: 10px 20px 10px 0; }
        .metric-value { font-size: 24px; font-weight: bold; color: #0066cc; }
        .metric-label { font-size: 12px; color: #666; }
        .pass { color: #28a745; font-weight: bold; }
        .fail { color: #dc3545; font-weight: bold; }
        .warn { color: #ffc107; font-weight: bold; }
        table { width: 100%; border-collapse: collapse; margin: 10px 0; }
        th { background: #f0f0f0; padding: 10px; text-align: left; border-bottom: 2px solid #ddd; }
        td { padding: 10px; border-bottom: 1px solid #ddd; }
        tr:hover { background: #f9f9f9; }
        .footer { color: #666; font-size: 12px; margin-top: 40px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>ClaudeFS Multi-Node Test Report</h1>
        <p>Generated: <span id="report-time"></span></p>
    </div>

    <div class="section">
        <h2>Test Summary</h2>
        <div class="metric">
            <div class="metric-value"><span id="total-tests">0</span></div>
            <div class="metric-label">Total Tests</div>
        </div>
        <div class="metric">
            <div class="metric-value pass"><span id="passed-tests">0</span></div>
            <div class="metric-label">Passed</div>
        </div>
        <div class="metric">
            <div class="metric-value fail"><span id="failed-tests">0</span></div>
            <div class="metric-label">Failed</div>
        </div>
        <div class="metric">
            <div class="metric-value"><span id="pass-rate">0</span>%</div>
            <div class="metric-label">Pass Rate</div>
        </div>
    </div>

    <div class="section">
        <h2>Test Suites</h2>
        <table>
            <tr>
                <th>Suite</th>
                <th>Tests</th>
                <th>Passed</th>
                <th>Failed</th>
                <th>Status</th>
            </tr>
            <tr>
                <td>POSIX (pjdfstest)</td>
                <td id="posix-total">-</td>
                <td id="posix-passed">-</td>
                <td id="posix-failed">-</td>
                <td id="posix-status">-</td>
            </tr>
            <tr>
                <td>Integration (Unit Tests)</td>
                <td id="integration-total">-</td>
                <td id="integration-passed">-</td>
                <td id="integration-failed">-</td>
                <td id="integration-status">-</td>
            </tr>
            <tr>
                <td>Performance (FIO)</td>
                <td id="perf-total">-</td>
                <td id="perf-passed">-</td>
                <td id="perf-failed">-</td>
                <td id="perf-status">-</td>
            </tr>
        </table>
    </div>

    <div class="section">
        <h2>Test Nodes</h2>
        <table>
            <tr>
                <th>Node Type</th>
                <th>Count</th>
                <th>Status</th>
            </tr>
            <tr>
                <td>Storage Nodes</td>
                <td id="storage-count">-</td>
                <td id="storage-status">-</td>
            </tr>
            <tr>
                <td>Metadata Nodes</td>
                <td id="metadata-count">-</td>
                <td id="metadata-status">-</td>
            </tr>
            <tr>
                <td>Client Nodes</td>
                <td id="client-count">-</td>
                <td id="client-status">-</td>
            </tr>
        </table>
    </div>

    <div class="section">
        <h2>Recommendations</h2>
        <ul id="recommendations">
            <li>Review test logs for details on failures</li>
            <li>Check node logs (/var/log/cfs) on cluster nodes</li>
            <li>For performance tuning: review FIO results</li>
        </ul>
    </div>

    <div class="footer">
        <p>Report generated by cfs-test-orchestrator.sh</p>
        <p>Results directory: <code>RESULTS_DIR</code></p>
        <p>Full logs and test output available in results directory</p>
    </div>

    <script>
        document.getElementById('report-time').textContent = new Date().toLocaleString();
        document.getElementById('total-tests').textContent = 'TOTAL_TESTS';
        document.getElementById('passed-tests').textContent = 'PASS_COUNT';
        document.getElementById('failed-tests').textContent = 'FAIL_COUNT';
        document.getElementById('pass-rate').textContent = 'PASS_RATE';
    </script>
</body>
</html>
EOF

  # Replace placeholders
  sed -i "s|RESULTS_DIR|$RESULTS_DIR|g" "$HTML_REPORT"
  sed -i "s|TOTAL_TESTS|$test_count|g" "$HTML_REPORT"
  sed -i "s|PASS_COUNT|$pass_count|g" "$HTML_REPORT"
  sed -i "s|FAIL_COUNT|$fail_count|g" "$HTML_REPORT"

  if [[ $test_count -gt 0 ]]; then
    local pass_rate=$((pass_count * 100 / test_count))
    sed -i "s|PASS_RATE|$pass_rate|g" "$HTML_REPORT"
  else
    sed -i "s|PASS_RATE|0|g" "$HTML_REPORT"
  fi

  info "HTML report generated: $HTML_REPORT"
  info "✅ PHASE 4 COMPLETE: Report generated"
}

# --- Phase 5: Cleanup ---

teardown_cluster() {
  info "PHASE 5: Tearing down test cluster..."

  # Get all test cluster instances
  local instance_ids
  instance_ids=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:test-cluster,Values=true" \
      "Name=instance-state-name,Values=running,stopped" \
    --query 'Reservations[].Instances[].InstanceId' \
    --output text --region "$REGION" 2>/dev/null || echo "")

  if [[ -n "$instance_ids" ]]; then
    info "Terminating instances: $instance_ids"
    aws ec2 terminate-instances --instance-ids $instance_ids --region "$REGION" > /dev/null 2>&1
    info "Instances terminated (may take 2-5 minutes)"
  fi

  info "✅ PHASE 5 COMPLETE: Cleanup initiated"
}

skip_teardown() {
  info "Skipping teardown (cluster will remain available for manual inspection)"
  info "To destroy manually: aws ec2 terminate-instances --instance-ids <ids> --region $REGION"
}

# Help
cmd_help() {
  cat << 'HELP'
cfs-test-orchestrator — Provision and run multi-node ClaudeFS tests

Usage: cfs-test-orchestrator.sh [options]

Options:
  --provision       Provision new 10-node test cluster (default: provision)
  --skip-provision  Use existing cluster (skip provisioning)
  --deploy          Deploy binaries to cluster (default: deploy)
  --skip-deploy     Skip deployment (use existing binaries)
  --test SUITE      Run tests (all|posix|integration|perf) (default: all)
  --report          Generate HTML report (default: report)
  --teardown        Tear down cluster after tests (default: skip-teardown)
  --skip-teardown   Keep cluster running for inspection
  --help            Show this help

Phases:
  1. Provision    Create test cluster (if --provision)
  2. Deploy       Build and deploy binaries
  3. Test         Run POSIX, integration, and performance tests
  4. Report       Generate HTML test report
  5. Cleanup      Tear down cluster (if --teardown)

Defaults:
  - Provisions new 10-node cluster
  - Deploys built binaries
  - Runs all test suites
  - Generates report
  - Keeps cluster for inspection

Quick start:
  # Full end-to-end test
  cfs-test-orchestrator.sh

  # Use existing cluster
  cfs-test-orchestrator.sh --skip-provision --test all

  # Test and cleanup
  cfs-test-orchestrator.sh --teardown

Results:
  - Saved to: /tmp/cfs-multinode-results-YYYYMMDD-HHMMSS/
  - HTML report: report.html
  - Test logs: *.log files
  - Instance IDs: provisioned-instances.txt

Requirements:
  - AWS CLI configured
  - EC2 credentials with proper permissions
  - VPC and security groups configured
  - SSH key pair available

Estimated times:
  - Provisioning: 10-15 minutes
  - Deployment: 5-10 minutes
  - POSIX tests: 30-60 minutes
  - Integration tests: 10-20 minutes
  - Performance: 10-15 minutes
  - Total: 60-120 minutes

Environment:
  CFS_KEY_NAME      SSH key pair name
  DEBUG=1           Enable debug output
  REGION            AWS region (default: us-west-2)
HELP
}

# --- Main ---
init_results

# Parse command line options
SHOULD_PROVISION=1
SHOULD_DEPLOY=1
SHOULD_REPORT=1
SHOULD_TEARDOWN=0
TEST_SUITE="all"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provision)       SHOULD_PROVISION=1; shift ;;
    --skip-provision)  SHOULD_PROVISION=0; shift ;;
    --deploy)          SHOULD_DEPLOY=1; shift ;;
    --skip-deploy)     SHOULD_DEPLOY=0; shift ;;
    --test)            TEST_SUITE="$2"; shift 2 ;;
    --report)          SHOULD_REPORT=1; shift ;;
    --skip-report)     SHOULD_REPORT=0; shift ;;
    --teardown)        SHOULD_TEARDOWN=1; shift ;;
    --skip-teardown)   SHOULD_TEARDOWN=0; shift ;;
    --help|-h)         cmd_help; exit 0 ;;
    *)                 die "Unknown option: $1" ;;
  esac
done

# Run orchestration phases
[[ $SHOULD_PROVISION -eq 1 ]] && provision_cluster || skip_provision
[[ $SHOULD_DEPLOY -eq 1 ]] && deploy_cluster || skip_deploy
run_tests "$TEST_SUITE"
[[ $SHOULD_REPORT -eq 1 ]] && generate_report
[[ $SHOULD_TEARDOWN -eq 1 ]] && teardown_cluster || skip_teardown

info ""
info "========================================"
info "Multi-Node Test Orchestration Complete"
info "========================================"
info "Results saved to: $RESULTS_DIR"
info "HTML Report: $HTML_REPORT"
info ""
