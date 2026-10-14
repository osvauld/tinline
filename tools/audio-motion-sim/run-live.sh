#!/usr/bin/env bash
# Opt-in viewer; microphone starts only by button. No audio saved/sent or app installation.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/home/abe/build-cache/p2p_chat/group-audio-capacity/target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
python3 /home/abe/p2p_chat/scripts/buildlock.py --who audio-motion-live -- \
    cargo build --release -p desktop --example audio_motion_demo
exec "$CARGO_TARGET_DIR/release/examples/audio_motion_demo" "$@"
