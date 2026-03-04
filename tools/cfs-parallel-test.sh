#!/usr/bin/env bash
# cfs-parallel-test.sh — Run cargo tests in parallel across all crates
# Usage: cfs-parallel-test.sh [--release] [--verbose] [CRATE_FILTER]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_DIR" || exit 1

# Parse arguments
RELEASE_BUILD=0
VERBOSE=0
CRATE_FILTER=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE_BUILD=1
      shift
      ;;
    --verbose)
      VERBOSE=1
      shift
      ;;
    --help)
      echo "Usage: $0 [--release] [--verbose] [CRATE_FILTER]"
      echo ""
      echo "Examples:"
      echo "  $0                           # Run all tests in debug mode"
      echo "  $0 --release                 # Run all tests in release mode"
      echo "  $0 claudefs-meta             # Run only claudefs-meta tests"
      echo "  $0 --verbose claudefs-fuse   # Verbose output for claudefs-fuse tests"
      exit 0
      ;;
    *)
      CRATE_FILTER="$1"
      shift
      ;;
  esac
done

# Detect available crates
mapfile -t CRATES < <(find crates -maxdepth 1 -type d -name "claudefs-*" | sort | xargs -n1 basename)

if [[ -z "${CRATES[0]}" ]]; then
  echo "ERROR: No crates found in crates/ directory"
  exit 1
fi

# Filter crates if specified
if [[ -n "$CRATE_FILTER" ]]; then
  CRATES=( "${CRATES[@]}" | grep "$CRATE_FILTER" || true )
  if [[ ${#CRATES[@]} -eq 0 ]]; then
    echo "ERROR: No crates matching filter '$CRATE_FILTER'"
    exit 1
  fi
fi

num_crates=${#CRATES[@]}
echo "Running tests for $num_crates crates: ${CRATES[*]}"

# Prepare build flags
BUILD_FLAGS=""
if [[ $RELEASE_BUILD -eq 1 ]]; then
  BUILD_FLAGS="--release"
fi

VERBOSE_FLAGS=""
if [[ $VERBOSE -eq 1 ]]; then
  VERBOSE_FLAGS="--verbose"
fi

# Temporary directory for results
RESULTS_DIR=$(mktemp -d)
trap 'rm -rf "$RESULTS_DIR"' EXIT

# Function to run tests for a single crate
run_crate_tests() {
  local crate="$1"
  local output_file="$RESULTS_DIR/$crate.txt"
  local status_file="$RESULTS_DIR/$crate.status"

  echo "Testing $crate..."

  if cargo test -p "$crate" $BUILD_FLAGS $VERBOSE_FLAGS > "$output_file" 2>&1; then
    echo "0" > "$status_file"
  else
    echo "1" > "$status_file"
  fi
}

export -f run_crate_tests
export RESULTS_DIR BUILD_FLAGS VERBOSE_FLAGS

# Run tests in parallel using GNU parallel or xargs
if command -v parallel &> /dev/null; then
  # Use GNU parallel if available (better progress reporting)
  parallel -j+0 --halt soon,fail=1 run_crate_tests ::: "${CRATES[@]}"
  PARALLEL_EXIT=$?
else
  # Fallback to xargs (less efficient but still parallel)
  printf '%s\0' "${CRATES[@]}" | xargs -0 -P 0 -I {} bash -c 'run_crate_tests "$@"' _ {}
  PARALLEL_EXIT=${PIPESTATUS[0]}
fi

# Collect results
echo ""
echo "=== TEST RESULTS ==="
echo ""

TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0
FAILED_CRATES=()

for crate in "${CRATES[@]}"; do
  status_file="$RESULTS_DIR/$crate.status"
  output_file="$RESULTS_DIR/$crate.txt"

  if [[ ! -f "$status_file" ]]; then
    echo "⚠️  $crate: NO STATUS (test may have crashed)"
    continue
  fi

  status=$(cat "$status_file")

  # Extract test count from output
  if [[ -f "$output_file" ]]; then
    test_count=$(grep -o "test result: ok\|test result: FAILED" "$output_file" | wc -l)
    if grep -q "test result: ok" "$output_file"; then
      echo "✅ $crate: PASSED"
      ((PASSED_TESTS++))
      if [[ -n "$test_count" && "$test_count" -gt 0 ]]; then
        TOTAL_TESTS=$((TOTAL_TESTS + test_count))
      fi
    else
      echo "❌ $crate: FAILED"
      ((FAILED_TESTS++))
      FAILED_CRATES+=("$crate")
      # Print failed test output for debugging
      if [[ $VERBOSE -eq 1 ]]; then
        tail -20 "$output_file"
        echo ""
      fi
    fi
  fi
done

echo ""
echo "=== SUMMARY ==="
echo "Passed: $PASSED_TESTS/$num_crates"
echo "Failed: $FAILED_TESTS/$num_crates"

if [[ $FAILED_TESTS -gt 0 ]]; then
  echo ""
  echo "Failed crates:"
  for crate in "${FAILED_CRATES[@]}"; do
    echo "  - $crate"
    echo ""
    echo "Details from $RESULTS_DIR/$crate.txt:"
    tail -30 "$RESULTS_DIR/$crate.txt"
    echo ""
  done
  exit 1
else
  echo "All tests passed! ✅"
  exit 0
fi
