# Architecture

This document explains how AuditForge is put together and, where a decision was not
obvious, why it was made that way.

## Shape

```
┌──────────────────────────────────────────────────────────┐
│  src/            React + TypeScript                      │
│                  screens, forms, the AI review pane       │
└───────────────────────────┬──────────────────────────────┘
                            │  Tauri commands (typed, one boundary)
┌───────────────────────────▼──────────────────────────────┐
│  src-tauri/      the desktop shell                        │
│                  window, session token, command wiring    │
└───────────────────────────┬──────────────────────────────┘
                            │  plain Rust calls
┌───────────────────────────▼──────────────────────────────┐
│  crates/auditforge-core/   everything the app decides     │
│                                                           │
│  auth  database  engagements  findings  evidence          │
│  incidents  postmortems  reports  templates               │
│  ai  pdf  export  search  security  settings  audit       │
└───────────────────────────┬──────────────────────────────┘
                            │
              SQLite  ·  evidence vault  ·  OS keychain
```

## The core is a library, not the shell

The build specification sketched the domain modules under `src-tauri/src/`. They live in
`crates/auditforge-core/` instead. The module names are unchanged, and `src-tauri/src/commands/`
mirrors them one for one.

Three reasons:

1. **The rules are testable without a window.** 362 tests run in seconds with
   `cargo test`, including the complete end-to-end workflow and the security suite. If the
   domain lived in the Tauri crate, every one of them would need the GUI toolchain.
2. **A command-line front end stays possible.** The specification asks for the architecture
   to remain CLI-compatible. It is: a CLI would be a second consumer of `auditforge_core::App`,
   with no logic to move.
3. **The boundary is enforced by the compiler.** `auditforge-core` does not depend on
   `tauri`. Domain logic cannot accidentally reach for a window handle.

`src-tauri` holds the window, the session token and the command definitions — and nothing
a test would want to assert about.

## The `App` facade

`auditforge_core::App` owns the database, the resolved paths and the secret store, and
provides the operations that span modules: rendering a PDF (report + template + settings +
evidence vault), exporting, and the dashboard.

Simple operations skip it. `reports::save_draft(&db, &actor, ...)` is a free function taking
a `&Database`, because inventing a service object for a single-table write adds indirection
without adding anything else.

## Storage

SQLite through `rusqlite`, bundled so there is no system dependency, behind a single
mutex-guarded connection. A desktop application with one user and short transactions does
not need a pool; a single connection makes "one writer at a time" true by construction.

Migrations are embedded in the binary via `include_str!` and applied at startup. Each
records a SHA-256 of the SQL it ran: if a shipped migration is later edited, startup fails
loudly rather than leaving two installations with silently different schemas.

Conventions: UUID string primary keys, RFC 3339 UTC timestamps as text (they sort
correctly and round-trip through JSON unchanged), and a separate human-facing reference
(`ENG-2026-0007`) so a report can cite an identifier without exposing an internal one.

`Timestamp::now()` truncates to whole seconds, because the storage format does. Without
that, an in-memory value would differ from the same value reloaded, and a report's content
hash would change simply by saving and reopening it.

## Reports and versions

A report has exactly one *current* version. Draft edits are written into it in place, which
is what makes the guided form feel like a document rather than a commit log. History is
created at explicit transitions:

| Action | Result |
|---|---|
| `create` | `v0.1 Draft` |
| `save_draft` | writes into the current version |
| `snapshot` | seals the current version, opens `v0.<n+1>` |
| `submit_for_review` | seals it, opens `v0.<n+1> Review` |
| `finalize` | seals everything, writes `v<major+1>.0 Final`, immutable |
| `create_revision` | opens `v<major>.<n+1> Revised` from the finalized content |

Each version stores the **whole document**: the author identity as it was at the time, the
engagement facts, and copies of every finding and evidence record it cites. That is what
makes the guarantee real — editing a job title, renaming an engagement or deleting a finding
tomorrow cannot alter a report finalized today, because the finalized version does not read
any of those tables.

Immutability is enforced twice: in `reports::ensure_editable`, and by `BEFORE UPDATE` and
`BEFORE DELETE` triggers on `report_versions`. The application check gives a good error
message; the trigger means no future code path — or a stray `sqlite3` session — can quietly
rewrite history. `tests/security.rs` asserts both.

## Templates

A template is data: JSON declaring sections, questions, which are required, and the order.
The eleven built-in templates live in `templates/` and are embedded at build time, so a
packaged application never depends on files being installed next to it. User templates are
loaded from the data directory on top, and a user template is never overwritten by an
update even if it shares a key with a built-in one.

Sections have a `kind`. `guided` sections are answered; `findings` and `evidence` sections
are rendered from the linked records. That is why the editor can show a picker for one and
a question list for the other without special-casing template keys.

`reconcile` brings an existing report up to date with an edited template: new questions
appear, and answers whose question was removed are **kept**. Losing an author's words to a
template edit would be worse than carrying an orphan.

## The AI layer

```
AiProvider (trait)
├── OpenAiProvider     Chat Completions with a strict json_schema response format
├── LocalProvider      Ollama /api/chat, with OpenAI-compatible fallbacks
├── DisabledProvider   refuses with an explanation, the default
└── ScriptedProvider   canned responses, for tests and the offline walkthrough
```

Nothing above the provider knows a vendor name. Every call runs the same pipeline:

1. **Policy.** A cloud call needs a configured provider, a stored key *and* explicit
   consent. Selecting OpenAI is not consent.
2. **Minimum context.** The passage plus a short caller-supplied context line, both capped.
   Never a whole engagement, never the database.
3. **Redaction** (optional, default on). Structured secrets are replaced with placeholders
   and restored afterwards, so the author sees real values in a suggestion the provider
   never saw.
4. **Strict schema.** Model output is untrusted input: parsed against a fixed shape, every
   string length-capped and stripped of control characters, unreadable enums degraded
   rather than propagated, and review quotes that do not appear in the report dropped.
5. **The fabrication guard.** Identifiers in the output that are not in the input mean the
   suggestion is discarded. See [security.md](security.md#the-fabrication-guard).
6. **Record.** The request, and later the accept/edit/reject decision, go into
   `ai_requests` and the audit log.

The layer returns a *suggestion*. Applying it is a separate, human-initiated call into
`reports::save_draft`. There is no code path from a model response to stored content.

### Provenance

Every stored answer carries one of four values:

| Provenance | Meaning |
|---|---|
| `user_provided` | The author wrote it. The only kind that asserts fact. |
| `ai_wording` | A model reworded the author's material. Authoritative. |
| `ai_inference` | Model-drafted prose the author has not confirmed. **Blocks finalization.** |
| `missing_information` | An acknowledged gap. Renders as "Information required." |

Rewording functions propose `ai_wording`; functions that produce new prose (Expand,
Executive Summary, Finding Analysis) propose `ai_inference`. That mapping is what gives the
finalization blocker teeth: assistant-drafted prose cannot reach a signed-off document
without a person claiming it.

## PDF rendering

Written rather than pulled in. `pdf/writer.rs` emits PDF 1.7 directly; `pdf/layout.rs` is a
block-flow engine; `pdf/mod.rs` turns a `ReportContent` into blocks.

The reasons: complete control of the document model and typography, deterministic output,
no system libraries on Kali or Ubuntu, and no dependency whose API might shift under a
security-reporting tool. Text is measured with the real Adobe Font Metrics for the base-14
fonts, generated into `pdf/metrics.rs` by `scripts/generate_font_metrics.py`, so line
breaking is accurate rather than estimated.

Pagination is two-pass. The body is laid out once to discover the headings and how many
pages it takes, which determines how many pages the contents will occupy; then it is laid
out again knowing which page number it starts on. Running headers and footers are drawn
last, because "Page 3 of 11" is not knowable until the final page exists.

The trade-off is encoding: base-14 fonts mean WinAnsi (Latin-1). Characters outside it
render as `?`. Full Unicode requires embedding a TrueType font, which is the planned change.

## Evidence

Files are copied into a managed vault, never referenced in place — a report that cites
evidence must still resolve after the user tidies their Downloads folder.

On import: the source path is validated (absolute, a regular file, within the size limit),
the type is determined by **sniffing the bytes** rather than trusting the extension, the
policy check runs, the SHA-256 is recorded, the filename is sanitized to a single safe
component, the destination is proved to be inside the vault, and the file is written
owner-only and never executable.

On read the digest is re-checked, so a modified copy in the vault is detected rather than
quietly used.

## Frontend

React with TypeScript, Vite, hand-written CSS with custom properties for theming. No UI
framework: the surface is small enough that a framework would add more to learn than it
removes, and a tool that renders client data benefits from having every rule that does so
visible in one stylesheet.

`src/services/api.ts` is the only file that imports from `@tauri-apps/api`. Everything else
calls typed methods on `api`, which gives one place that knows about `invoke`, one error
shape, and one place where a session ending is turned into a global event.

The session token never reaches JavaScript. It lives in `src-tauri/src/state.rs`; commands
resolve the acting user from it on every call. A compromised web view cannot lift a bearer
token out of frontend memory because there is not one there.

## Decisions worth recording

**Why not `printpdf` or a headless browser?** A layout engine is the part of this product
that most needs to be predictable and dependency-free. A browser-based renderer would add a
heavyweight runtime dependency to a security tool; a PDF crate would put the document model
behind someone else's API.

**Why a hand-written diff?** The AI review pane needs word-level changes over short
passages. A longest-common-subsequence diff over whitespace tokens is about eighty lines
and behaves exactly as required, including reconstructing both sides byte for byte.

**Why is severity a separate command?** Because it is the one judgement the product refuses
to make. Keeping it off the ordinary edit path means no future refactor of `FindingUpdate`
can accidentally let a severity through, and the confirmation flag is checked in the core,
not the UI.

**Why LIKE search instead of FTS5?** Search covers titles, references, assets, filenames
and authors — fields, not prose. FTS5 would need index maintenance on every write to answer
questions a bound `LIKE` over indexed columns already answers.

**Why no DOCX?** A valid `.docx` is an OOXML package, not an XML file. Shipping something
that only looks like Word output would be worse than not offering it. It is listed as not
implemented rather than quietly approximated.

## Integration points

The specification asks that DJINN, Kali tooling, scanners, SIEM, ticketing and cloud APIs
remain possible without AuditForge depending on any of them. Three seams make that so:

- **`AiProvider`** — any analysis service that takes text and returns structured output is
  another implementation of one trait with one method.
- **`auditforge-core` as a library** — an importer that creates findings from Nmap or a
  scanner is a consumer of `findings::create`, not a change to the application.
- **Export formats** — JSON export is the full stored document, which is what a downstream
  analysis system would consume.

None of these are implemented. That is deliberate: the MVP is a reporting tool that works
on its own.
