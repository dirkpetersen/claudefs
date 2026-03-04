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
ERROR_LOG="/tmp/cfs-build-errors.log"
OPENCODE_RETRY_COUNT="/tmp/cfs-opencode-retries"
MAX_OPENCODE_RETRIES=3

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

# Function to detect and classify build errors
detect_build_errors() {
  local output="$1"
  local error_type=""

  if echo "$output" | grep -q "^error:"; then
    if echo "$output" | grep -q "error\[E0433\]"; then
      error_type="unresolved_module"
    elif echo "$output" | grep -q "error\[E0425\]"; then
      error_type="cannot_find_value"
    elif echo "$output" | grep -q "error\[E0308\]"; then
      error_type="type_mismatch"
    elif echo "$output" | grep -q "error: failed to resolve"; then
      error_type="failed_resolve"
    elif echo "$output" | grep -q "error: no matching package"; then
      error_type="missing_dependency"
    elif echo "$output" | grep -q "could not compile"; then
      error_type="compile_failed"
    else
      error_type="unknown_error"
    fi
  fi

  echo "$error_type"
}

# Function to attempt automated error recovery via OpenCode
attempt_opencode_fix() {
  local error_output="$1"
  local retry_count=$(cat "$OPENCODE_RETRY_COUNT" 2>/dev/null || echo 0)

  if [[ $retry_count -ge $MAX_OPENCODE_RETRIES ]]; then
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Max retries ($MAX_OPENCODE_RETRIES) exceeded for OpenCode, escalating to manual review"
    return 1
  fi

  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Attempting OpenCode auto-fix (attempt $((retry_count + 1))/$MAX_OPENCODE_RETRIES)"

  # Create OpenCode prompt with error context
  local prompt=$(cat <<'FIXEOF'
You are helping fix a build error in the ClaudeFS Rust project.

The latest cargo check error:
FIXEOF
  )

  prompt+=$'\n'"$error_output"$'\n'

  prompt+=$(cat <<'FIXEOF2'

Your task: Fix this error by:
1. Identifying the root cause (missing dependency, type error, API mismatch)
2. Determining which file(s) need changes
3. Writing corrected code

Constraints:
- Do NOT modify Cargo.toml directly (dependency issues should be reviewed first)
- Only fix actual Rust code errors (.rs files)
- Follow the existing code style and patterns
- Include brief comments explaining the fix

Output format:
```rust
// file: path/to/file.rs
// changes: brief description
<corrected code>
```

If you cannot identify a clear fix, respond with "CANNOT_FIX: <reason>"
FIXEOF2
  )

  # Run OpenCode with the prompt
  if echo "$prompt" | ~/.opencode/bin/opencode run /dev/stdin \
    --model fireworks-ai/accounts/fireworks/models/glm-5 > /tmp/opencode-fix.md 2>&1; then

    if grep -q "CANNOT_FIX:" /tmp/opencode-fix.md; then
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode cannot fix this error, manual review needed"
      return 1
    fi

    # Attempt to extract and apply the fix
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode suggested a fix, validating..."

    # Increment retry counter
    echo $((retry_count + 1)) > "$OPENCODE_RETRY_COUNT"

    # Re-run cargo check to validate
    if cargo check 2>&1 | grep -q "error:"; then
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Fix validation failed, error still present"
      return 1
    fi

    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Fix validated successfully!"
    rm -f "$OPENCODE_RETRY_COUNT"  # Reset retry counter on success
    return 0
  else
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode execution failed"
    return 1
  fi
}

# Check for build errors and attempt recovery
echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Running cargo check..."
BUILD_OUTPUT=$(cargo check 2>&1)

if echo "$BUILD_OUTPUT" | grep -q "^error:"; then
  ERROR_TYPE=$(detect_build_errors "$BUILD_OUTPUT")
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Build error detected: $ERROR_TYPE"
  echo "$BUILD_OUTPUT" > "$ERROR_LOG"

  # Attempt auto-fix for certain error types
  case "$ERROR_TYPE" in
    unresolved_module|failed_resolve|type_mismatch|cannot_find_value)
      if attempt_opencode_fix "$BUILD_OUTPUT"; then
        echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Error fixed by OpenCode"
      else
        echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode could not fix error, will try again on next run"
      fi
      ;;
    missing_dependency)
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Dependency missing - manual review required (not auto-fixable)"
      ;;
    *)
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Unknown error type: $ERROR_TYPE"
      ;;
  esac
else
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Build check passed"
  rm -f "$ERROR_LOG" "$OPENCODE_RETRY_COUNT"
fi

# Gather diagnostics
gather_rust_stats() {
  for d in crates/*/src; do
    crate=$(basename "$(dirname "$d")")
    count=$(find "$d" -name '*.rs' 2>/dev/null | wc -l)
    lines=$(cat "$d"/*.rs 2>/dev/null | wc -l)
    echo "  $crate: $count files, $lines lines"
  done
}

last_commit_age() {
  local now ts
  now=$(date +%s)
  ts=$(git log -1 --format=%ct 2>/dev/null || echo "$now")
  echo "$(( now - ts )) seconds ago"
}

DIAG="=== TMUX SESSIONS ===
$(tmux ls 2>&1 || echo 'no tmux server')

=== AGENT PROCESSES ===
$(ps aux | grep -E '(claude|opencode|cargo|rustc)' | grep -v grep | head -30)

=== LAST 5 COMMITS ===
$(git log --oneline -5 2>/dev/null)

=== LAST COMMIT AGE ===
$(last_commit_age)

=== UNPUSHED COMMITS ===
$(git log origin/main..HEAD --oneline 2>/dev/null || echo 'none')

=== GIT STATUS (dirty files) ===
$(git diff --stat 2>/dev/null | tail -10)

=== CARGO BUILD CHECK ===
$(cargo check 2>&1 | tail -15)

=== WATCHDOG LOG (last 10 lines) ===
$(tail -10 /var/log/cfs-agents/watchdog.log 2>/dev/null || echo 'no watchdog log')

=== AGENT LOG SIZES ===
$(wc -c /var/log/cfs-agents/*.log 2>/dev/null)

=== RUST CODE STATS ===
$(gather_rust_stats)"

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

4. **Handle persistent build errors**: If cargo check still shows errors after OpenCode auto-fix attempts, investigate the root cause (check if dependency is missing, if API changed in another crate, etc.). Only delegate back to OpenCode if the error looks fixable.

5. **Report**: Print a one-paragraph summary of system health including:
   - Agent session status (running/dead/idle)
   - Last commit time and age
   - Build status (passing/failing)
   - Any OpenCode auto-fixes attempted
   - Overall system health (green/yellow/red)

Be quick and efficient. Do not re-read all the docs. Just fix what's broken and move on.
PROMPTEOF
)

# Use Haiku if budget exceeded, otherwise Sonnet
SUPERVISOR_MODEL="us.anthropic.claude-haiku-4-5-20251001-v1:0"

timeout 300 claude --dangerously-skip-permissions \
  --model "$SUPERVISOR_MODEL" \
  -p "$PROMPT" 2>&1 | tail -50

echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] === Supervisor check complete ==="
