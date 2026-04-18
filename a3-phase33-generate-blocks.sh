#!/bin/bash
# Generate Phase 33 Blocks 2-8 via OpenCode (parallel where possible)

set -e

cd /home/cfs/claudefs

# Ensure API key is available
export FIREWORKS_API_KEY=$(aws secretsmanager get-secret-value --secret-id cfs/fireworks-api-key --region us-west-2 --query SecretString --output text 2>/dev/null | jq -r '.FIREWORKS_API_KEY' 2>/dev/null || echo "")
if [ -z "$FIREWORKS_API_KEY" ]; then
  echo "ERROR: Could not retrieve API key"
  exit 1
fi

echo "API key ready (${#FIREWORKS_API_KEY} chars)"

# Function to run OpenCode for a block
run_block() {
  local block=$1
  local input_file="a3-phase33-block${block}-input.md"
  local output_file="a3-phase33-block${block}-output.md"

  if [ ! -f "$input_file" ]; then
    echo "ERROR: $input_file not found"
    return 1
  fi

  echo "[Block $block] Starting OpenCode..."
  ~/.opencode/bin/opencode run "$(cat $input_file)" \
    --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 \
    > "$output_file" 2>&1 &

  local pid=$!
  echo "[Block $block] OpenCode PID: $pid"

  # Wait for completion
  wait $pid
  local status=$?

  if [ $status -eq 0 ]; then
    local lines=$(wc -l < "$output_file")
    echo "[Block $block] ✓ Completed ($lines lines)"
    return 0
  else
    echo "[Block $block] ✗ Failed (exit code $status)"
    return 1
  fi
}

# Run blocks sequentially (each waits for files to be created)
for block in 2 3 4 5 6 7 8; do
  run_block $block || true
  sleep 2
done

echo ""
echo "All blocks submitted to OpenCode."
echo "Check output files: a3-phase33-block{2-8}-output.md"
