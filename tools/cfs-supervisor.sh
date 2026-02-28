#!/usr/bin/env bash
# cfs-supervisor.sh — Runs Claude Code every 15 minutes via cron to inspect and fix agents
# Cron entry: */15 * * * * cfs /opt/cfs-supervisor.sh >> /var/log/cfs-agents/supervisor.log 2>&1

set -uo pipefail

# Source environment
export HOME="/home/cfs"
[[ -f "$HOME/.bashrc" ]] && source "$HOME/.bashrc" 2>/dev/null || true
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env" 2>/dev/null || true
export CLAUDE_CODE_USE_BEDROCK=1
export AWS_REGION=us-west-2
export PATH="$HOME/.opencode/bin:$HOME/.cargo/bin:$PATH"

REPO_DIR="/home/cfs/claudefs"
LOG="/var/log/cfs-agents/supervisor.log"
LOCKFILE="/tmp/cfs-supervisor.lock"

# Prevent overlapping runs
if [[ -f "$LOCKFILE" ]]; then
  pid=$(cat "$LOCKFILE" 2>/dev/null)
  if kill -0 "$pid" 2>/dev/null; then
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Previous run still active (pid $pid), skipping"
    exit 0
  fi
fi
echo $$ > "$LOCKFILE"
trap 'rm -f "$LOCKFILE"' EXIT

echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] === Starting supervisor check ==="

cd "$REPO_DIR" || exit 1

# Gather diagnostics for Claude
DIAG=$(cat <<DIAGEOF
=== TMUX SESSIONS ===
$(tmux ls 2>&1 || echo "no tmux server")

=== AGENT PROCESSES ===
$(ps aux | grep -E '(claude|opencode|cargo|rustc)' | grep -v grep | head -30)

=== LAST 5 COMMITS ===
$(git log --oneline -5 2>/dev/null)

=== LAST COMMIT AGE ===
$(echo "$(( ($(date +%s) - $(git log -1 --format=%ct 2>/dev/null || echo $(date +%s))) )) seconds ago")

=== UNPUSHED COMMITS ===
$(git log origin/main..HEAD --oneline 2>/dev/null || echo "none")

=== GIT STATUS (dirty files) ===
$(git diff --stat 2>/dev/null | tail -10)

=== CARGO BUILD CHECK ===
$(cd "$REPO_DIR" && cargo check 2>&1 | tail -15)

=== WATCHDOG LOG (last 10 lines) ===
$(tail -10 /var/log/cfs-agents/watchdog.log 2>/dev/null || echo "no watchdog log")

=== AGENT LOG SIZES ===
$(wc -c /var/log/cfs-agents/*.log 2>/dev/null)

=== RUST CODE STATS ===
$(for d in crates/*/src; do crate=\$(basename \$(dirname \$d)); count=\$(find \$d -name '*.rs' 2>/dev/null | wc -l); lines=\$(cat \$d/*.rs 2>/dev/null | wc -l); echo "  \$crate: \$count files, \$lines lines"; done)
DIAGEOF
)

# Run Claude to analyze and take action
PROMPT=$(cat <<PROMPTEOF
You are the ClaudeFS supervisor running on the orchestrator every 15 minutes via cron.
Your job: check agent health, fix problems, push unpushed commits, and ensure progress.

Here is the current state of the system:

$DIAG

## Your tasks (do ALL of these):

1. **Push unpushed commits**: If there are unpushed commits, run git stash, git pull --rebase, git stash pop, git push origin main.

2. **Restart dead agents**: If any agent tmux session (cfs-a1 through cfs-a11) is missing or has no active claude/opencode process, relaunch it with: /opt/cfs-agent-launcher.sh --agent A<N>

3. **Restart the watchdog**: If cfs-watchdog tmux session is missing, start it: tmux new-session -d -s cfs-watchdog "cd /home/cfs/claudefs && /opt/cfs-watchdog.sh 1"

4. **Fix build errors**: If cargo check shows errors, investigate and fix them. Use opencode for any Rust fixes (NEVER write .rs files directly — see CLAUDE.md).

5. **Report**: Print a one-paragraph summary of system health.

Be quick and efficient. Do not re-read all the docs. Just fix what's broken and move on.
PROMPTEOF
)

# Use Sonnet for speed (supervisor doesn't need Opus)
timeout 300 claude --dangerously-skip-permissions \
  --model global.anthropic.claude-sonnet-4-6 \
  -p "$PROMPT" 2>&1 | tail -50

echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] === Supervisor check complete ==="
