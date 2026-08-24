#!/usr/bin/env bash
#
# Build the AuditForge .deb and AppImage on Ubuntu or Kali.
#
# Checks the toolchain and the Tauri build dependencies first and tells you exactly what to
# install if anything is missing, rather than letting the build fail two minutes in with a
# linker error.

set -euo pipefail

cd "$(dirname "$0")/.."

BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; OFF=$'\033[0m'

say()  { printf '%s==>%s %s\n' "$BOLD" "$OFF" "$*"; }
warn() { printf '%s warn%s %s\n' "$YELLOW" "$OFF" "$*"; }
die()  { printf '%s error%s %s\n' "$RED" "$OFF" "$*" >&2; exit 1; }
ok()   { printf '%s  ok%s %s\n' "$GREEN" "$OFF" "$*"; }

# ---------------------------------------------------------------- platform

if [[ "$(uname -s)" != "Linux" ]]; then
  die "Linux packages must be built on Linux. Cross-compiling a Tauri application is not supported.
     Build inside a VM or container running Ubuntu 22.04+ or Kali."
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
  warn "building on $(uname -m); the package name will reflect this architecture"
fi

# ---------------------------------------------------------------- toolchain

command -v cargo >/dev/null || die "Rust is not installed. Install it from https://rustup.rs"
command -v npm   >/dev/null || die "Node.js and npm are not installed. Node 20 or newer is required."

rust_version="$(rustc --version | awk '{print $2}')"
node_version="$(node --version)"
ok "rustc $rust_version, node $node_version"

# ---------------------------------------------------------------- system libraries

missing=()
need_pkgconfig() {
  pkg-config --exists "$1" 2>/dev/null || missing+=("$2")
}

if ! command -v pkg-config >/dev/null; then
  missing+=("pkg-config")
else
  need_pkgconfig webkit2gtk-4.1 libwebkit2gtk-4.1-dev
  need_pkgconfig gtk+-3.0       libgtk-3-dev
  need_pkgconfig librsvg-2.0    librsvg2-dev
  need_pkgconfig openssl        libssl-dev
fi

command -v cc >/dev/null || missing+=("build-essential")

if (( ${#missing[@]} > 0 )); then
  die "missing build dependencies: ${missing[*]}

     Install them with:

       sudo apt update
       sudo apt install -y build-essential curl wget file pkg-config \\
         libwebkit2gtk-4.1-dev librsvg2-dev libssl-dev libayatana-appindicator3-dev"
fi
ok "system libraries present"

# ---------------------------------------------------------------- build

say "installing frontend dependencies"
if [[ -f package-lock.json ]]; then
  npm ci --no-audit --no-fund
else
  npm install --no-audit --no-fund
fi

say "checking the frontend"
npm run typecheck
npm run lint

say "running the Rust test suite"
cargo test --workspace --quiet

say "building the application and packages (this takes a few minutes)"
npm run app:build

# ---------------------------------------------------------------- report

bundle="src-tauri/target/release/bundle"
say "artifacts"
found=0
for artifact in "$bundle"/deb/*.deb "$bundle"/appimage/*.AppImage; do
  if [[ -f "$artifact" ]]; then
    printf '     %s  (%s)\n' "$artifact" "$(du -h "$artifact" | cut -f1)"
    found=1
  fi
done

(( found )) || die "the build finished but produced no packages; check the output above"

cat <<'NOTE'

Install the .deb:

  sudo apt install ./src-tauri/target/release/bundle/deb/auditforge_*.deb

Or run the AppImage directly:

  chmod +x src-tauri/target/release/bundle/appimage/AuditForge_*.AppImage
  ./src-tauri/target/release/bundle/appimage/AuditForge_*.AppImage

NOTE
ok "done"
