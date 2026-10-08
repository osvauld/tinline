#!/usr/bin/env bash
set -euo pipefail

: "${R2_BUCKET:?set R2_BUCKET}"
: "${R2_ENDPOINT:?set R2_ENDPOINT, e.g. https://<account-id>.r2.cloudflarestorage.com}"
REPO_DIR="${REPO_DIR:-dist/repo}"

command -v aws >/dev/null || { echo "missing aws CLI" >&2; exit 1; }
aws s3 sync "$REPO_DIR/" "s3://${R2_BUCKET}/" --endpoint-url "$R2_ENDPOINT" --delete
