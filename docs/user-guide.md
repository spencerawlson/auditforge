# User guide

Writing a report in AuditForge, start to finish.

## The idea

You answer questions. AuditForge writes the document.

There is no page layout to fight, no template to copy from last quarter, no heading levels
to keep consistent. Choose a report type, work through its questions, attach your findings
and evidence, and the renderer produces the cover page, the numbered sections, the table of
contents, the finding tables, the evidence appendix, and the headers and footers carrying
the classification, report id, version and page numbers.

## First run

Create an account. What you enter becomes the author on every report:

```
Author:        Spencer Rawlson Spady
Organization:  PLH Ecosystem, Inc.
Role:          Cloud Computing & Cybersecurity
```

You can change these later in **Settings → Profile**. Reports you have already finalized
keep the details they were finalized with.

There is no password recovery. Argon2id with a random salt means a forgotten password is a
lost database, which is the correct behaviour for a tool holding client material.

## 1. Create an engagement

**Engagements → New engagement.**

An engagement is the container: client, dates, scope, out-of-scope, authorization reference,
rules of engagement and team. Reports created inside it inherit the client, dates and team
onto their cover page.

Record the authorization reference — the signed ROE, contract or change ticket. The
application warns when it is missing, because a report that cannot cite its authorization is
a problem you would rather find now.

## 2. Create a report

**Reports → New report.** Give it a title and choose a type:

| Type | Use it for |
|---|---|
| Penetration Test Report | A full narrative test with findings and remediation |
| Vulnerability Assessment | Scan-driven work focused on identified vulnerabilities |
| Security Assessment | Broad posture across people, process and technology |
| Network Assessment | Architecture, segmentation and exposure |
| Incident Report | What happened, what it affected, how it was handled |
| Postmortem | A blameless review with corrective actions |
| Cloud Security Assessment | Identity, network, data and workload configuration |
| Compliance / Audit Report | Controls against a named framework |
| Engagement Report | General "what we did and what we found" |
| Executive Summary | A short standalone summary for leadership |
| Custom Report | Minimal structure; sections can be switched off |

**Templates** in the sidebar shows exactly which sections and questions each type asks,
before you commit.

The classification (Public, Internal, Confidential, Restricted) prints in the header and
footer of every page and on the cover.

## 3. Answer the questions

The editor has three columns.

**Left — the outline.** Every section, with a dot showing its state: red for something that
blocks finalization, amber for a warning, green for done. Click to move between them.

**Middle — the questions.** Each has a prompt and a note on what belongs in the answer.
Required questions are marked. Write plainly; the wording can be improved later, and the
facts are what matter.

Optional sections can be switched off with **Exclude from report**. The text you have
written is kept in case you switch it back on.

Two buttons under each answer are worth knowing:

- **Mark as a gap** — records the answer as missing information. It prints as
  "Information required.", which is honest, and it appears in the readiness list so you
  cannot forget it.
- **AI assist** — see below.

**Right — readiness, review, export and history.**

`Ctrl`/`Cmd`+`S` saves. Nothing is written until you save.

## 4. Record findings

**Findings → New finding**, or from the Findings section of the report editor.

A finding is recorded once and can be included in any number of reports. Fill in what you
have: title, affected asset, description, technical detail, impact, likelihood, risk, root
cause, remediation, recommendation, references, CVSS.

### Severity

Severity is set separately, through **Set severity**, and requires you to tick a
confirmation. AuditForge never assigns it, and no AI suggestion can.

A finding whose severity nobody has confirmed **blocks finalization** of any report
including it. The rationale you write is recorded in the audit trail alongside the change,
and it is what you will read when someone challenges the rating a year later.

## 5. Attach evidence

**Evidence → Import file.** The file is copied into a managed vault; your original stays
where it is. AuditForge records its SHA-256, stores it owner-only, and never opens or runs
it.

Tick **Encrypt this file at rest** for credentials or personal data.

Executables and scripts are refused by default — detected by their contents, so renaming
does not help. If you are deliberately keeping a sample, acknowledge it and the import
proceeds, with the acknowledgement recorded.

In the report, the Evidence section attaches items to the document. The appendix cites each
one with its reference, filename and digest so a reader can verify the copy they hold.
Images are embedded.

## 6. Use the assistant (optional)

AI is off unless you turn it on. Everything above works identically without it.

Press **AI assist** under any answer. You will see where your text would go before anything
is sent, and — for a cloud provider — exactly which values redaction will replace.

Choose a function:

| Function | What it does |
|---|---|
| Improve writing | Grammar, spelling, clarity, structure. Facts untouched. |
| Rewrite | Reworks the passage, preserving factual meaning. |
| Technical rewrite | Informal notes into professional technical prose. |
| Executive rewrite | Restated for a non-technical reader. |
| Summarize | Condensed, for an executive, technical, incident or finding audience. |
| Expand | Short notes into a fuller section, marking gaps rather than filling them. |

Then you get three views: **Original**, **AI suggestion**, and **Changes** with insertions
and deletions marked. Three buttons: **Accept**, **Edit before accepting**, **Reject**.

Nothing is applied until you choose. What you chose is recorded.

A badge shows how the text will be recorded if you accept:

- **AI wording** — the assistant reworded your material. The facts remain yours.
- **AI inference** — assistant-drafted prose you have not confirmed. This **blocks
  finalization** until you press "Confirm as mine" or rewrite it.

If a suggestion introduces an IP address, CVE, CVSS vector, date or MAC address that is not
in your text, it is discarded before you see it and you are told what it tried to add.

## 7. Run a quality review

**Quality review → Run review** in the right-hand column.

The reviewer looks for contradictions, unsupported claims, missing information, inconsistent
terminology, inconsistent severity, incomplete remediation, missing evidence and timeline
problems. It returns **warnings only** and never edits anything. Each warning quotes the
passage it refers to; quotes that do not actually appear in your report are dropped before
you see them.

Treat them as prompts to look again, not as corrections.

## 8. Check readiness

The **Readiness** panel separates two things:

- **Blockers** — finalization is refused until these are resolved. Unanswered required
  questions, unconfirmed AI inferences, findings without a confirmed severity, a missing
  title or author.
- **Warnings** — worth reading, but they do not block. Empty optional answers, no findings
  linked, no evidence attached, a finding with no remediation.

## 9. Preview and finalize

**Preview** shows the report's content and structure inside the window, without writing
anything to disk.

**Open the PDF** renders the real document, writes it to your exports folder and opens it in
your system PDF viewer. That is where pagination, headers and footers appear exactly as a
reader will see them. (The preview is HTML rather than an embedded PDF because the web view
on Linux has no built-in PDF viewer; showing you a blank frame would be worse than being
clear about it.)

Every page carries the version label, so a draft can never be mistaken for an issued report.

**Submit for review** moves the report to `v0.n Review`. It is still editable.

**Finalize report** asks you to confirm that you have reviewed it and take responsibility
for its contents, then seals it as `v1.0 Final`. From that point it is immutable — in the
application and in the database.

## 10. Export

**PDF**, **HTML** (one self-contained page), **Markdown**, **JSON** (the stored document,
for archival) and **findings CSV**. Each opens a save dialog with a sensible filename.

## Making a correction after finalizing

You cannot edit a finalized report. Press **Create new revision**: `v1.1 Revised` opens with
the finalized content, you make the correction, and finalizing it produces `v2.0 Final`. The
document you already issued is untouched and can still be re-rendered exactly as it was.

This is deliberate. Silently editing a report someone already has is the failure mode this
product exists to prevent.

## Incidents and postmortems

**Incidents** record what happened, when it was detected, what it affected, the impact, the
timeline, the response, the root cause and the lessons. Severity follows the same explicit
confirmation as findings.

**Postmortems** ask fifteen fixed questions — what happened, when, how it was detected, what
was affected, the impact, the timeline, the root cause, what was done, what went well, what
did not, what could improve, what corrective actions are required, who owns each, deadlines,
and how remediation will be verified. A completion meter shows what is still unanswered.

Corrective actions need an owner and a date, and cannot be marked **Verified** until you
record *how* they were verified. That field is what you will point at when someone asks
whether the fix actually worked.

## Search

`Ctrl`/`Cmd`+`K` from anywhere. Searches engagements, reports, findings, incidents,
postmortems and evidence metadata by title, reference, author, client, asset and filename.
Exact reference matches rank first. Evidence content is never searched — that would mean
decrypting files as a side effect of typing.

## Settings worth visiting

**Profile** — the author details on your cover pages.

**Security** — session timeouts, encrypt-evidence-by-default, password change, and database
backup.

**AI** — provider, model, endpoint, consent, redaction and generation parameters.

**Reports** — default type and classification, page size, whether to include a table of
contents, the evidence appendix, embedded images and provenance markers; plus theme and
density.

**Audit log** — everything that has happened, filterable by action.

## Backing up

**Settings → Security → Back up database** writes a consistent copy while you work. Copy the
`evidence/` folder separately: the database references evidence but does not contain it.
