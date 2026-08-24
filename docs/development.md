# Development

## Getting set up

```bash
npm install
cargo fetch
npm run app:dev
```

`npm run app:dev` starts Vite on port 5173 and the Tauri shell together. The database is
created and migrated on first launch; there is no initialization step and no server.

Set `AUDITFORGE_LOG=auditforge=debug,auditforge_core=debug` for verbose logging. Report
content is never logged — entries carry ids, counts and outcomes only.

## Where things live

```
crates/auditforge-core/src/
├── app/          the facade: paths, bootstrap, PDF and export orchestration, dashboard
├── models/       domain records and the shared vocabulary (enums, Timestamp, provenance)
├── database/     connection, migrations, reference counters, LIKE escaping
├── auth/         accounts, password policy, sessions, lockout
├── engagements/  findings/  evidence/  incidents/  postmortems/  reports/
├── templates/    the template engine and the built-in template loader
├── ai/           provider trait, OpenAI, local, prompts, schema validation, guard, diff
├── pdf/          the PDF writer, the layout engine, font metrics
├── export/       HTML, Markdown, JSON, CSV
├── search/       global search and tags
├── security/     path containment, crypto, secret stores, redaction
├── settings/     per-user settings sections
└── audit/        the append-only trail

src-tauri/src/
├── main.rs       window, plugins, command registration
├── state.rs      the session token (never leaves Rust)
├── error.rs      the error payload the frontend sees, and the blocking-work helper
└── commands/     one module per core area

src/
├── services/api.ts   the only file that imports @tauri-apps/api
├── types/            TypeScript mirrors of the Rust models
├── components/       shared primitives, badges, modal, toasts, diff view
└── features/         one folder per screen area
```

## Tests

```bash
cargo test                              # everything: 341 unit, 19 security, 2 workflow
cargo test -p auditforge-core --lib     # unit tests only
cargo test --test security              # the security regression suite
cargo test --test workflow              # the end-to-end walkthrough
npm test                                # frontend
```

Unit tests live beside the code they exercise. The two integration suites are in `tests/`,
wired into the core crate through `[[test]]` entries in its manifest so they sit next to the
migrations and templates they use.

### How the tests are written

Each test name states the behaviour, not the function: `a_finalized_report_refuses_edits`,
not `test_save_draft_2`. A failing name should tell you what broke without opening the file.

Tests assert on what a user or a reader would see. The PDF tests inflate the content streams
and search the rendered text rather than checking that a function was called; the security
tests state an attack and assert the product refuses it.

`crates/auditforge-core/src/test_support.rs` provides `seeded()`: an in-memory database with
the built-in templates loaded and one signed-in user.

For AI work, `ScriptedProvider` replays canned responses, so the whole accept/reject
workflow is exercised without a network or an API key.

## Adding things

### A field on a record

1. Add the column in a **new** migration. Never edit a shipped one — the checksum check
   exists precisely to catch that.
2. Add the field to the model, the row mapper and the update struct.
3. Add it to the TypeScript type in `src/types/index.ts`.
4. Surface it in the relevant screen.

### A report type

Write a JSON template in `templates/` and add its key to `BUILTIN_TEMPLATES` in
`templates/mod.rs`. Add the variant to `ReportType` in `models/common.rs` and to the
TypeScript union. See [templates.md](templates.md).

Nothing else changes: the editor, renderer and export all read the template.

### An AI provider

Implement `AiProvider` — four methods, one of which does the work — and add a variant to
`AiProviderKind` and the match in `provider::build`. The pipeline, validation, guard,
redaction and audit trail apply automatically.

### A command

Add the function to the right module under `src-tauri/src/commands/`, register it in the
`generate_handler!` list in `main.rs`, and add a typed wrapper to `src/services/api.ts`.

Commands that can take more than a moment — anything writing, rendering or calling a model —
should be `async` and wrap their work in `blocking(...)` so the window stays responsive.

## House rules

**The core does not know about Tauri.** If a domain rule needs a window handle, the rule is
in the wrong place.

**Nothing writes to a report without a human decision.** The AI layer returns suggestions.
`reports::save_draft` is the only path to stored content, and the frontend calls it after
Accept.

**Severity is never inferred.** It changes only through `set_severity`, only with
`confirmed_by_user: true`.

**Finalized versions are immutable.** Both the application check and the database trigger
must stay. `tests/security.rs` asserts both; if you find yourself removing one, stop.

**Errors are typed.** Return `Error::Validation` for something the user can fix, and keep
detail that would help an attacker out of the message.

**Every user value is a bound parameter.** If you build SQL dynamically, only fixed
fragments may be concatenated, and `LIKE` patterns go through `escape_like`.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test
npm run lint
npm run typecheck
npm test
```

All six are expected to pass with no output worth reading.

## Building packages

Must be done on Linux; cross-compiling a Tauri application is not supported.

```bash
./scripts/build-linux.sh    # checks dependencies, then builds both packages
```

Output lands in `src-tauri/target/release/bundle/`.

## Regenerating the font metrics

`crates/auditforge-core/src/pdf/metrics.rs` is generated from the Adobe Font Metrics files
for the base-14 fonts. It should not be edited by hand. To regenerate:

```bash
python scripts/generate_font_metrics.py crates/auditforge-core/src/pdf/metrics.rs
```

The script expects the AFM files and the Adobe Glyph List in a working directory; see the
comments at the top of the file. It is a one-off: the metrics of Helvetica do not change.
