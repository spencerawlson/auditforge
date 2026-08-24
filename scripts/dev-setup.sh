#!/usr/bin/env bash
#
# One-time development setup for Ubuntu or Kali: system libraries, Rust, Node dependencies.
# Safe to re-run.

set -euo pipefail
cd "$(dirname "$0")/.."

BOLD=$'\033[1m'; OFF=$'\033[0m'
say() { printf '%s==>%s %s\n' "$BOLD" "$OFF" "$*"; }

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This script targets Ubuntu and Kali. On another platform, see the README." >&2
  exit 1
fi

say "installing Tauri build dependencies (sudo required)"
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev librsvg2-dev libssl-dev \
  libayatana-appindicator3-dev \
  gnome-keyring

if ! command -v cargo >/dev/null; then
  say "installing Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

if ! command -v node >/dev/null; then
  cat <<'MSG'
Node.js is not installed. Install version 20 or newer, for example:

  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo apt install -y nodejs

Then re-run this script.
MSG
  exit 1
fi

say "installing frontend dependencies"
npm install --no-audit --no-fund

say "fetching Rust dependencies"
cargo fetch

cat <<'MSG'

Ready. Useful commands:

  npm run app:dev            run AuditForge in development
  cargo test                 the Rust test suite
  npm test                   the frontend tests
  ./scripts/build-linux.sh   build the .deb and AppImage

MSG
