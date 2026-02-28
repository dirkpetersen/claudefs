#!/usr/bin/env bash
# cfs-watchdog.sh — Supervisor that monitors agents and takes corrective action
# Runs as a persistent tmux session on the orchestrator.
# Checks every 2 minutes: are agents alive? are they committing? push unpushed work.

set -uo pipefail

REPO_DIR="/home/cfs/claudefs"
LOG="/var/log/cfs-agents/watchdog.log"
CHECK_INTERVAL=120          # seconds between checks
STALE_THRESHOLD=600         # seconds without commit before restarting an agent
PUSH_INTERVAL=180           # push unpushed commits every 3 minutes
PHASE="${1:-1}"

# Source environment
[[ -f "$HOME/.bashrc" ]] && source "$HOME/.bashrc" 2>/dev/null || true
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env" 2>/dev/null || true

log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [watchdog] $*" | tee -a "$LOG"; }

# Phase-based agent list
case "$PHASE" in
  1) AGENTS=(A1 A2 A3 A4 A11) ;;
  2) AGENTS=(A3 A5 A6 A7 A8 A9 A10 A11) ;;
  3) AGENTS=(A1 A2 A3 A4 A5 A6 A7 A8 A9 A10 A11) ;;
esac

push_if_needed() {
  cd "$REPO_DIR" || return
  local unpushed
  unpushed=$(git log origin/main..HEAD --oneline 2>/dev/null | wc -l)
  if (( unpushed > 0 )); then
    log "Pushing $unpushed unpushed commit(s)..."
    # Pull first to avoid rejection
    git stash -q 2>/dev/null
    git pull --rebase origin main 2>/dev/null
    git stash pop -q 2>/dev/null || true
    if git push origin main 2>>"$LOG"; then
      log "Push successful"
    else
      log "Push failed — will retry next cycle"
    fi
  fi
}

check_agent() {
  local agent_id="$1"
  local session_name="cfs-${agent_id,,}"

  # Check if tmux session exists
  if ! tmux has-session -t "$session_name" 2>/dev/null; then
    log "$agent_id: session DEAD — relaunching"
    /opt/cfs-agent-launcher.sh --agent "$agent_id"
    return
  fi

  # Check if claude process is running in the session
  local pane_pid
  pane_pid=$(tmux list-panes -t "$session_name" -F '#{pane_pid}' 2>/dev/null)
  if [[ -n "$pane_pid" ]]; then
    # Check if any child process (claude or opencode) is running
    local children
    children=$(pstree -p "$pane_pid" 2>/dev/null | grep -cE '(claude|opencode|cargo|rustc)')
    if (( children == 0 )); then
      # Session exists but no active work — check if it's truly idle
      local pane_content
      pane_content=$(tmux capture-pane -t "$session_name" -p 2>/dev/null | tail -3)
      if echo "$pane_content" | grep -qiE '(error|failed|denied|exit|panic)'; then
        log "$agent_id: session has errors — killing and relaunching"
        tmux kill-session -t "$session_name" 2>/dev/null
        sleep 2
        /opt/cfs-agent-launcher.sh --agent "$agent_id"
      else
        log "$agent_id: session idle (no claude/opencode/cargo processes) — relaunching"
        tmux kill-session -t "$session_name" 2>/dev/null
        sleep 2
        /opt/cfs-agent-launcher.sh --agent "$agent_id"
      fi
      return
    fi
  fi

  log "$agent_id: alive and working"
}

report_status() {
  cd "$REPO_DIR" || return
  local total_commits
  total_commits=$(git log --oneline --since="1 hour ago" | wc -l)
  local total_lines
  total_lines=$(find crates -name '*.rs' -exec cat {} + 2>/dev/null | wc -l)
  local active_sessions
  active_sessions=$(tmux ls 2>/dev/null | grep -c cfs-a || echo 0)

  log "STATUS: $active_sessions agent sessions, $total_commits commits in last hour, $total_lines lines of Rust"
}

# --- Main loop ---
log "=== Watchdog started (phase $PHASE, checking every ${CHECK_INTERVAL}s) ==="
log "Monitoring agents: ${AGENTS[*]}"

LOOP=0
while true; do
  LOOP=$((LOOP + 1))

  # Check each agent
  for agent_id in "${AGENTS[@]}"; do
    check_agent "$agent_id"
  done

  # Push unpushed commits
  push_if_needed

  # Status report every 5 cycles (~10 minutes)
  if (( LOOP % 5 == 0 )); then
    report_status
  fi

  sleep "$CHECK_INTERVAL"
done
