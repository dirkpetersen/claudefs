# OpenCode Input: A11 Phase 5 Block 2 Implementation

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Target:** Shell scripts + Rust tests for preemptible instance lifecycle management
**Models:** Use minimax-m2p5 (default), fallback to glm-5 if issues

---

## Executive Summary

Implement **three shell scripts** and **15 Rust tests** for cost-efficient preemptible (spot) instance lifecycle management in ClaudeFS. This unblocks the entire Phase 5 pipeline and provides **60-70% cost savings** with zero manual operations.

**Success Criteria:**
- ✅ All scripts executable, production-ready
- ✅ All 15 tests passing with zero clippy warnings
- ✅ No compiler errors
- ✅ Shell scripts follow Phase 4 patterns (error handling, logging, AWS CLI)
- ✅ Rust tests use comprehensive mocks (no live AWS calls)

---

## Part 1: Shell Scripts

### Script 1: tools/cfs-spot-pricing.sh

**File:** `tools/cfs-spot-pricing.sh`
**LOC:** ~200
**Purpose:** Query AWS Spot prices, calculate economics, make buy/wait decisions

**Requirements:**

1. **Command:** `cfs-spot-pricing query --instance-types <types> --region <region>`
   - Query AWS EC2 DescribeSpotPriceHistory API
   - Parse JSON response for: InstanceType, SpotPrice, Timestamp
   - Output: JSON with fields: instance_type, current_spot_price, on_demand_price, discount_pct
   - Example: `{ "instance_type": "i4i.2xlarge", "current_spot": 0.19, "on_demand": 0.624, "discount": 69.6 }`

2. **Command:** `cfs-spot-pricing should-launch --instance-type <type> --region <region>`
   - Decision logic:
     - Query current spot price and on-demand price
     - Estimate interruption rate from history (optional, can use fixed 2% default)
     - Return "true" if: spot < 50% on-demand AND interruption_rate < 5%
     - Return "false" if: spot > 70% on-demand OR interruption_rate > 10%
     - Return "maybe" if: in between (wait for better prices)
   - Output: Single line "true", "false", or "maybe"

3. **Command:** `cfs-spot-pricing cost-breakdown --cluster <name> --date <YYYY-MM-DD>`
   - Read instance cost data from /tmp/claudefs-cost/<cluster>-<date>.json (format: array of instance objects)
   - Aggregate: total_cost, on_demand_equivalent, savings_pct, disruption_events
   - Output: JSON report with per-instance costs and cluster summary

4. **Helper Functions:**
   - `query_aws_api()` - Call AWS CLI describe-spot-price-history, handle errors
   - `parse_spot_response()` - Extract fields from JSON response
   - `calculate_discount()` - (spot - on_demand) / on_demand * 100
   - `calculate_monthly_savings()` - (on_demand - spot) * 730 hours
   - `log_decision()` - Output decision rationale to stderr

5. **Error Handling:**
   - Fail gracefully if AWS API unreachable (exit 1, log to stderr)
   - Default to on-demand if spot prices unavailable
   - Validate inputs (instance types, regions)

6. **Integration Points:**
   - Used by: `cfs-instance-manager.sh`, `cfs-cost-monitor.sh`
   - Outputs to: stdout (JSON), stderr (logs)
   - No dependencies except: bash, aws-cli, jq, curl

**Reference Implementation Style:**
```bash
#!/bin/bash
set -euo pipefail

main() {
  case "${1:-}" in
    query) query_prices "${@:2}" ;;
    should-launch) should_launch_decision "${@:2}" ;;
    cost-breakdown) cost_breakdown "${@:2}" ;;
    *) usage; exit 1 ;;
  esac
}

query_prices() {
  local instance_types="$1"
  local region="${2:-us-west-2}"

  aws ec2 describe-spot-price-history \
    --instance-types $instance_types \
    --region "$region" \
    --product-descriptions "Linux/UNIX" \
    --query 'SpotPriceHistory[0:1].[InstanceType,SpotPrice,Timestamp]' \
    --output json | jq ...
}

should_launch_decision() {
  # Parse arguments, query prices, apply decision logic
  # Output: true/false/maybe
}

main "$@"
```

---

### Script 2: tools/cfs-instance-manager.sh

**File:** `tools/cfs-instance-manager.sh`
**LOC:** ~300
**Purpose:** Provision, drain, replace, and manage instance lifecycle

**Requirements:**

1. **Command:** `cfs-instance-manager provision --role <role> --site <site> --count <n> --instance-type <type>`
   - role: storage | client | jepsen | conduit
   - site: A | B (for storage nodes)
   - count: number of instances
   - instance-type: i4i.2xlarge | c7a.xlarge | t3.medium
   - Actions:
     1. Generate Terraform variables file: /tmp/cfs-provision-<timestamp>.tfvars
     2. Call: `cd tools/terraform && terraform apply -var-file=/tmp/cfs-provision-*.tfvars`
     3. Wait for instances to reach "running" state (max 5 min timeout)
     4. Tag instances with: Name, Role, Site, CostCenter, Agent, StartTime
     5. Return instance IDs (space-separated)
   - Output: JSON: `{ "instances": ["i-12345", "i-67890"], "status": "ready" }`

2. **Command:** `cfs-instance-manager drain --node-id <id> --timeout <seconds>`
   - Actions:
     1. Mark instance as "draining" in cluster membership
     2. Broadcast to all FUSE clients: "redirect to primary node"
     3. Wait for pending operations to complete (up to timeout)
     4. Flush writes to persistent storage
     5. Checkpoint state to orchestrator
   - Timeout: 120 seconds default (2-min spot interruption window)
   - Output: JSON: `{ "status": "drained", "operations_completed": 42, "elapsed_ms": 8500 }`

3. **Command:** `cfs-instance-manager replace --node-id <old-id> --reason <reason>`
   - reason: spot-interrupted | health-check | manual
   - Actions:
     1. Call `drain --node-id <old-id> --timeout 120`
     2. Terminate old instance via AWS CLI
     3. Launch replacement via `provision` with matching role/site
     4. Tag replacement with: ReplacementOf: <old-id>, DisruptionCount, TotalUptime
     5. Wait for replacement to join cluster (SWIM gossip)
   - Output: JSON: `{ "old_id": "i-old", "new_id": "i-new", "elapsed_ms": 180000 }`

4. **Command:** `cfs-instance-manager status --cluster <name>`
   - Query all instances in cluster via AWS EC2 API
   - For each: instance_id, state, role, uptime, cost_so_far
   - Output: JSON array or human-readable table

5. **Helper Functions:**
   - `wait_for_instance_ready()` - Poll EC2 status, timeout 5 min
   - `tag_instance()` - Apply AWS tags
   - `initiate_cluster_drain()` - Call cluster API (via curl or cfs CLI)
   - `broadcast_to_clients()` - Publish drain notice via SWIM or management API
   - `update_cost_tags()` - Add replacement cost metadata

6. **Integration Points:**
   - Calls: Terraform, AWS CLI, cfs CLI (cluster management)
   - Called by: `cfs-disruption-handler.sh`, `cfs-dev`, `cfs-cost-monitor.sh`
   - Logging: All operations to `/var/log/cfs-instance-manager.log`

---

### Script 3: tools/cfs-disruption-handler.sh

**File:** `tools/cfs-disruption-handler.sh`
**LOC:** ~250
**Purpose:** Detect spot interruption notices and coordinate graceful shutdown

**Requirements:**

1. **Daemon Mode:** Runs continuously (as systemd service)
   - Poll interval: 5 seconds
   - IMDS endpoint: http://169.254.169.254/latest/meta-data/spot/instance-action
   - Protocol: HTTP with IMDSv2 token (X-aws-ec2-metadata-token header)

2. **Detection Logic:**
   - Every 5s: fetch termination notice from EC2 IMDS
   - If response: termination time in next 2 minutes
     - Log: "Spot interruption detected, terminating at <timestamp>"
     - Call: `cfs-instance-manager drain --node-id <self> --timeout 115`
     - Wait for drain to complete or timeout
     - Exit gracefully (AWS will terminate at scheduled time)
   - If no response (404): continue polling

3. **Systemd Service:** `systemd/cfs-spot-monitor.service`
   - ExecStart=/home/cfs/tools/cfs-disruption-handler.sh
   - Restart=on-failure
   - RestartSec=10
   - StandardOutput=journal
   - StandardError=journal

4. **Error Handling:**
   - IMDS timeout: log warning, continue polling
   - Drain failure: attempt forced shutdown after timeout
   - Network error: exponential backoff (5s, 10s, 20s, then give up)

5. **Logging:**
   - All events to: `/var/log/cfs-disruption-handler.log`
   - Also: systemd journal (journalctl -u cfs-spot-monitor)
   - Format: timestamp, event_type, duration_ms, status

6. **Exit Behavior:**
   - Normal exit: 0 (graceful shutdown after drain)
   - Error exit: 1 (forced shutdown or unrecoverable error)
   - systemd will restart on failure (RestartSec=10)

**Reference Implementation:**
```bash
#!/bin/bash
set -euo pipefail

LOG_FILE="/var/log/cfs-disruption-handler.log"
IMDS_ENDPOINT="http://169.254.169.254/latest/meta-data/spot/instance-action"

main() {
  init_logging
  get_node_id

  while true; do
    if check_termination_notice; then
      handle_termination
      exit 0
    fi
    sleep 5
  done
}

check_termination_notice() {
  # Call IMDS with IMDSv2 token
  # Return 0 if notice present, 1 if not
}

handle_termination() {
  log "Termination notice detected"
  # Call cfs-instance-manager drain
  # Wait for completion or timeout
}

main "$@"
```

---

### Systemd Service File

**File:** `systemd/cfs-spot-monitor.service`
**Purpose:** Run disruption handler as persistent daemon

```ini
[Unit]
Description=ClaudeFS Spot Instance Disruption Monitor
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=cfs
ExecStart=/home/cfs/tools/cfs-disruption-handler.sh
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=cfs-spot-monitor

# Resource limits
MemoryLimit=100M
CPUQuota=10%

# Graceful shutdown
KillMode=mixed
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
```

---

## Part 2: Rust Tests

### Test Module: crates/claudefs-tests/src/preemptible_lifecycle_tests.rs

**File:** `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs`
**LOC:** ~500-600
**Target:** 15 tests, zero clippy warnings, all passing

**Test Groups:**

#### Group 1: Spot Pricing (4 tests)

1. **test_spot_pricing_query_valid**
   - Mock AWS API response with instance prices
   - Verify parsing: instance_type, price, timestamp
   - Assert current_spot < on_demand

2. **test_spot_pricing_history_trend**
   - Load 7-day price history
   - Calculate trend: Downward, Stable, Upward
   - Verify trend accuracy ±5%

3. **test_breakeven_calculation**
   - spot=$0.19, on_demand=$0.624
   - Calculate: discount% = 69.6%
   - Assert within ±1%

4. **test_should_launch_decision_logic**
   - Case A: spot=0.19, on-demand=0.624, interruption=2% → true
   - Case B: spot=0.60, on-demand=0.624, interruption=15% → false
   - Verify all cases correct

#### Group 2: Instance Lifecycle (4 tests)

5. **test_provision_instance_success**
   - Mock Terraform apply
   - Verify tags applied (Name, Role, Site, CostCenter, Agent, StartTime)
   - Assert instance online

6. **test_provision_instance_with_retries**
   - Mock 2 transient failures, succeed on 3rd
   - Verify exponential backoff
   - Assert eventual success

7. **test_drain_instance_graceful**
   - Mock 10 pending operations
   - Operations complete in 50ms each (total 500ms < 120s timeout)
   - Assert all operations complete, status = "drained"

8. **test_drain_instance_timeout**
   - Mock 10 slow operations (150ms each = 1.5s total)
   - Set timeout to 0.5s
   - Assert timeout error after 500ms

#### Group 3: Disruption Handling (4 tests)

9. **test_spot_termination_notice_detected**
   - Mock IMDS returning termination notice on 2nd poll
   - Verify detection latency < 20ms
   - Assert notice contains time field

10. **test_disruption_triggers_drain**
    - Simulate spot termination notice
    - Verify drain initiated within 1s
    - Assert status changes to "draining"

11. **test_replacement_launch_after_disruption**
    - Simulate disruption of i-old-123
    - Verify replacement i-new-456 launched
    - Assert ReplacementOf tag = i-old-123

12. **test_concurrent_disruptions**
    - Simulate 3 simultaneous disruptions
    - Verify all 3 drains initiated without deadlock
    - Assert 3 replacements complete

#### Group 4: Cost Tracking (3 tests)

13. **test_instance_cost_calculation**
    - i4i.2xlarge, spot, 2 hours, $0.19/hr
    - Expected cost: $0.38
    - Assert within ±1%

14. **test_replacement_cost_included**
    - Instance A: 8 hrs × $0.19 = $1.52
    - Replacement: 4 hrs × $0.19 = $0.76
    - Total: $2.28
    - Assert chain tracked correctly

15. **test_daily_cost_report_accuracy**
    - 9-instance cluster: 1 orchestrator + 5 storage + 2 clients + 1 conduit
    - Calculate total_cost = sum of individual costs
    - Assert within ±1%
    - Verify savings_percent = (on-demand - spot) / on-demand ±2%

**Mock Fixtures:**

```rust
struct MockTerraform {
    calls: Vec<String>,
    should_fail: bool,
}

struct MockIMDS {
    call_count: usize,
    termination_notice_after: usize,
}

struct MockInstance {
    pending_operations: usize,
    operation_completion_time_ms: u64,
    status: InstanceStatus,
}

struct CostCalculation {
    instance_type: String,
    pricing_model: String,
    hourly_rate: f64,
    uptime_hours: f64,
}
```

**Integration with Test Suite:**
- Add module to `crates/claudefs-tests/src/lib.rs`: `mod preemptible_lifecycle_tests;`
- No external dependencies (all mocks)
- Use Arc<Mutex> or atomic types for concurrent test scenarios
- Async tests where needed (e.g., polling) with #[tokio::test]

---

## Part 3: Quality Checklist

### Shell Scripts
- [ ] `#!/bin/bash` shebang with `set -euo pipefail`
- [ ] All functions documented (comment above each)
- [ ] Error handling: all commands checked with `||`
- [ ] Logging: all important operations logged to file + stderr
- [ ] Variables quoted: all `$var` → `"$var"`
- [ ] AWS CLI calls handle errors (exit codes checked)
- [ ] No hardcoded values (use variables for regions, timeouts, etc.)
- [ ] Executable bit set: `chmod +x tools/cfs-*.sh`
- [ ] Pass shellcheck: `shellcheck tools/cfs-*.sh`

### Rust Tests
- [ ] All 15 tests compile without errors
- [ ] All 15 tests pass with 100% success rate
- [ ] Zero clippy warnings: `cargo clippy -p claudefs-tests`
- [ ] No panics (use Result assertions, not unwrap)
- [ ] Code follows module organization (4 groups + mocks)
- [ ] Each test function documented with purpose
- [ ] Mock fixtures reusable across tests
- [ ] Deterministic (no timing dependencies, no random seeds)
- [ ] Fast (<5 seconds total execution)

### Documentation
- [ ] Code comments on non-obvious logic
- [ ] Function signatures clear (inputs, outputs, errors)
- [ ] Example usage comments for scripts
- [ ] Test assertion messages clear (what failed and why)

---

## Part 4: Integration Points

**With Existing Phase 4 Tools:**
- `cfs-cost-monitor.sh` → Uses `cfs-spot-pricing` for pricing decisions
- `cfs-watchdog.sh` → Integrates with `cfs-instance-manager` for health recovery
- `cfs-supervisor.sh` → Can trigger replacements on failures

**With Phase 5 Block 1 (Terraform):**
- `cfs-instance-manager provision` → Calls Terraform apply
- Both scripts share instance role/site/type conventions

**With ClaudeFS Core:**
- `cfs` CLI → Integration for cluster drain/status (assumed already available)
- SWIM gossip (D2) → Automatic membership when new nodes join
- Multi-Raft (D4) → Automatic shard re-replication on node loss

---

## Part 5: Success Criteria

### Immediate (Testing)
1. ✅ All shell scripts: bash syntax check passes
2. ✅ All scripts: shellcheck -S error passes
3. ✅ All scripts: chmod +x applied
4. ✅ All Rust tests: `cargo test preemptible_lifecycle_tests` → all pass
5. ✅ All Rust tests: `cargo clippy -p claudefs-tests` → zero warnings
6. ✅ Build: `cargo build --release` → no errors

### Deployment (Later)
1. ✅ cfs-disruption-handler runs as systemd service
2. ✅ cfs-spot-pricing integrates with cfs-cost-monitor
3. ✅ cfs-instance-manager handles real spot interruptions
4. ✅ Cost reports aggregate correctly across cluster

---

## References

- **Existing Patterns:** Review Phase 4 scripts (cfs-cost-monitor.sh, cfs-watchdog.sh) for style
- **AWS APIs:**
  - EC2 DescribeSpotPriceHistory: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_DescribeSpotPriceHistory.html
  - EC2 Instance Metadata Service: https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ec2-instance-metadata.html
- **Architecture:** docs/agents.md (D2 SWIM, D4 multi-Raft)
- **Planning:** docs/A11-PHASE5-BLOCK2-PLAN.md (550 lines)
- **Test Spec:** docs/A11-PHASE5-BLOCK2-TESTS.md (320+ lines)

---

## Submission Checklist

When complete, verify:
1. [ ] All files created and executable
2. [ ] `cargo test preemptible_lifecycle_tests` → 15/15 pass
3. [ ] `cargo clippy -p claudefs-tests` → zero warnings
4. [ ] `cargo build --release` → no errors
5. [ ] Git status clean (all files staged)
6. [ ] Commit message: "[A11] Phase 5 Block 2: Preemptible Instance Lifecycle (15 tests, ~1,200 LOC shell scripts)"
7. [ ] Push to GitHub main

---

**Input File:** a11-phase5-block2-input.md
**Created:** 2026-04-18
**Status:** 🟡 READY FOR OPENCODE
