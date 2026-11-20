#!/bin/bash
# Setup script for the katlendar cloud environment (Claude Code on the web).
# Paste it into the environment's "Setup script" field. It runs as root on
# Ubuntu 24.04, before Claude Code starts.
#
# Environment settings that go with it:
#   Network access:  Limited, with "Allow package managers" on. That covers
#                    crates.io, static.rust-lang.org and the Ubuntu mirrors.
#                    Full is only needed for --check-feed / --check-caldav
#                    against real calendar servers.
#   Network secrets: none.
#   Variables:       LIBGL_ALWAYS_SOFTWARE=1 (optional, for --screenshot).
#
# It does not build the project: a script that finishes in about five minutes
# is snapshotted and reused, and a full build takes longer than that.
#
# Screenshot in a session:
#   xvfb-run -a -s "-screen 0 1920x1080x24" ./target/debug/katlendar --screenshot shot.png
set -euo pipefail

# Rust stable: the egui 0.37 crates need rustc >= 1.95, and the image's Rust
# may be older. rustup-init comes from static.rust-lang.org, which the Limited
# allowlist covers (sh.rustup.rs is not on it).
if ! command -v rustup >/dev/null 2>&1; then
  curl -fsSL -o /tmp/rustup-init \
    https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y --profile minimal --default-toolchain none
  rm /tmp/rustup-init
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain install stable --profile minimal --component clippy,rustfmt
rustup default stable
# Put rustup's cargo/rustc ahead of any apt-installed ones
for b in cargo rustc rustup cargo-clippy clippy-driver rustfmt cargo-fmt; do
  ln -sf "$HOME/.cargo/bin/$b" "/usr/local/bin/$b"
done

# For --screenshot only: a virtual X display, software OpenGL, the X
# libraries winit loads at runtime, and fonts. Not needed to build or test.
apt-get update
apt-get install -y --no-install-recommends \
  xvfb xauth \
  libgl1 libglx-mesa0 libegl1 libegl-mesa0 libgl1-mesa-dri \
  libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxkbcommon-x11-0 \
  fontconfig fonts-dejavu-core fonts-noto-color-emoji || true

cargo --version
