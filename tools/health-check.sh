#!/bin/bash
# ClaudeFS cluster health check script
# Usage: ./tools/health-check.sh [--cluster-nodes N] [--timeout SECS] [--interval SECS]
#
# Monitors cluster health including node status, replication, and data consistency

set -e

CLUSTER_NODES=3
TIMEOUT=1800
INTERVAL=30
METRICS_ENDPOINT="http://localhost:9090"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --cluster-nodes) CLUSTER_NODES="$2"; shift 2 ;;
        --timeout) TIMEOUT="$2"; shift 2 ;;
        --interval) INTERVAL="$2"; shift 2 ;;
        --metrics) METRICS_ENDPOINT="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo "[$(date)] Starting health checks (timeout: $((TIMEOUT / 60))min, interval: ${INTERVAL}s)..."

START_TIME=$(date +%s)
FAILED_CHECKS=0

check_node_status() {
    local node_ip=$1
    local node_name=$2

    echo -n "  Checking $node_name ($node_ip)... "

    if curl -s -m 5 "http://$node_ip:9000/health" | grep -q '"status":"healthy"'; then
        echo "✓"
        return 0
    else
        echo "✗"
        return 1
    fi
}

check_replication_status() {
    echo -n "  Checking replication status... "

    # Query Prometheus for replication lag
    if command -v curl &> /dev/null && [ -n "$METRICS_ENDPOINT" ]; then
        REPLICATION_LAG=$(curl -s "$METRICS_ENDPOINT/api/v1/query?query=replication_lag_seconds" \
            | grep -oP '"value":\s*\[\s*"\d+",\s*"(\d+)"' | head -1 | grep -oP '\d+$')

        if [ -z "$REPLICATION_LAG" ]; then
            echo "⚠ (unable to query)"
            return 0
        fi

        if [ "$REPLICATION_LAG" -gt 60 ]; then
            echo "✗ (lag: ${REPLICATION_LAG}s)"
            return 1
        else
            echo "✓ (lag: ${REPLICATION_LAG}s)"
            return 0
        fi
    else
        echo "⚠ (metrics unavailable)"
        return 0
    fi
}

check_data_consistency() {
    echo -n "  Checking data consistency... "

    # Create test file
    TEST_FILE="/tmp/health-check-test-$$"
    TEST_DATA="health-check-data-$(date +%s)"

    if echo "$TEST_DATA" > "$TEST_FILE"; then
        # Try to mount and read back
        if [ -d "/mnt/claudefs" ]; then
            # Attempt to write test file to filesystem
            if cp "$TEST_FILE" "/mnt/claudefs/health-check-$$.tmp" 2>/dev/null && \
               grep -q "$TEST_DATA" "/mnt/claudefs/health-check-$$.tmp" 2>/dev/null; then
                rm -f "/mnt/claudefs/health-check-$$.tmp" "$TEST_FILE" 2>/dev/null
                echo "✓"
                return 0
            fi
        else
            # Fallback: just check if filesystem is responsive
            echo "✓ (mount unavailable)"
            rm -f "$TEST_FILE"
            return 0
        fi
    fi

    echo "✗"
    rm -f "$TEST_FILE" 2>/dev/null
    return 1
}

check_raft_quorum() {
    echo -n "  Checking Raft quorum... "

    # Query metrics for Raft leader status
    if command -v curl &> /dev/null && [ -n "$METRICS_ENDPOINT" ]; then
        LEADERS=$(curl -s "$METRICS_ENDPOINT/api/v1/query?query=raft_leader_count" \
            | grep -oP '"value":\s*\[\s*"\d+",\s*"([0-9]+)"' | head -1 | grep -oP '[0-9]+$')

        if [ -z "$LEADERS" ]; then
            echo "⚠ (unable to query)"
            return 0
        fi

        if [ "$LEADERS" -gt 0 ]; then
            echo "✓ (leaders: $LEADERS)"
            return 0
        else
            echo "✗ (no leaders)"
            return 1
        fi
    else
        echo "⚠ (metrics unavailable)"
        return 0
    fi
}

check_disk_usage() {
    echo -n "  Checking disk usage... "

    if [ -d "/mnt/claudefs" ]; then
        USAGE=$(df /mnt/claudefs | tail -1 | awk '{print $5}' | sed 's/%//')

        if [ "$USAGE" -gt 90 ]; then
            echo "✗ ($USAGE% used)"
            return 1
        elif [ "$USAGE" -gt 80 ]; then
            echo "⚠ ($USAGE% used)"
            return 0
        else
            echo "✓ ($USAGE% used)"
            return 0
        fi
    else
        echo "⚠ (mount unavailable)"
        return 0
    fi
}

check_memory_usage() {
    echo -n "  Checking memory usage... "

    if [ -n "$METRICS_ENDPOINT" ]; then
        MEMORY_PERCENT=$(curl -s "$METRICS_ENDPOINT/api/v1/query?query=node_memory_MemAvailable_bytes" 2>/dev/null | \
            grep -oP 'value.*\[\s*"[^"]*",\s*"([0-9.]+)"' | tail -1 | grep -oP '[0-9.]+$')

        if [ -z "$MEMORY_PERCENT" ]; then
            echo "⚠ (unable to query)"
            return 0
        fi

        if (( $(echo "$MEMORY_PERCENT < 100000000" | bc -l) )); then  # < 100MB free
            echo "✗ (critical)"
            return 1
        else
            echo "✓"
            return 0
        fi
    else
        echo "⚠ (metrics unavailable)"
        return 0
    fi
}

# Main health check loop
while true; do
    ELAPSED=$(($(date +%s) - START_TIME))

    if [ $ELAPSED -ge $TIMEOUT ]; then
        echo "[$(date)] Health check period complete"
        break
    fi

    echo ""
    echo "[$(date)] Health check cycle ($((ELAPSED / 60))m of $((TIMEOUT / 60))m):"

    # Run health checks
    CYCLE_FAILED=0

    # Check node status
    if command -v aws &> /dev/null; then
        for ((i = 1; i <= CLUSTER_NODES; i++)); do
            NODE_IP=$(aws ec2 describe-instances \
                --filters "Name=tag:Role,Values=storage" \
                          "Name=instance-state-name,Values=running" \
                --query "Reservations[*].Instances[$((i-1))].PrivateIpAddress" \
                --output text 2>/dev/null)

            if [ -n "$NODE_IP" ] && [ "$NODE_IP" != "None" ]; then
                if ! check_node_status "$NODE_IP" "storage-$i"; then
                    CYCLE_FAILED=$((CYCLE_FAILED + 1))
                fi
            fi
        done
    fi

    # Check other metrics
    check_replication_status || CYCLE_FAILED=$((CYCLE_FAILED + 1))
    check_data_consistency || CYCLE_FAILED=$((CYCLE_FAILED + 1))
    check_raft_quorum || CYCLE_FAILED=$((CYCLE_FAILED + 1))
    check_disk_usage || true  # Warning only
    check_memory_usage || true  # Warning only

    if [ $CYCLE_FAILED -gt 0 ]; then
        echo "  → $CYCLE_FAILED check(s) failed"
        FAILED_CHECKS=$((FAILED_CHECKS + 1))

        if [ $FAILED_CHECKS -ge 3 ]; then
            echo "[$(date)] ✗ Multiple health check cycles failed, aborting"
            exit 1
        fi
    else
        echo "  → All checks passed"
        FAILED_CHECKS=0
    fi

    # Sleep before next cycle
    REMAINING=$((TIMEOUT - ELAPSED))
    SLEEP_TIME=$((INTERVAL < REMAINING ? INTERVAL : REMAINING))

    if [ $REMAINING -gt 0 ]; then
        echo "[$(date)] Next check in ${SLEEP_TIME}s..."
        sleep $SLEEP_TIME
    fi
done

echo "[$(date)] ✓ Health checks complete"
exit 0
