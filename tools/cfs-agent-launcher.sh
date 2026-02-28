#!/usr/bin/env bash
# cfs-agent-launcher.sh — Launches ClaudeFS agents as tmux sessions running Claude Code
# Each agent gets its role docs, decisions, and conventions as context.
# Usage: cfs-agent-launcher.sh [--phase 1|2|3] [--agent A1|A2|...]

set -euo pipefail

REPO_DIR="/home/cfs/claudefs"
PHASE="${1:-1}"
LOG_DIR="/var/log/cfs-agents"
mkdir -p "$LOG_DIR"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --phase) PHASE="$2"; shift 2 ;;
    --agent) SINGLE_AGENT="$2"; shift 2 ;;
    *) shift ;;
  esac
done

# Model assignments per agent
declare -A AGENT_MODEL=(
  [A1]="global.anthropic.claude-opus-4-6-v1"
  [A2]="global.anthropic.claude-opus-4-6-v1"
  [A3]="global.anthropic.claude-sonnet-4-6-v1"
  [A4]="global.anthropic.claude-opus-4-6-v1"
  [A5]="global.anthropic.claude-sonnet-4-6-v1"
  [A6]="global.anthropic.claude-sonnet-4-6-v1"
  [A7]="global.anthropic.claude-sonnet-4-6-v1"
  [A8]="global.anthropic.claude-sonnet-4-6-v1[1m]"
  [A9]="global.anthropic.claude-sonnet-4-6-v1"
  [A10]="global.anthropic.claude-opus-4-6-v1"
  [A11]="us.anthropic.claude-haiku-4-5-20251001-v1:0"
)

# Agent descriptions and prompts
declare -A AGENT_NAME=(
  [A1]="Storage Engine"
  [A2]="Metadata Service"
  [A3]="Data Reduction"
  [A4]="Transport"
  [A5]="FUSE Client"
  [A6]="Replication"
  [A7]="Protocol Gateways"
  [A8]="Management"
  [A9]="Test & Validation"
  [A10]="Security Audit"
  [A11]="Infrastructure & CI"
)

declare -A AGENT_CRATE=(
  [A1]="claudefs-storage"
  [A2]="claudefs-meta"
  [A3]="claudefs-reduce"
  [A4]="claudefs-transport"
  [A5]="claudefs-fuse"
  [A6]="claudefs-repl"
  [A7]="claudefs-gateway"
  [A8]="claudefs-mgmt"
)

# Phase-based agent selection
case "$PHASE" in
  1) AGENTS=(A1 A2 A3 A4 A11) ;;
  2) AGENTS=(A3 A5 A6 A7 A8 A9 A10 A11) ;;
  3) AGENTS=(A1 A2 A3 A4 A5 A6 A7 A8 A9 A10 A11) ;;
  *) echo "Unknown phase: $PHASE"; exit 1 ;;
esac

# If single agent requested, override
if [[ -n "${SINGLE_AGENT:-}" ]]; then
  AGENTS=("$SINGLE_AGENT")
fi

launch_agent() {
  local agent_id="$1"
  local model="${AGENT_MODEL[$agent_id]}"
  local name="${AGENT_NAME[$agent_id]}"
  local crate="${AGENT_CRATE[$agent_id]:-}"
  local session_name="cfs-${agent_id,,}"
  local log_file="${LOG_DIR}/${agent_id}.log"

  # Skip if already running
  if tmux has-session -t "$session_name" 2>/dev/null; then
    echo "[$agent_id] Session already running: $session_name"
    return
  fi

  # Build the agent prompt
  local crate_info=""
  if [[ -n "$crate" ]]; then
    crate_info="You own the crate \`$crate\` in the Cargo workspace."
  fi

  local prompt
  prompt=$(cat << PROMPT_EOF
You are agent $agent_id: $name for the ClaudeFS distributed file system.
$crate_info

Your task: Read CLAUDE.md, docs/decisions.md, and docs/agents.md to understand
the full architecture. Then implement your assigned subsystem following the
conventions and decisions documented there.

Key rules:
1. Prefix every commit with [$agent_id] — e.g. "[$agent_id] Implement block allocator"
2. Commit early, commit often. Push after every commit.
3. Follow shared conventions: thiserror, serde+bincode, tokio+io_uring, tracing crate.
4. Keep unsafe code isolated and well-documented (only in A1/A4/A5/A7).
5. Write tests alongside implementation (proptest for data transforms).
6. If blocked on another agent's work, create a GitHub Issue tagged with both agents.
7. Update CHANGELOG.md when completing a milestone.

Current phase: $PHASE
Your model: $model

Start by reading the project docs, then begin implementing. Work autonomously
and push your progress to GitHub continuously.
PROMPT_EOF
  )

  echo "[$agent_id] Launching $name (model: $model, session: $session_name)"

  tmux new-session -d -s "$session_name" \
    "cd $REPO_DIR && ANTHROPIC_MODEL=$model claude --print --dangerously-skip-permissions -p '$prompt' 2>&1 | tee $log_file"

  echo "[$agent_id] Started. Logs: $log_file"
}

echo "=== ClaudeFS Agent Launcher ==="
echo "Phase: $PHASE"
echo "Agents: ${AGENTS[*]}"
echo "Repo: $REPO_DIR"
echo ""

for agent_id in "${AGENTS[@]}"; do
  launch_agent "$agent_id"
  # Small delay between launches to avoid thundering herd on Bedrock
  sleep 5
done

echo ""
echo "=== All agents launched ==="
echo "Monitor with: tmux ls"
echo "Attach to agent: tmux attach -t cfs-a1"
echo "View logs: tail -f $LOG_DIR/A*.log"
