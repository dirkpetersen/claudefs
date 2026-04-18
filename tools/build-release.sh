#!/bin/bash
# Build production release binaries for ClaudeFS
# Usage: ./tools/build-release.sh [VERSION] [--aarch64]
#
# Creates optimized, stripped binaries with LTO for multiple architectures
# Output: releases/cfs-{server,client,mgmt}-v{VERSION}-{arch}.tar.gz{,.sha256}

set -e

VERSION="${1:-0.0.1}"
BUILD_LTO="${BUILD_LTO:-true}"
BUILD_DIR="$(pwd)/target/release"
ARTIFACT_DIR="$(pwd)/releases"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Create artifacts directory
mkdir -p "$ARTIFACT_DIR"

echo "[$(date)] Building ClaudeFS v${VERSION}..."

# Clean build artifacts
echo "[$(date)] Cleaning previous builds..."
cargo clean

# Build release binaries with LTO optimization
echo "[$(date)] Building x86_64 release binaries (LTO enabled)..."
if [ "$BUILD_LTO" = "true" ]; then
    RUSTFLAGS="-C lto=fat -C embed-bitcode=yes" cargo build --release --locked
else
    cargo build --release --locked
fi

# Extract binary targets from Cargo workspace
# The main binary is 'cfs' which should be the entry point
# For this release, we'll package the primary binaries
BINARIES=(
    "$BUILD_DIR/cfs"
)

# If binaries don't exist with those names, check common patterns
if [ ! -f "$BUILD_DIR/cfs" ] && [ -f "$BUILD_DIR/claudefs" ]; then
    BINARIES=("$BUILD_DIR/claudefs")
fi

echo "[$(date)] Packaging binaries..."

# For each binary, create tarball and checksum
for BINARY in "${BINARIES[@]}"; do
    if [ ! -f "$BINARY" ]; then
        echo "Warning: Binary $BINARY not found, skipping"
        continue
    fi

    NAME=$(basename "$BINARY")
    STRIPPED_NAME="${NAME}.stripped"

    echo "[$(date)] Processing $NAME..."

    # Strip debug symbols
    strip "$BINARY" -o "$BUILD_DIR/$STRIPPED_NAME" 2>/dev/null || {
        # If strip fails, just copy the binary
        cp "$BINARY" "$BUILD_DIR/$STRIPPED_NAME"
    }

    # Get size before/after for logging
    BEFORE=$(du -h "$BINARY" | cut -f1)
    AFTER=$(du -h "$BUILD_DIR/$STRIPPED_NAME" | cut -f1)
    echo "[$(date)] $NAME: $BEFORE → $AFTER (stripped)"

    # Create tarball with compressed binary
    TARBALL="$ARTIFACT_DIR/${NAME}-v${VERSION}-x86_64.tar.gz"
    tar -czf "$TARBALL" \
        -C "$BUILD_DIR" "$STRIPPED_NAME" \
        --transform "s/${STRIPPED_NAME}/${NAME}/"

    # Generate SHA256 checksum
    SHA256_FILE="${TARBALL}.sha256"
    sha256sum "$TARBALL" > "$SHA256_FILE"
    echo "[$(date)] Created $TARBALL"
    echo "        SHA256: $(cut -d' ' -f1 "$SHA256_FILE")"
done

# Optional: aarch64 cross-compilation
if [[ " $@ " =~ " --aarch64 " ]]; then
    echo "[$(date)] Building aarch64 release binaries..."

    # Check if aarch64 target is installed
    if ! rustup target list | grep -q "aarch64-unknown-linux-gnu (installed)"; then
        echo "[$(date)] Installing aarch64-unknown-linux-gnu target..."
        rustup target add aarch64-unknown-linux-gnu
    fi

    if [ "$BUILD_LTO" = "true" ]; then
        RUSTFLAGS="-C lto=fat -C embed-bitcode=yes" cargo build --release --locked --target aarch64-unknown-linux-gnu
    else
        cargo build --release --locked --target aarch64-unknown-linux-gnu
    fi

    BUILD_DIR_AARCH64="$(pwd)/target/aarch64-unknown-linux-gnu/release"

    for BINARY in "${BINARIES[@]}"; do
        NAME=$(basename "$BINARY")
        AARCH64_BINARY="$BUILD_DIR_AARCH64/$NAME"

        if [ ! -f "$AARCH64_BINARY" ]; then
            echo "Warning: aarch64 binary $AARCH64_BINARY not found, skipping"
            continue
        fi

        STRIPPED_NAME="${NAME}.stripped"
        echo "[$(date)] Processing aarch64 $NAME..."

        # Strip debug symbols
        aarch64-linux-gnu-strip "$AARCH64_BINARY" -o "$BUILD_DIR_AARCH64/$STRIPPED_NAME" 2>/dev/null || {
            cp "$AARCH64_BINARY" "$BUILD_DIR_AARCH64/$STRIPPED_NAME"
        }

        BEFORE=$(du -h "$AARCH64_BINARY" | cut -f1)
        AFTER=$(du -h "$BUILD_DIR_AARCH64/$STRIPPED_NAME" | cut -f1)
        echo "[$(date)] $NAME (aarch64): $BEFORE → $AFTER (stripped)"

        # Create tarball
        TARBALL="$ARTIFACT_DIR/${NAME}-v${VERSION}-aarch64.tar.gz"
        tar -czf "$TARBALL" \
            -C "$BUILD_DIR_AARCH64" "$STRIPPED_NAME" \
            --transform "s/${STRIPPED_NAME}/${NAME}/"

        # Generate SHA256 checksum
        SHA256_FILE="${TARBALL}.sha256"
        sha256sum "$TARBALL" > "$SHA256_FILE"
        echo "[$(date)] Created $TARBALL"
        echo "        SHA256: $(cut -d' ' -f1 "$SHA256_FILE")"
    done
fi

# Generate build manifest
MANIFEST="$ARTIFACT_DIR/BUILD-MANIFEST-v${VERSION}.json"
cat > "$MANIFEST" << EOF
{
  "version": "v${VERSION}",
  "build_timestamp": "$(date -u -Iseconds)",
  "git_commit": "$(git rev-parse HEAD)",
  "git_branch": "$(git rev-parse --abbrev-ref HEAD)",
  "build_host": "$(hostname)",
  "build_profile": "release-lto",
  "artifacts": {
EOF

# Add artifact list
FIRST=true
for TARBALL in "$ARTIFACT_DIR"/*.tar.gz; do
    if [ -f "$TARBALL" ]; then
        if [ "$FIRST" = true ]; then
            FIRST=false
        else
            echo "," >> "$MANIFEST"
        fi
        SHA256=$(cut -d' ' -f1 "${TARBALL}.sha256")
        cat >> "$MANIFEST" << EOF
    {
      "path": "$(basename "$TARBALL")",
      "sha256": "$SHA256",
      "size_bytes": $(stat -f%z "$TARBALL" 2>/dev/null || stat -c%s "$TARBALL" 2>/dev/null || echo "0")
    }
EOF
    fi
done

cat >> "$MANIFEST" << EOF
  ]
}
EOF

echo "[$(date)] ✓ Release artifacts ready in $ARTIFACT_DIR"
echo "[$(date)] Manifest: $MANIFEST"
echo ""
echo "Summary:"
ls -lh "$ARTIFACT_DIR"/*.tar.gz 2>/dev/null | awk '{print "  " $9 " (" $5 ")"}'
echo ""
echo "Checksums:"
cat "$ARTIFACT_DIR"/*.sha256 2>/dev/null | awk '{print "  " $2 ": " $1}'
