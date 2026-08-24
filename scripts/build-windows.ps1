# Build the AuditForge Windows installer.
#
# Linux is the primary target; this exists because the application is portable and Windows
# is a reasonable place to run it.
#
# One wrinkle, handled here. With the MSVC toolchain, `WebView2Loader` is linked statically
# and the executable is self-contained. With the GNU toolchain it becomes a DLL dependency,
# emitted next to the binary by a build script - which is why the app runs from
# `target\release` but a copy of the executable on its own does not. The DLL is copied into
# `src-tauri\` and declared as a bundle resource in tauri.windows.conf.json so the installer
# ships it. Symptom if this is skipped: the installed application exits immediately with
# 0xC0000135, STATUS_DLL_NOT_FOUND, and no window ever appears.

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Say($text) { Write-Host "==> $text" -ForegroundColor Cyan }

foreach ($tool in @("cargo", "npm")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool is not on PATH. Rust (https://rustup.rs) and Node 20+ are required."
    }
}

$host_triple = (rustc -vV | Select-String "^host:").ToString().Split(" ")[1]
Say "toolchain: $host_triple"

Say "installing frontend dependencies"
if (Test-Path "package-lock.json") { npm ci --no-audit --no-fund } else { npm install --no-audit --no-fund }

Say "checking the frontend"
npm run typecheck
npm run lint

Say "running the Rust test suite"
cargo test --workspace --quiet

# Build first so the loader DLL exists before the bundler looks for the resource.
Say "building the release binary"
cargo build --release -p auditforge

$loader = "target\release\WebView2Loader.dll"
if (Test-Path $loader) {
    Copy-Item $loader "src-tauri\WebView2Loader.dll" -Force
    Say "staged WebView2Loader.dll for the installer"
} elseif ($host_triple -like "*windows-gnu*") {
    throw "WebView2Loader.dll was not produced at $loader; the installer would build but the installed application would not start."
} else {
    Say "no loader DLL to stage (MSVC links it statically)"
}

Say "building the installer"
npx tauri build --bundles nsis --config src-tauri/tauri.windows.conf.json

$setup = Get-ChildItem "target\release\bundle\nsis\*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $setup) { throw "the build finished but produced no installer" }

Write-Host ""
Say "installer: $($setup.FullName)  ($([math]::Round($setup.Length / 1MB, 1)) MB)"
Write-Host @"

Install it with:

    & "$($setup.FullName)"

or silently, for the current user:

    Start-Process "$($setup.FullName)" -ArgumentList "/S" -Wait

Data is stored in %APPDATA%\AuditForge. The API key goes into Windows Credential Manager.
"@
