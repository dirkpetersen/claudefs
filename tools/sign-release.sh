#!/bin/bash
# Sign ClaudeFS release artifacts with GPG
# Usage: ./tools/sign-release.sh [ARTIFACT_DIR] [GPG_KEY_ID]
#
# Signs all .tar.gz files in the artifact directory with detached signatures
# Creates .asc files for each binary and a signed manifest

set -e

ARTIFACT_DIR="${1:-./releases}"
GPG_KEY_ID="${2:-${CFS_GPG_KEY_ID:-}}"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

if [ ! -d "$ARTIFACT_DIR" ]; then
    echo "Error: Artifact directory not found: $ARTIFACT_DIR"
    exit 1
fi

echo "[$(date)] Signing release artifacts from $ARTIFACT_DIR..."

# If GPG key ID not provided, try to get it from environment or Secrets Manager
if [ -z "$GPG_KEY_ID" ]; then
    echo "[$(date)] No GPG_KEY_ID provided. Checking AWS Secrets Manager..."

    if command -v aws &> /dev/null; then
        GPG_KEY_ID=$(aws secretsmanager get-secret-value \
            --secret-id cfs/gpg-key-id \
            --region us-west-2 \
            --query SecretString --output text 2>/dev/null || echo "")
    fi

    if [ -z "$GPG_KEY_ID" ]; then
        # Try to get first available signing key
        GPG_KEY_ID=$(gpg --list-secret-keys --keyid-format LONG 2>/dev/null | grep -oP '(?<=sec.*/)[\dA-F]+' | head -1)
    fi

    if [ -z "$GPG_KEY_ID" ]; then
        echo "Error: No GPG key ID found. Please set CFS_GPG_KEY_ID or provide AWS secrets."
        exit 1
    fi
fi

echo "[$(date)] Using GPG key: $GPG_KEY_ID"

# Try to import GPG key from AWS Secrets Manager if available
if command -v aws &> /dev/null; then
    echo "[$(date)] Attempting to import GPG key from AWS Secrets Manager..."
    aws secretsmanager get-secret-value \
        --secret-id cfs/gpg-key \
        --region us-west-2 \
        --query SecretString --output text 2>/dev/null | \
        gpg --import --quiet 2>/dev/null || {
        echo "[$(date)] Note: Could not import GPG key from Secrets Manager, using local key"
    }
fi

# Sign each artifact with detached signature
SIGNED_COUNT=0
for TARBALL in "$ARTIFACT_DIR"/*.tar.gz; do
    if [ ! -f "$TARBALL" ]; then
        continue
    fi

    BASENAME=$(basename "$TARBALL")
    SIG_FILE="${TARBALL}.asc"

    echo "[$(date)] Signing $BASENAME..."

    gpg --local-user "$GPG_KEY_ID" \
        --armor \
        --detach-sign \
        --output "$SIG_FILE" \
        "$TARBALL"

    if [ -f "$SIG_FILE" ]; then
        echo "         → Created $SIG_FILE"
        SIGNED_COUNT=$((SIGNED_COUNT + 1))
    else
        echo "Error: Failed to create signature for $BASENAME"
        exit 1
    fi
done

if [ $SIGNED_COUNT -eq 0 ]; then
    echo "Warning: No artifacts found to sign in $ARTIFACT_DIR"
    exit 1
fi

echo "[$(date)] Signed $SIGNED_COUNT artifacts"

# Generate signed manifest
VERSION=$(ls "$ARTIFACT_DIR"/*.tar.gz 2>/dev/null | head -1 | grep -oP 'v[0-9]+\.[0-9]+\.[0-9]+' | head -1)
if [ -z "$VERSION" ]; then
    VERSION="unknown"
fi

MANIFEST="$ARTIFACT_DIR/MANIFEST-${VERSION}.json"
echo "[$(date)] Generating manifest: $MANIFEST"

cat > "$MANIFEST" << EOF
{
  "version": "$VERSION",
  "timestamp": "$(date -u -Iseconds)",
  "signing_key_id": "$GPG_KEY_ID",
  "binaries": {
EOF

FIRST=true
for TARBALL in "$ARTIFACT_DIR"/*.tar.gz; do
    if [ -f "$TARBALL" ] && [ -f "${TARBALL}.asc" ]; then
        if [ "$FIRST" = true ]; then
            FIRST=false
        else
            echo "," >> "$MANIFEST"
        fi

        BASENAME=$(basename "$TARBALL")
        SHA256=$(cut -d' ' -f1 "${TARBALL}.sha256" 2>/dev/null || sha256sum "$TARBALL" | cut -d' ' -f1)
        SIZE=$(stat -f%z "$TARBALL" 2>/dev/null || stat -c%s "$TARBALL" 2>/dev/null || echo "0")

        cat >> "$MANIFEST" << EOF
    "$BASENAME": {
      "sha256": "$SHA256",
      "size_bytes": $SIZE,
      "signature": "$(basename "${TARBALL}.asc")",
      "signature_url": "https://github.com/dirkpetersen/claudefs/releases/download/${VERSION}/$(basename "${TARBALL}.asc")"
    }
EOF
    fi
done

cat >> "$MANIFEST" << EOF
  },
  "verification_instructions": "gpg --verify <file>.asc <file>.tar.gz"
}
EOF

echo "[$(date)] ✓ Release signing complete"
echo ""
echo "Signed artifacts:"
ls -lh "$ARTIFACT_DIR"/*.asc 2>/dev/null | awk '{print "  " $9}'
echo ""
echo "Manifest:"
echo "  $MANIFEST"
echo ""
echo "Verification command:"
echo "  gpg --verify <file>.asc <file>.tar.gz"
