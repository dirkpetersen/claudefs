#!/bin/bash
# Verify ClaudeFS release artifacts by checking GPG signatures and checksums
# Usage: ./tools/verify-release.sh [ARTIFACT_DIR]
#
# Verifies GPG signatures on all .asc files and validates SHA256 checksums

set -e

ARTIFACT_DIR="${1:-./releases}"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

if [ ! -d "$ARTIFACT_DIR" ]; then
    echo "Error: Artifact directory not found: $ARTIFACT_DIR"
    exit 1
fi

echo "[$(date)] Verifying release artifacts from $ARTIFACT_DIR..."

# Check for GPG signatures
SIG_FILES=$(ls "$ARTIFACT_DIR"/*.asc 2>/dev/null | wc -l)
if [ $SIG_FILES -eq 0 ]; then
    echo "Warning: No signature files found in $ARTIFACT_DIR"
    echo "Proceeding with SHA256 verification only..."
fi

# Verify each GPG signature
VERIFIED_COUNT=0
FAILED_COUNT=0

if [ $SIG_FILES -gt 0 ]; then
    echo "[$(date)] Verifying GPG signatures..."

    for SIG_FILE in "$ARTIFACT_DIR"/*.asc; do
        if [ ! -f "$SIG_FILE" ]; then
            continue
        fi

        BINARY_FILE="${SIG_FILE%.asc}"
        BASENAME=$(basename "$BINARY_FILE")

        if [ ! -f "$BINARY_FILE" ]; then
            echo "Warning: Binary file not found for signature: $SIG_FILE"
            continue
        fi

        echo -n "  Verifying $BASENAME... "

        if gpg --verify "$SIG_FILE" "$BINARY_FILE" 2>/dev/null; then
            echo "✓"
            VERIFIED_COUNT=$((VERIFIED_COUNT + 1))
        else
            echo "✗ FAILED"
            FAILED_COUNT=$((FAILED_COUNT + 1))
        fi
    done
fi

# Verify SHA256 checksums
echo "[$(date)] Verifying SHA256 checksums..."

CHECKSUM_VERIFIED=0
CHECKSUM_FAILED=0

for SHA256_FILE in "$ARTIFACT_DIR"/*.sha256; do
    if [ ! -f "$SHA256_FILE" ]; then
        continue
    fi

    BASENAME=$(basename "$SHA256_FILE")
    TARBALL_FILE="${SHA256_FILE%.sha256}"

    if [ ! -f "$TARBALL_FILE" ]; then
        echo "Warning: Tarball file not found for checksum: $SHA256_FILE"
        continue
    fi

    echo -n "  Verifying $(basename "$TARBALL_FILE")... "

    if sha256sum -c "$SHA256_FILE" >/dev/null 2>&1; then
        echo "✓"
        CHECKSUM_VERIFIED=$((CHECKSUM_VERIFIED + 1))
    else
        echo "✗ FAILED"
        CHECKSUM_FAILED=$((CHECKSUM_FAILED + 1))
    fi
done

# Check manifest if present
MANIFEST_COUNT=$(ls "$ARTIFACT_DIR"/MANIFEST-*.json 2>/dev/null | wc -l)
if [ $MANIFEST_COUNT -gt 0 ]; then
    echo "[$(date)] Verifying manifest..."

    for MANIFEST in "$ARTIFACT_DIR"/MANIFEST-*.json; do
        echo "  Manifest: $(basename "$MANIFEST")"

        if command -v jq &> /dev/null; then
            echo "    Version: $(jq -r '.version' "$MANIFEST" 2>/dev/null || echo 'unknown')"
            echo "    Timestamp: $(jq -r '.timestamp' "$MANIFEST" 2>/dev/null || echo 'unknown')"
            echo "    Signing Key: $(jq -r '.signing_key_id' "$MANIFEST" 2>/dev/null || echo 'unknown')"
        fi
    done
fi

# Summary
echo ""
echo "Verification Summary:"
if [ $SIG_FILES -gt 0 ]; then
    echo "  GPG Signatures: $VERIFIED_COUNT verified, $FAILED_COUNT failed"
fi
echo "  SHA256 Checksums: $CHECKSUM_VERIFIED verified, $CHECKSUM_FAILED failed"

if [ $FAILED_COUNT -gt 0 ] || [ $CHECKSUM_FAILED -gt 0 ]; then
    echo ""
    echo "✗ Verification FAILED"
    exit 1
else
    echo ""
    echo "✓ All artifacts verified successfully"
    exit 0
fi
