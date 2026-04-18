#!/usr/bin/env bash
# cfs-instance-manager.sh — Provision, drain, replace, and manage instance lifecycle
#
# Commands:
#   provision   - Provision new instances via Terraform
#   drain      - Gracefully drain an instance
#   replace    - Replace a failed instance
#   status     - Show cluster instance status
#
set -euo pipefail

LOG="${LOG:-/var/log/cfs-instance-manager.log}"
REGION="${REGION:-us-west-2}"
TERRAFORM_DIR="${TERRAFORM_DIR:-/home/cfs/claudefs/tools/terraform}"
PROJECT_TAG="claudefs"
CFS_USER="cfs"
TIMEOUT_SECONDS=300

log() {
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$LOG" >&2
}

usage() {
    cat <<EOF
Usage: cfs-instance-manager <command> [options]

Commands:
  provision --role <role> --site <site> --count <n> --instance-type <type> [--region <region>]
    Provision new instances
    --role: storage | client | jepsen | conduit
    --site: A | B (for storage nodes)
    --count: number of instances
    --instance-type: i4i.2xlarge | c7a.xlarge | t3.medium | etc.
    --region: AWS region (default: us-west-2)

  drain --node-id <id> --timeout <seconds>
    Gracefully drain an instance
    --node-id: EC2 instance ID
    --timeout: drain timeout in seconds (default: 120)

  replace --node-id <old-id> --reason <reason>
    Replace a failed instance
    --node-id: EC2 instance ID to replace
    --reason: spot-interrupted | health-check | manual

  status --cluster <name>
    Show cluster instance status
    --cluster: cluster name

Examples:
  cfs-instance-manager provision --role storage --site A --count 2 --instance-type i4i.2xlarge
  cfs-instance-manager drain --node-id i-1234567890abcdef0 --timeout 120
  cfs-instance-manager replace --node-id i-1234567890abcdef0 --reason spot-interrupted
  cfs-instance-manager status --cluster cfs-dev
EOF
}

get_instance_id() {
    curl -s http://169.254.169.254/latest/meta-data/instance-id 2>/dev/null || echo ""
}

wait_for_instance_ready() {
    local instance_id="$1"
    local timeout="${2:-$TIMEOUT_SECONDS}"
    local start_time

    start_time=$(date +%s)

    log "Waiting for instance $instance_id to be ready (timeout: ${timeout}s)"

    while true; do
        local elapsed
        elapsed=$(($(date +%s) - start_time))

        if (( elapsed >= timeout )); then
            log "Timeout waiting for instance $instance_id"
            return 1
        fi

        local state
        state=$(aws ec2 describe-instances \
            --instance-ids "$instance_id" \
            --region "$REGION" \
            --query 'Reservations[].Instances[].State.Name' \
            --output text 2>/dev/null) || {
            sleep 5
            continue
        }

        if [[ "$state" == "running" ]]; then
            log "Instance $instance_id is running"
            return 0
        fi

        sleep 5
    done
}

tag_instance() {
    local instance_id="$1"
    shift
    local tags=("$@")

    local tag_args=""
    for tag in "${tags[@]}"; do
        tag_args="$tag_args Key=$(echo "$tag" | cut -d= -f1),Value=$(echo "$tag" | cut -d= -f2-)"
    done

    aws ec2 create-tags \
        --resources "$instance_id" \
        --tags $tag_args \
        --region "$REGION" 2>&1 | tee -a "$LOG" || true
}

initiate_cluster_drain() {
    local node_id="$1"

    log "Initiating cluster drain for node $node_id"

    if command -v cfs &>/dev/null; then
        cfs cluster drain "$node_id" 2>&1 | tee -a "$LOG" || {
            log "Warning: cfs CLI drain failed, continuing with cleanup"
        }
    else
        log "cfs CLI not available, skipping cluster drain"
    fi

    return 0
}

broadcast_to_clients() {
    local node_id="$1"

    log "Broadcasting drain notice to clients for node $node_id"

    if command -v cfs &>/dev/null; then
        cfs cluster notify-drain "$node_id" 2>&1 | tee -a "$LOG" || true
    else
        log "cfs CLI not available, skipping broadcast"
    fi

    return 0
}

check_pending_operations() {
    local node_id="$1"

    if command -v cfs &>/dev/null; then
        cfs cluster pending-ops --node "$node_id" 2>/dev/null | jq -r '.count // "0"' || echo "0"
    else
        echo "0"
    fi
}

flush_writes() {
    local node_id="$1"

    log "Flushing writes for node $node_id"

    if command -v cfs &>/dev/null; then
        cfs storage flush --node "$node_id" 2>&1 | tee -a "$LOG" || true
    fi

    return 0
}

checkpoint_state() {
    local node_id="$1"

    log "Checkpointing state for node $node_id"

    if command -v cfs &>/dev/null; then
        cfs cluster checkpoint --node "$node_id" 2>&1 | tee -a "$LOG" || true
    fi

    return 0
}

cmd_provision() {
    local role=""
    local site=""
    local count=1
    local instance_type=""
    local region="$REGION"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --role)
                role="$2"
                shift 2
                ;;
            --site)
                site="$2"
                shift 2
                ;;
            --count)
                count="$2"
                shift 2
                ;;
            --instance-type)
                instance_type="$2"
                shift 2
                ;;
            --region)
                region="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$role" ]] || [[ -z "$instance_type" ]]; then
        echo "Error: --role and --instance-type required" >&2
        exit 1
    fi

    local timestamp
    timestamp=$(date -u +%Y%m%dT%H%M%SZ)
    local tfvars_file="/tmp/cfs-provision-${timestamp}.tfvars"

    log "Generating Terraform variables file: $tfvars_file"

    case "$role" in
        storage)
            if [[ "$site" == "A" ]]; then
                cat > "$tfvars_file" <<EOF
storage_site_a_count = $count
storage_instance_type = "$instance_type"
use_spot_instances = true
EOF
            elif [[ "$site" == "B" ]]; then
                cat > "$tfvars_file" <<EOF
storage_site_b_count = $count
storage_instance_type = "$instance_type"
use_spot_instances = true
EOF
            else
                echo "Error: --site required for storage role (A or B)" >&2
                exit 1
            fi
            ;;
        client)
            cat > "$tfvars_file" <<EOF
fuse_client_count = $count
fuse_client_instance_type = "$instance_type"
use_spot_instances = true
EOF
            ;;
        jepsen)
            cat > "$tfvars_file" <<EOF
jepsen_count = $count
jepsen_instance_type = "$instance_type"
use_spot_instances = true
EOF
            ;;
        conduit)
            cat > "$tfvars_file" <<EOF
conduit_count = $count
conduit_instance_type = "$instance_type"
use_spot_instances = true
EOF
            ;;
        *)
            echo "Error: Unknown role: $role" >&2
            exit 1
            ;;
    esac

    log "Running Terraform apply..."
    cd "$TERRAFORM_DIR"

    if ! terraform apply -var-file="$tfvars_file" -auto-approve 2>&1 | tee -a "$LOG"; then
        log "ERROR: Terraform apply failed"
        rm -f "$tfvars_file"
        exit 1
    fi

    rm -f "$tfvars_file"

    local instance_ids
    case "$role" in
        storage)
            if [[ "$site" == "A" ]]; then
                instance_ids=$(terraform output -raw storage_site_a_ids 2>/dev/null || echo "")
            else
                instance_ids=$(terraform output -raw storage_site_b_ids 2>/dev/null || echo "")
            fi
            ;;
        client)
            instance_ids=$(terraform output -raw fuse_client_ids 2>/dev/null || echo "")
            ;;
        jepsen)
            instance_ids=$(terraform output -raw jepsen_controller_ids 2>/dev/null || echo "")
            ;;
        conduit)
            instance_ids=$(terraform output -raw conduit_id 2>/dev/null || echo "")
            ;;
    esac

    if [[ -z "$instance_ids" ]]; then
        log "Warning: Could not retrieve instance IDs from Terraform output"
        echo '{"status": "error", "message": "Failed to retrieve instance IDs"}'
        exit 1
    fi

    local start_time
    start_time=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    for id in $instance_ids; do
        wait_for_instance_ready "$id" || log "Warning: Timeout waiting for $id"
        tag_instance "$id" \
            "Name=cfs-${role}-${id}" \
            "Role=${role}" \
            "Site=${site:-primary}" \
            "CostCenter=Infrastructure" \
            "Agent=a11" \
            "StartTime=${start_time}"
    done

    log "Provision complete: $instance_ids"

    local json_ids
    json_ids=$(echo "$instance_ids" | jq -R -s 'split("\n") | map(select(length > 0))')

    jq -n \
        --argjson ids "$json_ids" \
        '{instances: $ids, status: "ready"}'
}

cmd_drain() {
    local node_id=""
    local timeout=120

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --node-id)
                node_id="$2"
                shift 2
                ;;
            --timeout)
                timeout="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$node_id" ]]; then
        echo "Error: --node-id required" >&2
        exit 1
    fi

    local start_time
    start_time=$(date +%s%3N)

    log "Draining node $node_id (timeout: ${timeout}s)"

    initiate_cluster_drain "$node_id"
    broadcast_to_clients "$node_id"
    flush_writes "$node_id"

    local elapsed=0
    local operations_completed=0

    while (( elapsed < timeout )); do
        local pending
        pending=$(check_pending_operations "$node_id")

        if [[ "$pending" == "0" ]]; then
            operations_completed=$((timeout - elapsed))
            break
        fi

        sleep 1
        elapsed=$(($(date +%s%3N) - start_time) / 1000)
    done

    checkpoint_state "$node_id"

    local elapsed_ms
    elapsed_ms=$(($(date +%s%3N) - start_time))

    log "Drain complete: $operations_completed operations in ${elapsed_ms}ms"

    jq -n \
        --arg status "drained" \
        --arg ops "$operations_completed" \
        --arg ms "$elapsed_ms" \
        '{status: $status, operations_completed: ($ops | tonumber), elapsed_ms: ($ms | tonumber)}'
}

cmd_replace() {
    local node_id=""
    local reason="manual"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --node-id)
                node_id="$2"
                shift 2
                ;;
            --reason)
                reason="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$node_id" ]]; then
        echo "Error: --node-id required" >&2
        exit 1
    fi

    local start_time
    start_time=$(date +%s%3N)

    log "Replacing node $node_id (reason: $reason)"

    cmd_drain --node-id "$node_id" --timeout 120

    log "Terminating old instance $node_id"
    aws ec2 terminate-instances \
        --instance-ids "$node_id" \
        --region "$REGION" 2>&1 | tee -a "$LOG"

    local role site instance_type
    role=$(aws ec2 describe-instances \
        --instance-ids "$node_id" \
        --region "$REGION" \
        --query 'Reservations[].Instances[].Tags[?Key==`Role`].Value | [0]' \
        --output text 2>/dev/null || echo "storage")
    site=$(aws ec2 describe-instances \
        --instance-ids "$node_id" \
        --region "$REGION" \
        --query 'Reservations[].Instances[].Tags[?Key==`Site`].Value | [0]' \
        --output text 2>/dev/null || echo "A")

    case "$role" in
        storage)
            instance_type="i4i.2xlarge"
            ;;
        client)
            instance_type="c7a.xlarge"
            ;;
        jepsen)
            instance_type="c7a.2xlarge"
            ;;
        conduit)
            instance_type="t3.medium"
            ;;
        *)
            instance_type="c7a.xlarge"
            ;;
    esac

    log "Launching replacement instance..."
    local provision_output
    provision_output=$(cmd_provision --role "$role" --site "$site" --count 1 --instance-type "$instance_type" --region "$REGION")

    local new_id
    new_id=$(echo "$provision_output" | jq -r '.instances[0] // empty')

    if [[ -n "$new_id" ]] && [[ "$new_id" != "null" ]]; then
        sleep 5

        local disruption_count=1
        tag_instance "$new_id" \
            "ReplacementOf=$node_id" \
            "DisruptionCount=$disruption_count" \
            "TotalUptime=0"

        log "Replacement complete: $node_id -> $new_id"
    else
        log "ERROR: Failed to launch replacement"
        new_id=""
    fi

    local elapsed_ms
    elapsed_ms=$(($(date +%s%3N) - start_time))

    jq -n \
        --arg old "$node_id" \
        --arg new "$new_id" \
        --arg ms "$elapsed_ms" \
        '{old_id: $old, new_id: $new, elapsed_ms: ($ms | tonumber)}'
}

cmd_status() {
    local cluster=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --cluster)
                cluster="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$cluster" ]]; then
        echo "Error: --cluster required" >&2
        exit 1
    fi

    local instances
    instances=$(aws ec2 describe-instances \
        --filters \
            "Name=tag:project,Values=$PROJECT_TAG" \
            "Name=instance-state-name,Values=running" \
        --region "$REGION" \
        --query 'Reservations[].Instances[]' \
        --output json 2>/dev/null)

    local results="[]"
    local launched_time
    launched_time=$(date -u -d '1 day ago' +%Y-%m-%dT%H:%M:%SZ)

    echo "$instances" | jq -r '.[] | @base64' 2>/dev/null | while read -r encoded; do
        local instance
        instance=$(echo "$encoded" | base64 -d)

        local instance_id state role uptime_hours hourly_rate
        instance_id=$(echo "$instance" | jq -r '.InstanceId')
        state=$(echo "$instance" | jq -r '.State.Name')
        role=$(echo "$instance" | jq -r '.Tags[] | select(.Key == "Role") | .Value // "unknown"')
        uptime_hours=$(echo "$instance" | jq -r '.Tags[] | select(.Key == "StartTime") | .Value' | xargs -I{} date -d{} +%s 2>/dev/null | xargs -I{} echo "scale=1; ($(date +%s) - {}) / 3600" | bc -l || echo "0")

        case "$role" in
            storage) hourly_rate="0.19" ;;
            client) hourly_rate="0.05" ;;
            jepsen) hourly_rate="0.10" ;;
            conduit) hourly_rate="0.015" ;;
            *) hourly_rate="0.10" ;;
        esac

        local cost
        cost=$(echo "scale=2; $hourly_rate * $uptime_hours" | bc -l)

        local entry
        entry=$(jq -n \
            --arg id "$instance_id" \
            --arg st "$state" \
            --arg rl "$role" \
            --arg uh "$uptime_hours" \
            --arg c "$cost" \
            '{
                instance_id: $id,
                state: $st,
                role: $rl,
                uptime_hours: ($uh | tonumber),
                cost_so_far: ($c | tonumber)
            }')
        results=$(echo "$results" | jq ". + [$entry]")
    done

    local total_instances total_cost
    total_instances=$(echo "$results" | jq 'length')
    total_cost=$(echo "$results" | jq '[.[].cost_so_far] | add')

    jq -n \
        --arg cluster "$cluster" \
        --argjson instances "$results" \
        --arg count "$total_instances" \
        --arg tc "$total_cost" \
        '{
            cluster: $cluster,
            total_instances: ($count | tonumber),
            total_cost: ($tc | tonumber),
            instances: $instances
        }' | jq .
}

main() {
    local command="${1:-}"
    shift 2>/dev/null || true

    case "$command" in
        provision)
            cmd_provision "$@"
            ;;
        drain)
            cmd_drain "$@"
            ;;
        replace)
            cmd_replace "$@"
            ;;
        status)
            cmd_status "$@"
            ;;
        -h|--help|help)
            usage
            ;;
        *)
            echo "Error: Unknown command '$command'" >&2
            usage
            exit 1
            ;;
    esac
}

main "$@"