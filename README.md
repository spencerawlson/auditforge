# AuditForge

**AI-Assisted Security, Compliance & Operations Reporting**

A desktop application for people who have to write security, compliance and operations
reports: penetration testers, cloud engineers, auditors, consultants, incident responders
and operations teams.

You answer guided questions, record findings and attach evidence. AuditForge produces the
document: section order, numbering, typography, tables, table of contents, headers,
footers, page numbers and the evidence appendix. You never lay anything out.

Everything is stored locally. AI assistance is optional, off by default, and constrained:
it helps with language, and it is not allowed to invent facts.

---

## Contents

- [What it does](#what-it-does)
- [Prerequisites](#prerequisites)
- [Install on Ubuntu](#install-on-ubuntu)
- [Install on Kali Linux](#install-on-kali-linux)
- [Install on Windows](#install-on-windows)
- [First run](#first-run)
- [AI configuration](#ai-configuration)
- [Where your data lives](#where-your-data-lives)
- [Development setup](#development-setup)
- [Commands](#commands)
- [Building packages](#building-packages)
- [Documentation](#documentation)
- [Project status](#project-status)
- [Licence](#licence)

---

## What it does

**Engagements** group a piece of work: client, scope, out-of-scope, authorization, rules of
engagement, team and dates.

**Reports** come in eleven types — penetration test, vulnerability assessment, security
assessment, network assessment, incident report, postmortem, cloud security assessment,
compliance/audit, engagement report, executive summary and a custom type. Each type is a
template that decides which sections exist and what you are asked. You answer; the renderer
formats.

**Findings** are recorded once and included in any number of reports. Severity is never
assigned automatically or by an AI suggestion: setting it is a separate, explicitly
confirmed action, and a report cannot be finalized while it includes a finding whose
severity nobody has confirmed.

**Evidence** is copied into a managed vault, hashed with SHA-256, stored owner-only, never
executed and never opened by the application. Files can be encrypted at rest with
AES-256-GCM. The report appendix cites each item with its digest so a reader can verify the
copy they hold.

**Incidents and postmortems** use the same guided approach — the postmortem asks a fixed
set of fifteen questions and will not let a corrective action be marked verified until
somebody records how it was verified.

**Versions** are immutable once finalized. Editing a finalized report is not possible; you
create a revision, and the issued document stays exactly as it was. This is enforced in the
application *and* by database triggers.

**Export** to PDF, HTML, Markdown, JSON, and findings to CSV.

---

## Prerequisites

### To run the application

Ubuntu 22.04+ or Kali Linux (rolling), with a desktop session.

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0
```

The `.deb` declares these, so `apt install ./auditforge_0.1.0_amd64.deb` pulls them in for
you. The AppImage needs them present already.

Optional, for storing the OpenAI API key in your desktop keychain rather than an encrypted
local file:

```bash
sudo apt install -y gnome-keyring    # or: kwalletmanager on KDE
```

Optional, for local AI: [Ollama](https://ollama.com).

### To build from source

- Rust 1.77 or newer (via [rustup](https://rustup.rs))
- Node.js 20 or newer, with npm
- The Tauri build dependencies:

```bash
sudo apt install -y \
  build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev librsvg2-dev libssl-dev \
  libayatana-appindicator3-dev
```

---

## Install on Ubuntu

**From the `.deb`** (recommended):

```bash
sudo apt install ./auditforge_0.1.0_amd64.deb
auditforge
```

It appears in your applications menu as *AuditForge*.

**From the AppImage** (no root required):

```bash
chmod +x AuditForge_0.1.0_amd64.AppImage
./AuditForge_0.1.0_amd64.AppImage
```

**Removing it:**

```bash
sudo apt remove auditforge          # the .deb
rm AuditForge_0.1.0_amd64.AppImage  # the AppImage
```

Neither removes your data. To delete that as well, see
[Where your data lives](#where-your-data-lives).

## Install on Kali Linux

Identical to Ubuntu — Kali is Debian-based and the same package works:

```bash
sudo apt update
sudo apt install ./auditforge_0.1.0_amd64.deb
```

Two things worth knowing on Kali:

- A default Kali install may have no keychain daemon running. AuditForge detects this and
  falls back to an encrypted local file for secrets, telling you so in Settings. Install
  `gnome-keyring` if you would rather use the desktop keychain.
- Running as `root` works, but your data then lives under `/root/.local/share/auditforge`
  rather than your user's directory.

---

## Install on Windows

Linux is the primary target and the one the packaging is built around, but the application
is portable and Windows works. Windows 10 1803+ or Windows 11, with the Edge WebView2
runtime (present by default on Windows 11).

```powershell
# Build the installer
npm ci
npx tauri build --bundles nsis

# Then run it
.\src-tauri\target\release\bundle\nsis\AuditForge_0.1.0_x64-setup.exe
```

Or use the standalone executable at `src-tauri\target\release\auditforge.exe`, which needs
no installation.

Data lives in `%APPDATA%\AuditForge`. The API key goes into Windows Credential Manager.

Uninstall through **Settings → Apps**, or delete the executable. Neither removes
`%APPDATA%\AuditForge`.

Two differences from Linux worth knowing:

- The `.deb` and AppImage targets in `tauri.conf.json` are Linux-only, which is why the
  Windows build passes `--bundles nsis` explicitly.
- File permissions are inherited from the per-user AppData ACL rather than set with mode
  bits; the evidence vault is owner-only either way, but by a different mechanism.

## First run

```
Install  →  Create account  →  Sign in  →  Dashboard
```

The account you create becomes the default author on every report: full name,
organization, and job title or credentials. These are printed on the cover page.

Change them later in **Settings → Profile**. Reports you have already finalized keep the
details they were finalized with — a promotion this year cannot rewrite a report issued
last year.

The password is hashed with Argon2id and a random salt. There is no recovery: if you lose
it, the data is not recoverable, which is the point.

---

## AI configuration

AI is **disabled** on a fresh install, and AuditForge is fully usable that way. Everything
except the assistant works identically with it off.

Three modes, chosen in **Settings → AI**:

| Mode | What happens |
|---|---|
| **Disabled** | No AI features. No network calls. |
| **Local AI** | Requests go to Ollama or a compatible endpoint. Text does not leave the machine. |
| **OpenAI** | Requests go to the OpenAI API. **Report text is sent to a third party.** |

The current mode is shown in the top bar of every screen and beside every AI action.

### Local AI (recommended)

```bash
curl -fsSL https://ollama.com/install.sh | sh
ollama pull llama3.1:8b
ollama serve
```

Then in Settings → AI: choose **Local AI**, endpoint `http://127.0.0.1:11434`, model
`llama3.1:8b`.

If you point the "local" endpoint at another host, AuditForge says so explicitly rather
than continuing to describe it as private.

### OpenAI

Two independent things are required, and selecting the provider is not by itself consent:

1. Store an API key (Settings → AI → API key). It goes into your OS keychain, never into
   the database, and is never displayed again.
2. Tick **"I consent to sending report text to the configured cloud provider."**

Without both, cloud calls are refused with an explanation.

The API key can also be supplied out-of-band for automation:

| Variable | Purpose |
|---|---|
| `AUDITFORGE_LOG` | Log filter, `RUST_LOG` syntax. Example: `AUDITFORGE_LOG=auditforge=debug` |
| `XDG_DATA_HOME` | Overrides the data directory root on Linux |

There is deliberately **no** `OPENAI_API_KEY` environment variable: a key read from the
environment would be invisible to the user and could be picked up from a shell profile they
had forgotten about. The key is stored explicitly, through the UI, in the keychain.

### What the assistant will and will not do

It improves grammar, clarity, structure and tone; rewrites for technical or executive
audiences; summarises; and expands notes into a section.

It does **not** invent IP addresses, hostnames, CVEs, CVSS scores, timestamps, affected
systems, root causes, remediation steps or severities. Suggestions that introduce
identifiers not present in your text are **refused before you see them** — see
[docs/security.md](docs/security.md#the-fabrication-guard).

Every suggestion is shown as *original*, *suggestion* and *changes*, with Accept, Edit and
Reject. Nothing is written until you decide, and what you decided is recorded in the audit
trail.

---

## Where your data lives

Linux:

```
~/.local/share/auditforge/
├── auditforge.db      # SQLite: accounts, engagements, reports, versions, audit log
├── evidence/          # the evidence vault (0700, files 0600)
├── templates/         # drop custom report templates here
├── exports/           # generated documents
├── secrets/           # only when no OS keychain is available
└── logs/
```

Set `XDG_DATA_HOME` to move it. On Windows it is `%APPDATA%\AuditForge`, on macOS
`~/Library/Application Support/AuditForge`.

**Backing up:** Settings → Security → *Back up database* writes a consistent copy while the
application is running. Copy the `evidence/` directory separately; the database references
evidence but does not contain it.

---

## Development setup

```bash
git clone <repository-url> auditforge
cd auditforge

npm install                    # frontend dependencies
cargo fetch                    # Rust dependencies

npm run app:dev                # runs Vite and the Tauri shell together
```

The database is created and migrated automatically on first launch. There is no separate
initialization step and no server to run.

To work on the core without launching the window:

```bash
cargo test -p auditforge-core
```

### Layout

```
auditforge/
├── crates/auditforge-core/   # all domain logic: storage, auth, reports, AI, PDF
│   └── src/{auth,database,reports,engagements,findings,evidence,
│            postmortems,ai,pdf,security,settings,...}
├── src-tauri/                # the desktop shell: commands, window, session token
├── src/                      # React + TypeScript frontend
├── templates/                # the eleven built-in report templates (JSON)
├── migrations/               # SQL schema migrations
├── tests/                    # end-to-end workflow and security test suites
├── docs/
└── scripts/
```

The domain lives in `auditforge-core`, not in the Tauri crate. That keeps the rules
testable without a window, and leaves a command-line front end possible later without
moving any logic. See [docs/architecture.md](docs/architecture.md).

---

## Commands

| Command | What it does |
|---|---|
| `npm run app:dev` | Run the application in development |
| `npm run dev` | Frontend only, in a browser at `localhost:5173` |
| `npm run build` | Typecheck and build the frontend bundle |
| `npm run typecheck` | TypeScript only |
| `npm run lint` | ESLint |
| `npm test` | Frontend tests (Vitest) |
| `cargo test` | All Rust tests: unit, security and end-to-end |
| `cargo test -p auditforge-core --lib` | Core unit tests only |
| `cargo test --test workflow` | The end-to-end walkthrough |
| `cargo test --test security` | The security regression suite |
| `cargo clippy --workspace --all-targets` | Rust lints |
| `cargo fmt --all` | Format Rust |
| `npm run app:build` | Build the release application and packages |
| `npm run app:deb` | Build the `.deb` only |
| `npm run app:appimage` | Build the AppImage only |
| `./scripts/build-linux.sh` | Both packages, with dependency checks |

---

## Building packages

On Ubuntu or Kali, with the build dependencies installed:

```bash
./scripts/build-linux.sh
```

Or directly:

```bash
npm ci
npm run app:build
```

Output:

```
src-tauri/target/release/bundle/deb/auditforge_0.1.0_amd64.deb
src-tauri/target/release/bundle/appimage/AuditForge_0.1.0_amd64.AppImage
```

Packages must be built on Linux; cross-compiling a Tauri application from another platform
is not supported. `scripts/build-linux.sh` checks for the required libraries first and
tells you exactly what to install if any are missing.

---

## Documentation

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | How the pieces fit, and why each significant decision was made |
| [docs/security.md](docs/security.md) | Threat model, controls, the AI safety model, and what is *not* protected |
| [docs/user-guide.md](docs/user-guide.md) | Writing your first report, start to finish |
| [docs/templates.md](docs/templates.md) | Writing a custom report template |
| [docs/development.md](docs/development.md) | Working on the code: tests, layout, adding features |

---

## Project status

Version 0.1.0. The MVP workflow is complete and covered end to end by automated tests:
create an account, sign in, run an engagement, write a report through the guided questions,
record findings and evidence, use and review AI assistance, run a quality review, finalize,
generate a PDF, reopen it later, browse version history, search, and sign out.

Known limitations, stated rather than glossed over:

- **DOCX export is not implemented.** PDF, HTML, Markdown, JSON and CSV are. A `.docx` needs
  a real OOXML package, and shipping something that only looks like Word output would be
  worse than not offering it.
- **PDF text is WinAnsi (Latin-1).** The renderer uses the PDF base-14 fonts, which keeps
  documents small and portable. Characters outside that set render as `?`. Full Unicode
  needs an embedded TrueType font, which is a planned change.
- **Redaction covers structured values only** — IP addresses, emails, cloud access key ids,
  bearer tokens, credential assignments and PEM blocks. Not hostnames, project names or
  people's names, because no mechanical test separates those from ordinary prose. Privacy
  mode, not redaction, is the real control.
- **Integrations are interfaces, not implementations.** DJINN, Nmap, SIEM, ticketing and
  cloud connectors are deliberately out of scope for the MVP; the provider abstraction and
  the core's separation from the shell are what make them straightforward later.
- **Multi-user is designed for but not exposed.** The schema and every write already carry
  an owning user and an actor; the UI signs in one person at a time.

---

## Licence

Apache-2.0. See [LICENSE](LICENSE).
