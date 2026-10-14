#!/usr/bin/env bash
# Native call-screen design preview. Microphone off until explicitly started.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/home/abe/build-cache/p2p_chat/group-audio-capacity/target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
python3 /home/abe/p2p_chat/scripts/buildlock.py --who iced-cloud-live -- \
    cargo build --release -p desktop --example audio_cloud_demo
exec "$CARGO_TARGET_DIR/release/examples/audio_cloud_demo" "$@"
