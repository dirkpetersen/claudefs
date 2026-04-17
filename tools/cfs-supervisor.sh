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
OPENCODE_FIX_LOG="/var/log/cfs-agents/opencode-fixes.log"

# Ensure log directory exists
mkdir -p /var/log/cfs-agents 2>/dev/null || true

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

# Function to detect and classify build errors (returns error type and complexity)
detect_build_errors() {
  local output="$1"
  local error_type=""
  local complexity="simple"  # simple or complex

  if echo "$output" | grep -q "^error:"; then
    if echo "$output" | grep -q "error\[E0433\]"; then
      error_type="unresolved_module"
    elif echo "$output" | grep -q "error\[E0425\]"; then
      error_type="cannot_find_value"
    elif echo "$output" | grep -q "error\[E0308\]"; then
      error_type="type_mismatch"
      complexity="complex"  # Type errors often need context
    elif echo "$output" | grep -q "error\[E0425\].*MethodValue.*parameter"; then
      error_type="api_mismatch"
      complexity="complex"  # API mismatches need context
    elif echo "$output" | grep -q "error: failed to resolve"; then
      error_type="failed_resolve"
      complexity="complex"
    elif echo "$output" | grep -q "error: no matching package"; then
      error_type="missing_dependency"
    elif echo "$output" | grep -q "could not compile"; then
      error_type="compile_failed"
      complexity="complex"
    else
      error_type="unknown_error"
    fi
  fi

  echo "$error_type:$complexity"
}

# Function to gather error context (affected files, related code snippets)
gather_error_context() {
  local error_output="$1"

  # Extract file paths from error message
  local files=$(echo "$error_output" | grep -oP '(?<==>\s+).*?:\d+' | cut -d: -f1 | sort -u)
  local context=""

  if [[ -n "$files" ]]; then
    context="Affected files context:"$'\n'
    while read -r file; do
      if [[ -f "$file" ]]; then
        line_num=$(grep -n "error:" <<< "$error_output" | head -1 | cut -d: -f1)
        line_num=${line_num:-1}
        start=$((line_num > 3 ? line_num - 3 : 1))
        end=$((line_num + 3))
        context+="$(sed -n "${start},${end}p" "$file" | head -10)"$'\n'
      fi
    done <<< "$files"
  fi

  echo "$context"
}

# Function to attempt automated error recovery via OpenCode
attempt_opencode_fix() {
  local error_output="$1"
  local error_complexity="$2"  # simple or complex
  local retry_count=$(cat "$OPENCODE_RETRY_COUNT" 2>/dev/null || echo 0)

  if [[ $retry_count -ge $MAX_OPENCODE_RETRIES ]]; then
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Max retries ($MAX_OPENCODE_RETRIES) exceeded for OpenCode, escalating to manual review" | tee -a "$OPENCODE_FIX_LOG"
    return 1
  fi

  # Choose model based on error complexity
  local opencode_model="fireworks-ai/accounts/fireworks/models/glm-5"
  if [[ "$error_complexity" == "complex" ]]; then
    opencode_model="fireworks-ai/accounts/fireworks/models/minimax-m2p5"
  fi

  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Attempting OpenCode auto-fix (attempt $((retry_count + 1))/$MAX_OPENCODE_RETRIES) using model: ${opencode_model##*/}" | tee -a "$OPENCODE_FIX_LOG"

  # Gather context
  local error_context=$(gather_error_context "$error_output")

  # Create OpenCode prompt with error context
  local prompt=$(cat <<'FIXEOF'
You are helping fix a build error in the ClaudeFS Rust project.

**Build Error:**
FIXEOF
  )

  prompt+=$'\n'"$error_output"$'\n'

  if [[ -n "$error_context" ]]; then
    prompt+=$'\n'"**Context:**"$'\n'"$error_context"$'\n'
  fi

  prompt+=$(cat <<'FIXEOF2'

**Your Task:**
1. Identify the root cause (missing dependency, type error, API mismatch)
2. Determine which file(s) need changes
3. Write corrected code

**Constraints:**
- Do NOT modify Cargo.toml or Cargo.lock
- Only fix actual Rust code errors (.rs files)
- Follow the existing code style and patterns
- Preserve all comments and structure
- Include brief comments explaining the fix

**Output Format:**
For each file to fix:
```rust
// file: path/to/file.rs
// changes: brief description of what's being fixed
<corrected code block>
```

If you cannot identify a clear fix, respond with:
CANNOT_FIX: <reason>
FIXEOF2
  )

  # Run OpenCode with the prompt
  local opencode_output="/tmp/opencode-fix-${retry_count}.md"
  if echo "$prompt" | timeout 180 ~/.opencode/bin/opencode run /dev/stdin \
    --model "$opencode_model" > "$opencode_output" 2>&1; then

    if grep -q "CANNOT_FIX:" "$opencode_output"; then
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode cannot fix: $(grep 'CANNOT_FIX:' "$opencode_output" | head -1)" | tee -a "$OPENCODE_FIX_LOG"
      return 1
    fi

    # Log the suggested fix
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode suggested a fix, validating..." | tee -a "$OPENCODE_FIX_LOG"
    echo "--- Suggested fix output ---" >> "$OPENCODE_FIX_LOG"
    head -50 "$opencode_output" >> "$OPENCODE_FIX_LOG"
    echo "--- End fix output ---" >> "$OPENCODE_FIX_LOG"

    # Increment retry counter
    echo $((retry_count + 1)) > "$OPENCODE_RETRY_COUNT"

    # Re-run cargo check to validate (timeout after 60s)
    if timeout 60 cargo check 2>&1 | grep -q "error:"; then
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Fix validation failed, error still present" | tee -a "$OPENCODE_FIX_LOG"
      return 1
    fi

    # Also run clippy to catch warnings
    if timeout 60 cargo clippy --all-targets 2>&1 | grep -q "error:"; then
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Fix introduced clippy errors" | tee -a "$OPENCODE_FIX_LOG"
      return 1
    fi

    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ✅ Fix validated successfully!" | tee -a "$OPENCODE_FIX_LOG"
    rm -f "$OPENCODE_RETRY_COUNT"  # Reset retry counter on success
    return 0
  else
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] OpenCode execution failed or timed out" | tee -a "$OPENCODE_FIX_LOG"
    return 1
  fi
}

# Check for build errors and attempt recovery
echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Running cargo check..."
BUILD_OUTPUT=$(timeout 90 cargo check 2>&1)

if echo "$BUILD_OUTPUT" | grep -q "^error:"; then
  ERROR_DETECT=$(detect_build_errors "$BUILD_OUTPUT")
  ERROR_TYPE="${ERROR_DETECT%:*}"
  ERROR_COMPLEXITY="${ERROR_DETECT#*:}"
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] Build error detected: $ERROR_TYPE (complexity: $ERROR_COMPLEXITY)" | tee -a "$OPENCODE_FIX_LOG"
  echo "$BUILD_OUTPUT" > "$ERROR_LOG"

  # Attempt auto-fix for certain error types
  case "$ERROR_TYPE" in
    unresolved_module|failed_resolve|type_mismatch|cannot_find_value|api_mismatch)
      if attempt_opencode_fix "$BUILD_OUTPUT" "$ERROR_COMPLEXITY"; then
        echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ✅ Error fixed by OpenCode" | tee -a "$OPENCODE_FIX_LOG"
      else
        echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ⚠️  OpenCode could not fix error, will retry on next run" | tee -a "$OPENCODE_FIX_LOG"
      fi
      ;;
    missing_dependency)
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ⚠️  Dependency missing - manual review required (not auto-fixable)" | tee -a "$OPENCODE_FIX_LOG"
      ;;
    *)
      echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ⚠️  Unknown error type: $ERROR_TYPE" | tee -a "$OPENCODE_FIX_LOG"
      ;;
  esac
else
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [supervisor] ✅ Build check passed"
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
