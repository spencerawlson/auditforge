-- AuditForge initial schema.
--
-- Conventions:
--   * Primary keys are UUIDv4 strings.
--   * Timestamps are RFC 3339 strings in UTC ("2026-08-24T09:15:00Z") so they sort
--     lexicographically and survive round-tripping through JSON unchanged.
--   * Human-facing identifiers (engagement_ref, report_ref, ...) are separate from the
--     primary key so they can be shown in reports without exposing internal ids.
--   * Free text is stored verbatim; no formatting is applied at rest.

CREATE TABLE organizations (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE users (
    id                    TEXT PRIMARY KEY,
    username              TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash         TEXT NOT NULL,          -- Argon2id PHC string, never a plaintext password
    full_name             TEXT NOT NULL,
    job_title             TEXT NOT NULL DEFAULT '',
    email                 TEXT NOT NULL DEFAULT '',
    organization_id       TEXT REFERENCES organizations(id) ON DELETE SET NULL,
    is_active             INTEGER NOT NULL DEFAULT 1,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until          TEXT,
    last_login_at         TEXT,
    password_changed_at   TEXT NOT NULL,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

-- Sessions store only a hash of the session token, so a stolen database file cannot be
-- replayed as a live session.
CREATE TABLE sessions (
    id            TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash    TEXT NOT NULL UNIQUE,
    created_at    TEXT NOT NULL,
    last_seen_at  TEXT NOT NULL,
    expires_at    TEXT NOT NULL,
    revoked_at    TEXT
);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at);

CREATE TABLE engagements (
    id                   TEXT PRIMARY KEY,
    engagement_ref       TEXT NOT NULL UNIQUE,
    name                 TEXT NOT NULL,
    client_name          TEXT NOT NULL DEFAULT '',
    organization_id      TEXT REFERENCES organizations(id) ON DELETE SET NULL,
    owner_user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    status               TEXT NOT NULL DEFAULT 'draft',
    start_date           TEXT,
    end_date             TEXT,
    objectives           TEXT NOT NULL DEFAULT '',
    scope                TEXT NOT NULL DEFAULT '',
    out_of_scope         TEXT NOT NULL DEFAULT '',
    authorization_ref    TEXT NOT NULL DEFAULT '',
    rules_of_engagement  TEXT NOT NULL DEFAULT '',
    notes                TEXT NOT NULL DEFAULT '',
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX idx_engagements_status ON engagements(status);
CREATE INDEX idx_engagements_owner ON engagements(owner_user_id);

CREATE TABLE engagement_team_members (
    id             TEXT PRIMARY KEY,
    engagement_id  TEXT NOT NULL REFERENCES engagements(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    role           TEXT NOT NULL DEFAULT '',
    email          TEXT NOT NULL DEFAULT '',
    sort_order     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_team_members_engagement ON engagement_team_members(engagement_id);

CREATE TABLE reports (
    id                 TEXT PRIMARY KEY,
    report_ref         TEXT NOT NULL UNIQUE,
    engagement_id      TEXT REFERENCES engagements(id) ON DELETE SET NULL,
    template_key       TEXT NOT NULL,
    report_type        TEXT NOT NULL,
    title              TEXT NOT NULL,
    classification     TEXT NOT NULL DEFAULT 'confidential',
    status             TEXT NOT NULL DEFAULT 'draft',
    current_version_id TEXT,
    created_by         TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    finalized_at       TEXT
);
CREATE INDEX idx_reports_engagement ON reports(engagement_id);
CREATE INDEX idx_reports_status ON reports(status);
CREATE INDEX idx_reports_created_by ON reports(created_by);

-- One row per saved version. Finalized rows are immutable: the triggers below back up
-- the application-level rule so no future code path can quietly rewrite history.
CREATE TABLE report_versions (
    id                   TEXT PRIMARY KEY,
    report_id            TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    version_major        INTEGER NOT NULL,
    version_minor        INTEGER NOT NULL,
    version_label        TEXT NOT NULL,
    status               TEXT NOT NULL,            -- draft | review | final
    content_json         TEXT NOT NULL,            -- serialized ReportContent
    author_snapshot_json TEXT NOT NULL,            -- author identity frozen at save time
    content_hash         TEXT NOT NULL,            -- sha256 of content_json
    notes                TEXT NOT NULL DEFAULT '',
    is_immutable         INTEGER NOT NULL DEFAULT 0,
    created_by           TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at           TEXT NOT NULL,
    finalized_at         TEXT,
    UNIQUE (report_id, version_major, version_minor)
);
CREATE INDEX idx_report_versions_report ON report_versions(report_id);

CREATE TRIGGER trg_report_versions_immutable_update
BEFORE UPDATE ON report_versions
FOR EACH ROW WHEN OLD.is_immutable = 1
BEGIN
    SELECT RAISE(ABORT, 'finalized report versions are immutable');
END;

CREATE TRIGGER trg_report_versions_immutable_delete
BEFORE DELETE ON report_versions
FOR EACH ROW WHEN OLD.is_immutable = 1
BEGIN
    SELECT RAISE(ABORT, 'finalized report versions cannot be deleted');
END;

CREATE TABLE findings (
    id                  TEXT PRIMARY KEY,
    finding_ref         TEXT NOT NULL UNIQUE,
    engagement_id       TEXT REFERENCES engagements(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    severity            TEXT NOT NULL DEFAULT 'informational',
    severity_confirmed  INTEGER NOT NULL DEFAULT 0,  -- severity is never set by AI alone
    category            TEXT NOT NULL DEFAULT '',
    affected_asset      TEXT NOT NULL DEFAULT '',
    description         TEXT NOT NULL DEFAULT '',
    technical_details   TEXT NOT NULL DEFAULT '',
    impact              TEXT NOT NULL DEFAULT '',
    likelihood          TEXT NOT NULL DEFAULT '',
    risk                TEXT NOT NULL DEFAULT '',
    root_cause          TEXT NOT NULL DEFAULT '',
    remediation         TEXT NOT NULL DEFAULT '',
    recommendation      TEXT NOT NULL DEFAULT '',
    verification_status TEXT NOT NULL DEFAULT 'unverified',
    reference_links     TEXT NOT NULL DEFAULT '',
    cvss_vector         TEXT NOT NULL DEFAULT '',
    cvss_score          REAL,
    provenance_json     TEXT NOT NULL DEFAULT '{}',  -- per-field provenance map
    author_user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    author_name         TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_findings_engagement ON findings(engagement_id);
CREATE INDEX idx_findings_severity ON findings(severity);

CREATE TABLE report_findings (
    report_id   TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    finding_id  TEXT NOT NULL REFERENCES findings(id) ON DELETE CASCADE,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (report_id, finding_id)
);

CREATE TABLE evidence (
    id                TEXT PRIMARY KEY,
    evidence_ref      TEXT NOT NULL UNIQUE,
    engagement_id     TEXT REFERENCES engagements(id) ON DELETE SET NULL,
    title             TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    original_filename TEXT NOT NULL,
    stored_filename   TEXT NOT NULL,   -- sanitized name inside the managed evidence vault
    relative_path     TEXT NOT NULL,   -- always relative to the evidence root, never absolute
    mime_type         TEXT NOT NULL,
    kind              TEXT NOT NULL,   -- image | log | text | csv | json | capture | archive | other
    byte_size         INTEGER NOT NULL,
    sha256            TEXT NOT NULL,
    is_encrypted      INTEGER NOT NULL DEFAULT 0,
    captured_at       TEXT,
    created_by        TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);
CREATE INDEX idx_evidence_engagement ON evidence(engagement_id);
CREATE INDEX idx_evidence_sha256 ON evidence(sha256);

CREATE TABLE evidence_links (
    id           TEXT PRIMARY KEY,
    evidence_id  TEXT NOT NULL REFERENCES evidence(id) ON DELETE CASCADE,
    target_type  TEXT NOT NULL,   -- engagement | report | finding | incident | postmortem
    target_id    TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    UNIQUE (evidence_id, target_type, target_id)
);
CREATE INDEX idx_evidence_links_target ON evidence_links(target_type, target_id);

CREATE TABLE incidents (
    id                 TEXT PRIMARY KEY,
    incident_ref       TEXT NOT NULL UNIQUE,
    engagement_id      TEXT REFERENCES engagements(id) ON DELETE SET NULL,
    title              TEXT NOT NULL,
    detected_at        TEXT,
    resolved_at        TEXT,
    reporter           TEXT NOT NULL DEFAULT '',
    severity           TEXT NOT NULL DEFAULT 'informational',
    severity_confirmed INTEGER NOT NULL DEFAULT 0,
    affected_systems   TEXT NOT NULL DEFAULT '',
    description        TEXT NOT NULL DEFAULT '',
    impact             TEXT NOT NULL DEFAULT '',
    timeline           TEXT NOT NULL DEFAULT '',
    detection_method   TEXT NOT NULL DEFAULT '',
    response_actions   TEXT NOT NULL DEFAULT '',
    root_cause         TEXT NOT NULL DEFAULT '',
    remediation        TEXT NOT NULL DEFAULT '',
    lessons_learned    TEXT NOT NULL DEFAULT '',
    status             TEXT NOT NULL DEFAULT 'open',
    created_by         TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
CREATE INDEX idx_incidents_severity ON incidents(severity);

CREATE TABLE postmortems (
    id             TEXT PRIMARY KEY,
    postmortem_ref TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    incident_id    TEXT REFERENCES incidents(id) ON DELETE SET NULL,
    engagement_id  TEXT REFERENCES engagements(id) ON DELETE SET NULL,
    status         TEXT NOT NULL DEFAULT 'draft',
    answers_json   TEXT NOT NULL DEFAULT '{}',   -- guided answers, each carrying provenance
    created_by     TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE corrective_actions (
    id                  TEXT PRIMARY KEY,
    action_ref          TEXT NOT NULL UNIQUE,
    postmortem_id       TEXT REFERENCES postmortems(id) ON DELETE CASCADE,
    incident_id         TEXT REFERENCES incidents(id) ON DELETE CASCADE,
    finding_id          TEXT REFERENCES findings(id) ON DELETE CASCADE,
    description         TEXT NOT NULL,
    owner               TEXT NOT NULL DEFAULT '',
    due_date            TEXT,
    status              TEXT NOT NULL DEFAULT 'open',
    verification_method TEXT NOT NULL DEFAULT '',
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_corrective_actions_postmortem ON corrective_actions(postmortem_id);

CREATE TABLE templates (
    id              TEXT PRIMARY KEY,
    template_key    TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    report_type     TEXT NOT NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    source          TEXT NOT NULL DEFAULT 'builtin',   -- builtin | user
    definition_json TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Every AI call is recorded, including the accept/edit/reject decision, so a report can
-- be audited afterwards for what was machine-assisted.
CREATE TABLE ai_requests (
    id            TEXT PRIMARY KEY,
    user_id       TEXT REFERENCES users(id) ON DELETE SET NULL,
    provider      TEXT NOT NULL,        -- openai | local | disabled
    privacy_mode  TEXT NOT NULL,        -- offline | local | cloud
    model         TEXT NOT NULL DEFAULT '',
    function      TEXT NOT NULL,        -- improve_writing | rewrite | ... | report_review
    target_type   TEXT NOT NULL DEFAULT '',
    target_id     TEXT NOT NULL DEFAULT '',
    prompt_hash   TEXT NOT NULL DEFAULT '',
    input_chars   INTEGER NOT NULL DEFAULT 0,
    output_chars  INTEGER NOT NULL DEFAULT 0,
    confidence    TEXT NOT NULL DEFAULT '',   -- high | medium | low
    status        TEXT NOT NULL,              -- succeeded | failed | blocked
    error         TEXT NOT NULL DEFAULT '',
    decision      TEXT,                       -- accepted | edited | rejected
    created_at    TEXT NOT NULL,
    completed_at  TEXT,
    decided_at    TEXT
);
CREATE INDEX idx_ai_requests_target ON ai_requests(target_type, target_id);

CREATE TABLE audit_logs (
    id           TEXT PRIMARY KEY,
    occurred_at  TEXT NOT NULL,
    user_id      TEXT,
    username     TEXT NOT NULL DEFAULT '',
    action       TEXT NOT NULL,
    target_type  TEXT NOT NULL DEFAULT '',
    target_id    TEXT NOT NULL DEFAULT '',
    outcome      TEXT NOT NULL DEFAULT 'success',
    detail_json  TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_audit_logs_time ON audit_logs(occurred_at);
CREATE INDEX idx_audit_logs_action ON audit_logs(action);
CREATE INDEX idx_audit_logs_target ON audit_logs(target_type, target_id);

-- Settings are per user when user_id is set, otherwise application wide.
-- Secret material (API keys) is NEVER stored here; see the OS keychain integration.
CREATE TABLE settings (
    id         TEXT PRIMARY KEY,
    user_id    TEXT REFERENCES users(id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (user_id, key)
);

CREATE TABLE tags (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE entity_tags (
    tag_id      TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    PRIMARY KEY (tag_id, entity_type, entity_id)
);
CREATE INDEX idx_entity_tags_entity ON entity_tags(entity_type, entity_id);

-- Monotonic counters backing the human-facing reference identifiers.
CREATE TABLE reference_counters (
    scope      TEXT NOT NULL,
    period     TEXT NOT NULL,
    next_value INTEGER NOT NULL,
    PRIMARY KEY (scope, period)
);
