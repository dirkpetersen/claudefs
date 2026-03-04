#!/usr/bin/env bash
# cfs-build-cache.sh — Manage build artifact caching in S3 for faster deployments
# Usage: cfs-build-cache.sh [status|get|put|clean|clear]

set -uo pipefail

# Configuration
S3_BUCKET="claudefs-build-cache"
S3_REGION="us-west-2"
CACHE_DIR="${HOME}/.cfs-build-cache"
MAX_CACHED_BINARIES=10

# Get git commit SHA (short form)
get_git_sha() {
  git rev-parse --short HEAD 2>/dev/null || echo "unknown"
}

# Get Cargo.lock hash to detect dependency changes
get_cargo_hash() {
  sha256sum Cargo.lock 2>/dev/null | awk '{print $1}' || echo "unknown"
}

# Generate cache key
generate_cache_key() {
  local sha=$(get_git_sha)
  local cargo_hash=$(get_cargo_hash)
  echo "${sha}-${cargo_hash}"
}

# Check if binary exists in S3
check_cache() {
  local cache_key=$(generate_cache_key)
  local s3_path="s3://${S3_BUCKET}/binaries/${cache_key}/cfs.tar.gz"

  echo "Checking cache for: $cache_key"

  if aws s3 ls "$s3_path" --region "$S3_REGION" &>/dev/null; then
    echo "✅ Cache HIT: $s3_path"
    return 0
  else
    echo "❌ Cache MISS: $s3_path"
    return 1
  fi
}

# Download binary from S3
get_from_cache() {
  local cache_key=$(generate_cache_key)
  local s3_path="s3://${S3_BUCKET}/binaries/${cache_key}/cfs.tar.gz"
  local local_path="${CACHE_DIR}/cfs-${cache_key}.tar.gz"

  mkdir -p "$CACHE_DIR"

  echo "Downloading from cache: $cache_key"
  if aws s3 cp "$s3_path" "$local_path" --region "$S3_REGION"; then
    echo "✅ Downloaded to $local_path"
    tar -xzf "$local_path" -C "$(dirname "$local_path")"
    echo "✅ Extracted binary"
    return 0
  else
    echo "❌ Failed to download from cache"
    return 1
  fi
}

# Upload binary to S3
put_to_cache() {
  local cache_key=$(generate_cache_key)
  local binary_path="${1:-.}"
  local s3_path="s3://${S3_BUCKET}/binaries/${cache_key}/cfs.tar.gz"
  local temp_tar=$(mktemp)

  echo "Uploading to cache: $cache_key"

  # Create tarball of release binary
  if tar -czf "$temp_tar" -C "$(dirname "$binary_path")" "$(basename "$binary_path")" 2>/dev/null; then
    if aws s3 cp "$temp_tar" "$s3_path" --region "$S3_REGION"; then
      echo "✅ Uploaded to $s3_path"
      # Add metadata
      aws s3api put-object-tagging \
        --bucket "$S3_BUCKET" \
        --key "binaries/${cache_key}/cfs.tar.gz" \
        --tagging 'TagSet=[{Key=built-date,Value='$(date -u +%Y-%m-%d)'},{Key=git-sha,Value='$(get_git_sha)'}]' \
        --region "$S3_REGION" 2>/dev/null || true
      rm "$temp_tar"
      return 0
    else
      echo "❌ Failed to upload to S3"
      rm "$temp_tar"
      return 1
    fi
  else
    echo "❌ Failed to create tarball"
    rm "$temp_tar"
    return 1
  fi
}

# Show cache status and inventory
show_status() {
  echo "=== ClaudeFS Build Cache Status ==="
  echo ""
  echo "Current Git SHA: $(get_git_sha)"
  echo "Cargo.lock Hash: $(get_cargo_hash)"
  echo "Cache Key: $(generate_cache_key)"
  echo ""

  if ! command -v aws &>/dev/null; then
    echo "⚠️  AWS CLI not found, cannot check S3 cache"
    return 1
  fi

  echo "S3 Bucket: s3://${S3_BUCKET}/"
  echo ""

  # List cached binaries
  echo "=== Cached Binaries (Latest 20) ==="
  aws s3 ls "s3://${S3_BUCKET}/binaries/" --recursive --region "$S3_REGION" \
    | sort -k1,2 -r \
    | head -20 \
    | awk '{print $3, "(" $4 " bytes)"}' \
    || echo "No cached binaries found"

  echo ""
  echo "=== Cache Statistics ==="
  cache_size=$(aws s3 ls "s3://${S3_BUCKET}/binaries/" --recursive --region "$S3_REGION" \
    | awk '{sum+=$3} END {print sum/1024/1024 " MB"}' || echo "unknown")
  echo "Total cache size: $cache_size"

  # Estimate savings
  echo ""
  echo "=== Build Time Savings Estimate ==="
  echo "- Without cache: ~300 sec (cargo build --release)"
  echo "- With cache hit: ~5 sec (S3 download)"
  echo "- Savings: ~295 sec per deployment"
  echo "- Break-even: ~1 deployment per day"
}

# Clean up old entries from S3
clean_old_entries() {
  echo "Cleaning up old cache entries (keeping last $MAX_CACHED_BINARIES)..."

  if ! command -v aws &>/dev/null; then
    echo "⚠️  AWS CLI not found"
    return 1
  fi

  # List all entries sorted by date, keep only the newest $MAX_CACHED_BINARIES
  aws s3 ls "s3://${S3_BUCKET}/binaries/" --recursive --region "$S3_REGION" \
    | awk '{print $3}' \
    | sort -r \
    | tail -n +$((MAX_CACHED_BINARIES + 1)) \
    | while read -r key; do
      echo "Deleting: $key"
      aws s3 rm "s3://${S3_BUCKET}/$key" --region "$S3_REGION" || true
    done

  echo "✅ Cleanup complete"
}

# Clear all cache entries
clear_cache() {
  if ! command -v aws &>/dev/null; then
    echo "⚠️  AWS CLI not found"
    return 1
  fi

  read -p "Clear ALL build cache entries? (yes/no): " confirm
  if [[ "$confirm" != "yes" ]]; then
    echo "Cancelled."
    return 0
  fi

  echo "Clearing S3 cache..."
  aws s3 rm "s3://${S3_BUCKET}/binaries/" --recursive --region "$S3_REGION" || true

  echo "Clearing local cache..."
  rm -rf "$CACHE_DIR"

  echo "✅ Cache cleared"
}

# Main entry point
main() {
  local cmd="${1:-status}"

  case "$cmd" in
    status)
      show_status
      ;;
    get)
      get_from_cache
      ;;
    put)
      put_to_cache "${2:-.}"
      ;;
    clean)
      clean_old_entries
      ;;
    clear)
      clear_cache
      ;;
    help|--help|-h)
      cat <<EOF
Usage: $0 [COMMAND]

Commands:
  status    Show cache status and inventory (default)
  get       Download cached binary from S3
  put PATH  Upload binary to S3 cache
  clean     Remove old cache entries (keep last $MAX_CACHED_BINARIES)
  clear     Clear all cache entries (confirmation required)
  help      Show this help message

Examples:
  $0 status                    # Check if current build is cached
  $0 get                       # Download cached binary if available
  $0 put target/release/cfs    # Upload newly built binary
  $0 clean                     # Clean up old entries

Environment Variables:
  S3_BUCKET          S3 bucket name (default: claudefs-build-cache)
  S3_REGION          AWS region (default: us-west-2)
  CACHE_DIR          Local cache directory (default: \$HOME/.cfs-build-cache)

Cache Key Format: {git-sha}-{cargo-hash}
EOF
      ;;
    *)
      echo "Unknown command: $cmd"
      echo "Run '$0 help' for usage information"
      exit 1
      ;;
  esac
}

main "$@"
