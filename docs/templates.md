# Writing a report template

A template is data. It declares the sections a report type has, the questions inside each
section, which are required, and the order everything appears in. Nothing about rendering
lives in it: the renderer decides typography, numbering and pagination.

## Where they live

The eleven built-in templates are in `templates/` in the repository and are embedded in the
binary at build time.

Your own go in the `templates` folder of the application data directory:

```
~/.local/share/auditforge/templates/house_style.json
```

They are loaded at startup, on top of the built-ins. A user template is never overwritten by
an application update, even if it shares a key with a shipped one.

## Shape

```json
{
  "key": "house_style",
  "name": "House Style Assessment",
  "report_type": "security_assessment",
  "version": 1,
  "description": "Our standard assessment format.",
  "default_classification": "confidential",
  "sections": [
    {
      "key": "executive_summary",
      "title": "Executive Summary",
      "order": 1,
      "kind": "guided",
      "required": true,
      "guidance": "Written for a decision maker. Everything here must be supported elsewhere.",
      "questions": [
        {
          "key": "exec_purpose",
          "label": "What was assessed, and why?",
          "help": "One or two sentences a non-technical reader can follow.",
          "input": "textarea",
          "required": true,
          "placeholder": "Describe the target and the business reason."
        }
      ]
    },
    {
      "key": "findings",
      "title": "Findings",
      "order": 2,
      "kind": "findings",
      "guidance": "Built from the findings linked to this report.",
      "questions": []
    }
  ]
}
```

### Template fields

| Field | Required | Notes |
|---|---|---|
| `key` | yes | Lowercase letters, digits and underscores. Identifies the template. |
| `name` | yes | Shown in the picker. |
| `report_type` | yes | One of the values below. Drives the type shown on the cover. |
| `version` | no | Defaults to 1. Raise it when you change a shipped template. |
| `description` | no | One line in the picker. |
| `default_classification` | no | `public`, `internal`, `confidential` (default) or `restricted`. |
| `sections` | yes | At least one. |

`report_type` must be one of: `penetration_test`, `vulnerability_assessment`,
`security_assessment`, `network_assessment`, `incident_report`, `postmortem`,
`cloud_security_assessment`, `compliance_audit`, `engagement_report`, `executive_summary`,
`custom`.

### Section fields

| Field | Required | Notes |
|---|---|---|
| `key` | yes | Unique within the template. **Answers are addressed by it — never change it on a template already in use.** |
| `title` | yes | The heading in the document. |
| `order` | yes | Sort position. Numbering in the document is computed from included sections, so gaps are fine. |
| `kind` | no | `guided` (default), `findings` or `evidence`. |
| `required` | no | A required section cannot be excluded from a report. |
| `guidance` | no | Shown above the questions in the editor. Not printed. |
| `questions` | for guided | Empty for `findings` and `evidence` sections. |

**Section kinds.** `guided` sections are answered by the author. `findings` and `evidence`
sections are rendered from the records linked to the report — the editor shows a picker
instead of a question list, and the renderer builds the table and the detail entries.

### Question fields

| Field | Required | Notes |
|---|---|---|
| `key` | yes | Unique within the template. Same warning as section keys. |
| `label` | yes | The prompt. Phrase it as a question. |
| `help` | no | A sentence on what belongs in the answer. |
| `input` | no | `textarea` (default), `text`, `date` or `list`. |
| `required` | no | A blank required answer blocks finalization. |
| `placeholder` | no | Example text in the empty field. |

`date` inputs are validated as `YYYY-MM-DD`.

## Writing good questions

The built-in templates follow a few habits worth copying.

**Ask a question, do not name a field.** "What was in scope?" gets a better answer than
"Scope".

**Say what belongs in the answer.** The `help` line is where you put the thing you would
otherwise say in review: *"Anything deliberately excluded. This protects both parties
later."*

**Make room for not knowing.** Several built-in questions explicitly invite a blank: *"Leave
blank rather than speculating."* A report that says a cause is not established is more useful
than one that guesses.

**Mark required sparingly.** Required means finalization is refused. Use it for the things a
reader genuinely cannot do without.

## Validation

A template is checked on load and rejected — with the reason logged — if it has no key or
name, no sections, a key with characters outside `[a-z0-9_]`, duplicate section or question
keys, a section without a title, or a required guided section with no questions.

A malformed file is skipped rather than stopping the application from starting.

## Changing a template already in use

Answers are addressed by `(section key, question key)`. Renaming a *label* is always safe.
Changing a *key* is not: it orphans the answers stored under the old one.

When a report opens, it is reconciled with its template. New questions appear as empty
answers. **Answers whose question has been removed are kept** and still render — losing an
author's words to a template edit would be worse than carrying an orphan. The editor marks
such a section read-only and explains why.

Reports already finalized are unaffected: each version stores its own complete content and
does not read the template at all.

## Testing yours

```bash
cp house_style.json ~/.local/share/auditforge/templates/
```

Restart AuditForge, open **Templates** in the sidebar, and check that the sections and
questions read the way you intended. Then create a report from it and generate a PDF: the
numbering, the table of contents and the section order all follow from what you wrote.
