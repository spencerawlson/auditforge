# Security

What AuditForge protects, how, and — as importantly — what it does not.

## Threat model

AuditForge is a single-user desktop application holding client material: findings before
they are disclosed, evidence, credentials captured during testing, and reports under
embargo. It runs on a workstation the user controls.

**In scope**

| Threat | Control |
|---|---|
| Another local account reads the data | Owner-only permissions on the database, vault and data directory |
| A stolen laptop or backup exposes credentials | Argon2id password hashing; session tokens stored as hashes; optional evidence encryption |
| A copied database is replayed as a live session | Only the SHA-256 of a session token is stored |
| Online password guessing | Lockout after five consecutive failures |
| Hostile evidence escapes the vault | Filename sanitizing plus resolved-path containment |
| Hostile evidence is executed | Never executed, never opened, never marked executable; executables refused by content |
| A report is altered after sign-off | Immutable versions, enforced in the application and by database triggers |
| Client text reaches a third party unintentionally | AI disabled by default; explicit consent required; privacy mode always visible |
| A model invents facts into a report | Provenance tracking, the fabrication guard, and a human decision on every suggestion |
| Injection through search or filters | Every user value is a bound parameter; `LIKE` wildcards escaped |
| Formula injection via CSV export | Leading `=`, `+`, `-`, `@` neutralized |
| Markup injection via HTML export | All report text HTML-escaped |
| A dispute over what a report said | Append-only audit log; per-version content hashes |

**Out of scope**

- An attacker who already has code execution as the user. They can read the database, the
  vault, and anything in the keychain the user can.
- Full-disk encryption. That is the operating system's job; AuditForge assumes it where the
  data warrants it.
- Protecting data after it leaves: an exported PDF is an ordinary file.
- Network attackers between the user and a configured AI provider, beyond requiring TLS for
  the cloud endpoint.

## Authentication

**Passwords** are hashed with Argon2id at the crate's default parameters (19 MiB, t=2,
p=1), each with a fresh random salt. Parameters are encoded in the stored PHC string, so
raising them later re-hashes on next login rather than invalidating accounts.

The policy requires at least 12 characters combining three of: lowercase, uppercase,
digits, symbols; rejects an obvious-password list; and caps length at 1024 so a
multi-megabyte "password" cannot be fed to the hasher.

A malformed stored hash is treated as a failed verification, not an error, so a corrupted
row cannot be probed to distinguish "no such user" from "bad hash".

**Unknown usernames** run through a real Argon2id verification against a generated dummy
hash, so response timing does not reveal which accounts exist. The error text is identical
for an unknown user and a wrong password.

**Lockout** after five consecutive failures, for fifteen minutes. While locked, even the
correct password is refused, and the response says how long to wait.

**Sessions** are 256-bit random tokens. Only the SHA-256 is stored. Two independent clocks
bound a session: an idle timeout that slides on use, and an absolute lifetime that does
not. Changing a password revokes every session.

The raw token never reaches JavaScript. It lives in the Tauri shell's state, and commands
resolve the acting user from it on each call.

## Storage

The database, evidence vault and data directory are created `0600`/`0700` on Unix. On
Windows they inherit the per-user AppData ACL.

Every query that takes user input uses bound parameters. Where SQL is built dynamically —
filters and search — only fixed fragments are concatenated, and `LIKE` patterns go through
`escape_like` so `%` and `_` are literal text. `tests/security.rs` fires a list of injection
payloads at every search and filter path and asserts they match nothing and break nothing,
then stores the same payloads as report content and asserts they round-trip verbatim,
because a penetration test report legitimately contains them.

## Evidence handling

1. The source path must be absolute and a regular file. Directories, devices, sockets and
   FIFOs are refused — reading `/dev/zero` as evidence would otherwise fill the vault.
2. Size is capped at 100 MiB.
3. The type is determined by **sniffing the leading bytes**, not by the extension. A PE, ELF,
   Mach-O or `#!` script named `screenshot.png` is still recognised as executable.
4. Executables and scripts are refused unless the user explicitly acknowledges importing
   one on purpose; the acknowledgement is recorded in the audit trail.
5. The filename is reduced to a single safe component: no separators, no leading dots, no
   Windows device names, length-capped, with runs of dots and underscores collapsed.
6. The destination is proved to be inside the vault after `..` and symlinks are resolved.
7. The file is written owner-only and never executable.
8. The SHA-256 of the original bytes is recorded, and re-checked on every read.

Optional encryption is AES-256-GCM with a random 96-bit nonce per file, under a vault key
held in the OS keychain. The evidence reference is passed as associated data, so a blob
cannot be moved between records without failing authentication.

**AuditForge never opens or executes evidence.** The UI offers "open with the system
handler", which hands the path to the desktop and lets the user decide.

## Secrets

The OpenAI API key goes into the operating system keychain: Secret Service (GNOME Keyring,
KWallet) on Linux, Credential Manager on Windows, Keychain on macOS. It is never written to
the database and never returned to the frontend — the UI learns only that a key exists.

Where no keychain is available, AuditForge falls back to an encrypted local file and says
so in Settings. **Be clear about what that fallback is worth:** it stops the key appearing
in plain text in a backup, a screen share or a `grep` of the data directory. It does not
protect against someone who can already read files as this user, because the key that
decrypts it sits beside it. On Kali, install `gnome-keyring` if that matters.

There is deliberately no `OPENAI_API_KEY` environment variable. A key read from the
environment would be invisible to the user and could be picked up from a shell profile they
had forgotten about.

## The AI safety model

The rule is: **the assistant helps with language; it must not fabricate facts.** Four
mechanisms enforce it, and the last one is the only one that is not advisory.

### 1. Privacy modes

Off by default. A cloud call requires a configured provider, a stored key, *and* an explicit
consent toggle — selecting OpenAI is not consent. The current mode is shown in the top bar
of every screen and again beside every AI action. A "local" endpoint pointing at another
host is described as leaving the machine, not as private.

### 2. Minimum context

A request carries the passage being worked on and a short caller-supplied context line,
both length-capped. Never a whole engagement, never a finding table, never the database.
Before a report review the user can display exactly what would be sent.

### 3. Redaction

On by default. IP addresses (v4 and v6), email addresses, cloud access key ids, bearer
tokens, `password=`-style assignments and PEM private key blocks are replaced with stable
placeholders before sending, and the real values are restored in the reply — so the author
sees their own data in a suggestion the provider never saw.

**Scope, stated plainly:** structured, mechanically recognisable values only. Not hostnames,
project code names or people's names, because a matcher loose enough to catch those would
mangle ordinary prose, and a guard with false positives trains users to ignore it. Redaction
is a safety net. Privacy mode is the control.

### 4. The fabrication guard

The prompt tells the model not to invent facts. The guard checks whether it did, because a
rule that is only asked for is not a rule.

Every hard identifier in the model's output — IPv4 and IPv6 addresses, CVE and CWE ids,
CVSS vectors, MAC addresses and ISO dates — is compared against the text the model was
given. Anything new means the suggestion is **discarded before the user sees it**, the
request is recorded as `blocked`, and the audit log names the invented values.

Scope again limited on purpose: identifiers whose form is unambiguous and whose invention
would be a serious defect. Hostnames, product names and prose claims are not covered, which
is why the review pane always shows the original beside the suggestion.

### Provenance and finalization

Every stored answer carries where it came from: `user_provided`, `ai_wording`,
`ai_inference` or `missing_information`. Functions that reword the author's material
propose `ai_wording`; functions that produce new prose propose `ai_inference`.

**An unconfirmed `ai_inference` blocks finalization.** Assistant-drafted prose cannot reach
a signed-off document without a person claiming it as their own. A report containing any
assisted text also carries an automatic "Statement of AI Assistance" section naming how much
and reaffirming that the facts, findings, severities and conclusions are the author's.

### What the assistant cannot touch

Severity. Not through a suggestion, not through an edit, not through any command other than
`set_finding_severity` / `set_incident_severity` with an explicit confirmation flag that the
core checks. A finding whose severity nobody has confirmed blocks the report that includes
it.

## Report integrity

A finalized version is immutable:

- `reports::ensure_editable` refuses edits, snapshots, review transitions and deletion.
- `BEFORE UPDATE` and `BEFORE DELETE` triggers on `report_versions` refuse the same at the
  storage layer, so no future code path or stray `sqlite3` session can rewrite history.

Each version stores a SHA-256 of its content, shown in the version history, so a document
can be checked against a copy issued earlier. Each version also embeds the author identity
and every finding and evidence record it cites, which is why editing a profile or deleting a
finding cannot alter a report already signed off.

## Audit trail

Append-only by API: the core exposes writes and reads for `audit_logs` and nothing else.
Sign-ins and failures, lockouts, every record creation and change, severity confirmations
with before and after, evidence imports with digests, every AI request with its provider and
privacy mode, every accept/edit/reject decision, blocked suggestions with the invented
values, finalizations, PDF generation and exports.

Entries carry ids, counts and outcomes — never report bodies, secrets or evidence contents.

## Reporting a vulnerability

Open an issue describing the impact and how to reproduce it. If it is sensitive, say so and
ask for a private channel before posting details.

## Verifying the controls yourself

```bash
cargo test --test security      # the assertions behind this document
cargo test --test workflow      # the end-to-end walkthrough
cargo test -p auditforge-core   # everything, including unit tests
```

`tests/security.rs` is organised by the classes above: injection, path traversal, hostile
filenames, oversized and mistyped uploads, authentication bypass, session handling and
integrity. Each test states an attack and asserts the product refuses it.
