#!/usr/bin/env bash
set -euo pipefail
: "${R2_BUCKET:?set R2_BUCKET}"
: "${R2_ENDPOINT:?set R2_ENDPOINT}"
REPO_DIR="${REPO_DIR:-dist/repo}"
command -v aws >/dev/null || { echo 'missing AWS CLI' >&2; exit 1; }
test -s "$REPO_DIR/apt/dists/stable/InRelease"
test -s "$REPO_DIR/tinline.gpg"
DEST="s3://${R2_BUCKET}"
# Never delete older packages or indices. Upload referenced objects first.
aws s3 sync "$REPO_DIR/" "$DEST/" --endpoint-url "$R2_ENDPOINT" \
  --exclude 'apt/dists/*' --cache-control 'public,max-age=300'
aws s3 sync "$REPO_DIR/apt/dists/" "$DEST/apt/dists/" --endpoint-url "$R2_ENDPOINT" \
  --exclude '*' --include '*/by-hash/*' --cache-control 'public,max-age=31536000,immutable'
aws s3 sync "$REPO_DIR/apt/dists/" "$DEST/apt/dists/" --endpoint-url "$R2_ENDPOINT" \
  --exclude '*/by-hash/*' --exclude '*/Release' --exclude '*/Release.gpg' --exclude '*/InRelease' \
  --cache-control 'no-cache,max-age=0,must-revalidate'
for name in Release Release.gpg InRelease; do
  aws s3 cp "$REPO_DIR/apt/dists/stable/$name" "$DEST/apt/dists/stable/$name" \
    --endpoint-url "$R2_ENDPOINT" --cache-control 'no-cache,max-age=0,must-revalidate'
done
