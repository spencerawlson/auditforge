//! The report template engine.
//!
//! A template is data, not code: it declares the sections a report type has, the guided
//! questions inside each section, which are required, and the order everything appears
//! in. The eleven built-in templates ship as JSON under `templates/` and are embedded in
//! the binary so a packaged application never depends on files being installed beside
//! it. User templates can be dropped into the templates directory of the application
//! data folder and are loaded on top of the built-ins.

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub mod tables;
pub use tables::TemplateColumn;

use crate::database::new_id;
use crate::error::{Error, Result};
use crate::models::{
    AnswerValue, AuthorSnapshot, Classification, EngagementSnapshot, Provenance, ReportContent,
    ReportType, SectionContent, TemplateSource, Timestamp, CONTENT_SCHEMA_VERSION,
};

/// How a section gets its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// Filled in by answering the section's questions.
    Guided,
    /// Rendered from the findings linked to the report.
    Findings,
    /// Rendered from the evidence linked to the report.
    Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Textarea,
    Date,
    /// One item per line.
    List,
    /// A grid of rows the author adds one at a time. The question declares the columns;
    /// the answer stores the rows. See [`tables`].
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateQuestion {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub help: String,
    #[serde(default = "default_input")]
    pub input: InputKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub placeholder: String,
    /// Columns, for `input: table` only. Empty for every other input kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<TemplateColumn>,
    /// Label for the "add a row" control, e.g. "Add asset". Falls back to "Add row".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub row_label: String,
}

impl TemplateQuestion {
    pub fn is_table(&self) -> bool {
        self.input == InputKind::Table
    }
}

fn default_input() -> InputKind {
    InputKind::Textarea
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateSection {
    pub key: String,
    pub title: String,
    pub order: i64,
    #[serde(default = "default_kind")]
    pub kind: SectionKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub guidance: String,
    #[serde(default)]
    pub questions: Vec<TemplateQuestion>,
}

fn default_kind() -> SectionKind {
    SectionKind::Guided
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportTemplate {
    pub key: String,
    pub name: String,
    pub report_type: ReportType,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default_classification: Classification,
    pub sections: Vec<TemplateSection>,
    /// Set by the loader, not by the file.
    #[serde(default = "default_source")]
    pub source: TemplateSource,
}

fn default_version() -> u32 {
    1
}

fn default_source() -> TemplateSource {
    TemplateSource::Builtin
}

/// Row for the template picker.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub key: String,
    pub name: String,
    pub report_type: ReportType,
    pub description: String,
    pub section_count: usize,
    pub question_count: usize,
    pub source: TemplateSource,
    pub default_classification: Classification,
}

impl ReportTemplate {
    pub fn section(&self, key: &str) -> Option<&TemplateSection> {
        self.sections.iter().find(|section| section.key == key)
    }

    pub fn question(&self, section_key: &str, question_key: &str) -> Option<&TemplateQuestion> {
        self.section(section_key)?
            .questions
            .iter()
            .find(|q| q.key == question_key)
    }

    pub fn summary(&self) -> TemplateSummary {
        TemplateSummary {
            key: self.key.clone(),
            name: self.name.clone(),
            report_type: self.report_type,
            description: self.description.clone(),
            section_count: self.sections.len(),
            question_count: self.sections.iter().map(|s| s.questions.len()).sum(),
            source: self.source,
            default_classification: self.default_classification,
        }
    }

    /// Create the empty document body for a new report from this template.
    ///
    /// Every question is materialized with an empty answer so the guided form has a
    /// stable set of fields and so `missing_answers` can report what still needs input.
    pub fn instantiate(
        &self,
        title: &str,
        classification: Classification,
        author: AuthorSnapshot,
        engagement: EngagementSnapshot,
    ) -> ReportContent {
        let now = Timestamp::now();
        let mut sections: Vec<SectionContent> = self
            .sections
            .iter()
            .map(|section| SectionContent {
                key: section.key.clone(),
                title: section.title.clone(),
                order: section.order,
                included: true,
                answers: section
                    .questions
                    .iter()
                    .map(|question| AnswerValue {
                        question_key: question.key.clone(),
                        value: String::new(),
                        provenance: Provenance::UserProvided,
                        ai_request_id: None,
                        updated_at: now,
                    })
                    .collect(),
            })
            .collect();
        sections.sort_by_key(|section| section.order);

        ReportContent {
            schema_version: CONTENT_SCHEMA_VERSION,
            template_key: self.key.clone(),
            report_type: self.report_type,
            title: title.to_string(),
            classification,
            author,
            engagement,
            sections,
            findings: Vec::new(),
            evidence: Vec::new(),
            report_ref: String::new(),
            version_label: String::new(),
            generated_at: now,
        }
    }

    /// Bring an existing document body up to date with the template, adding sections and
    /// questions that did not exist when the report was created. Existing answers are
    /// never touched, and answers whose question has been removed are kept rather than
    /// discarded - losing an author's words to a template edit would be worse than
    /// carrying an orphan.
    pub fn reconcile(&self, content: &mut ReportContent) {
        let now = Timestamp::now();
        for section in &self.sections {
            match content
                .sections
                .iter_mut()
                .find(|existing| existing.key == section.key)
            {
                Some(existing) => {
                    existing.title = section.title.clone();
                    existing.order = section.order;
                    for question in &section.questions {
                        if !existing
                            .answers
                            .iter()
                            .any(|a| a.question_key == question.key)
                        {
                            existing.answers.push(AnswerValue {
                                question_key: question.key.clone(),
                                value: String::new(),
                                provenance: Provenance::UserProvided,
                                ai_request_id: None,
                                updated_at: now,
                            });
                        }
                    }
                }
                None => content.sections.push(SectionContent {
                    key: section.key.clone(),
                    title: section.title.clone(),
                    order: section.order,
                    included: true,
                    answers: section
                        .questions
                        .iter()
                        .map(|question| AnswerValue {
                            question_key: question.key.clone(),
                            value: String::new(),
                            provenance: Provenance::UserProvided,
                            ai_request_id: None,
                            updated_at: now,
                        })
                        .collect(),
                }),
            }
        }
        content.sections.sort_by_key(|section| section.order);
    }
}

/// Reject a template that would produce a broken report before it is ever stored.
pub fn validate(template: &ReportTemplate) -> Result<()> {
    if template.key.trim().is_empty() {
        return Err(Error::validation("template key is required"));
    }
    if !template
        .key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(Error::validation(
            "template key may contain only lowercase letters, digits and underscores",
        ));
    }
    if template.name.trim().is_empty() {
        return Err(Error::validation("template name is required"));
    }
    if template.sections.is_empty() {
        return Err(Error::validation("a template needs at least one section"));
    }

    let mut section_keys = BTreeSet::new();
    for section in &template.sections {
        if section.key.trim().is_empty() {
            return Err(Error::validation("every section needs a key"));
        }
        if !section_keys.insert(section.key.as_str()) {
            return Err(Error::validation(format!(
                "duplicate section key: {}",
                section.key
            )));
        }
        if section.title.trim().is_empty() {
            return Err(Error::validation(format!(
                "section {} needs a title",
                section.key
            )));
        }
        if section.kind == SectionKind::Guided && section.required && section.questions.is_empty() {
            return Err(Error::validation(format!(
                "section {} is required but has no questions",
                section.key
            )));
        }

        let mut question_keys = BTreeSet::new();
        for question in &section.questions {
            if question.key.trim().is_empty() {
                return Err(Error::validation(format!(
                    "a question in section {} has no key",
                    section.key
                )));
            }
            if !question_keys.insert(question.key.as_str()) {
                return Err(Error::validation(format!(
                    "duplicate question key {} in section {}",
                    question.key, section.key
                )));
            }
            if question.label.trim().is_empty() {
                return Err(Error::validation(format!(
                    "question {} needs a label",
                    question.key
                )));
            }
        }
    }
    Ok(())
}

macro_rules! builtin {
    ($($key:literal),+ $(,)?) => {
        &[ $( ($key, include_str!(concat!("../../../../templates/", $key, ".json"))) ),+ ]
    };
}

/// The templates shipped with the application, embedded at build time.
pub const BUILTIN_TEMPLATES: &[(&str, &str)] = builtin![
    "penetration_test",
    "vulnerability_assessment",
    "security_assessment",
    "network_assessment",
    "incident_report",
    "postmortem",
    "cloud_security_assessment",
    "compliance_audit",
    "engagement_report",
    "executive_summary",
    "custom_report",
];

pub fn parse_builtin() -> Result<Vec<ReportTemplate>> {
    BUILTIN_TEMPLATES
        .iter()
        .map(|(key, json)| {
            let mut template: ReportTemplate = serde_json::from_str(json).map_err(|e| {
                Error::internal(format!("built-in template {key} is malformed: {e}"))
            })?;
            template.source = TemplateSource::Builtin;
            validate(&template)?;
            Ok(template)
        })
        .collect()
}

/// Write the built-in templates into the database, refreshing them when the shipped
/// version is newer. User templates with the same key are left alone.
pub fn sync_builtins(conn: &Connection) -> Result<usize> {
    let mut updated = 0;
    for template in parse_builtin()? {
        let existing: Option<(String, i64)> = conn
            .query_row(
                "SELECT source, version FROM templates WHERE template_key = ?1",
                [&template.key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        match existing {
            Some((source, version))
                if source == TemplateSource::Builtin.as_str()
                    && version >= i64::from(template.version) =>
            {
                continue
            }
            Some((source, _)) if source == TemplateSource::User.as_str() => continue,
            _ => {}
        }

        upsert(conn, &template)?;
        updated += 1;
    }
    Ok(updated)
}

/// Load `.json` templates from a directory, skipping any that fail to parse so one bad
/// file cannot stop the application from starting.
pub fn import_user_templates(conn: &Connection, dir: &Path) -> Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut imported = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(_) => continue,
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(?path, %error, "could not read user template");
                continue;
            }
        };
        match serde_json::from_str::<ReportTemplate>(&raw) {
            Ok(mut template) => {
                template.source = TemplateSource::User;
                if let Err(error) = validate(&template) {
                    tracing::warn!(?path, %error, "user template rejected");
                    continue;
                }
                upsert(conn, &template)?;
                imported.push(template.key);
            }
            Err(error) => tracing::warn!(?path, %error, "user template is not valid JSON"),
        }
    }
    Ok(imported)
}

fn upsert(conn: &Connection, template: &ReportTemplate) -> Result<()> {
    let now = Timestamp::now();
    conn.execute(
        "INSERT INTO templates (id, template_key, name, report_type, version, source,
                                definition_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(template_key) DO UPDATE SET
            name = ?3, report_type = ?4, version = ?5, source = ?6,
            definition_json = ?7, updated_at = ?8",
        rusqlite::params![
            new_id(),
            template.key,
            template.name,
            template.report_type,
            template.version,
            template.source,
            serde_json::to_string(template)?,
            now,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, key: &str) -> Result<ReportTemplate> {
    let raw: String = conn
        .query_row(
            "SELECT definition_json FROM templates WHERE template_key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::not_found("template"))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn list(conn: &Connection) -> Result<Vec<TemplateSummary>> {
    let mut stmt = conn.prepare("SELECT definition_json FROM templates ORDER BY name")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .iter()
        .filter_map(|raw| serde_json::from_str::<ReportTemplate>(raw).ok())
        .map(|template| template.summary())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn author() -> AuthorSnapshot {
        AuthorSnapshot {
            user_id: "u1".to_string(),
            username: "analyst".to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            captured_at: Timestamp::now(),
        }
    }

    #[test]
    fn every_builtin_template_parses_and_validates() {
        let templates = parse_builtin().unwrap();
        assert_eq!(
            templates.len(),
            11,
            "the spec calls for eleven report types"
        );
        for template in &templates {
            validate(template).unwrap_or_else(|e| panic!("{} invalid: {e}", template.key));
            assert!(!template.sections.is_empty());
        }
    }

    #[test]
    fn template_keys_and_report_types_are_unique() {
        let templates = parse_builtin().unwrap();
        let keys: BTreeSet<_> = templates.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys.len(), templates.len());
        let types: BTreeSet<_> = templates.iter().map(|t| t.report_type.as_str()).collect();
        assert_eq!(
            types.len(),
            templates.len(),
            "each template covers a distinct report type"
        );
    }

    #[test]
    fn question_keys_are_unique_across_a_whole_template() {
        // Answers are addressed by (section, question), but a globally unique key makes
        // cross-section review warnings unambiguous.
        for template in parse_builtin().unwrap() {
            let mut seen = BTreeSet::new();
            for section in &template.sections {
                for question in &section.questions {
                    assert!(
                        seen.insert(question.key.clone()),
                        "{}: duplicate question key {}",
                        template.key,
                        question.key
                    );
                }
            }
        }
    }

    #[test]
    fn instantiate_produces_every_question_as_an_empty_answer() {
        let template = parse_builtin()
            .unwrap()
            .into_iter()
            .find(|t| t.key == "penetration_test")
            .unwrap();
        let content = template.instantiate(
            "Q3 External Test",
            Classification::Confidential,
            author(),
            EngagementSnapshot::default(),
        );

        assert_eq!(content.template_key, "penetration_test");
        assert_eq!(content.sections.len(), template.sections.len());
        let expected: usize = template.sections.iter().map(|s| s.questions.len()).sum();
        let actual: usize = content.sections.iter().map(|s| s.answers.len()).sum();
        assert_eq!(actual, expected);
        assert!(content.sections.iter().all(|s| s.included));
        assert!(content
            .sections
            .windows(2)
            .all(|pair| pair[0].order <= pair[1].order));
        // A brand new report is entirely missing information, by construction.
        assert_eq!(content.missing_answers().len(), expected);
    }

    #[test]
    fn reconcile_adds_new_questions_without_touching_existing_answers() {
        let mut template = parse_builtin()
            .unwrap()
            .into_iter()
            .find(|t| t.key == "executive_summary")
            .unwrap();
        let mut content = template.instantiate(
            "Board Summary",
            Classification::Internal,
            author(),
            EngagementSnapshot::default(),
        );
        content.sections[0].answers[0].value = "Author's own words".to_string();

        template.sections[0].questions.push(TemplateQuestion {
            key: "es_new_question".to_string(),
            label: "Something added later".to_string(),
            help: String::new(),
            input: InputKind::Textarea,
            required: false,
            placeholder: String::new(),
            columns: Vec::new(),
            row_label: String::new(),
        });
        template.reconcile(&mut content);

        assert_eq!(content.sections[0].answers[0].value, "Author's own words");
        assert!(content.sections[0]
            .answers
            .iter()
            .any(|a| a.question_key == "es_new_question"));
    }

    #[test]
    fn reconcile_keeps_answers_whose_question_was_removed() {
        let mut template = parse_builtin()
            .unwrap()
            .into_iter()
            .find(|t| t.key == "custom_report")
            .unwrap();
        let mut content = template.instantiate(
            "Ad hoc",
            Classification::Internal,
            author(),
            EngagementSnapshot::default(),
        );
        let orphan_key = template.sections[0].questions[0].key.clone();
        content.sections[0]
            .answers
            .iter_mut()
            .find(|a| a.question_key == orphan_key)
            .unwrap()
            .value = "Do not lose me".to_string();

        template.sections[0].questions.remove(0);
        template.reconcile(&mut content);

        let kept = content.sections[0]
            .answers
            .iter()
            .find(|a| a.question_key == orphan_key)
            .expect("orphaned answer must survive");
        assert_eq!(kept.value, "Do not lose me");
    }

    #[test]
    fn validation_catches_malformed_templates() {
        let mut template = parse_builtin().unwrap().into_iter().next().unwrap();
        template.key = "Not A Key".to_string();
        assert!(validate(&template).is_err());

        let mut template = parse_builtin().unwrap().into_iter().next().unwrap();
        let duplicate = template.sections[0].clone();
        template.sections.push(duplicate);
        assert!(validate(&template).is_err());

        let mut template = parse_builtin().unwrap().into_iter().next().unwrap();
        template.sections.clear();
        assert!(validate(&template).is_err());
    }

    #[test]
    fn builtins_sync_into_the_database_and_are_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let first = db.with_conn(sync_builtins).unwrap();
        assert_eq!(first, 11);
        let second = db.with_conn(sync_builtins).unwrap();
        assert_eq!(second, 0, "re-syncing an unchanged version must be a no-op");

        let listed = db.with_conn(list).unwrap();
        assert_eq!(listed.len(), 11);
        let template = db.with_conn(|conn| get(conn, "incident_report")).unwrap();
        assert_eq!(template.report_type, ReportType::IncidentReport);
        assert!(db.with_conn(|conn| get(conn, "nope")).is_err());
    }

    #[test]
    fn a_user_template_is_not_overwritten_by_the_shipped_one() {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(sync_builtins).unwrap();

        let mut custom = parse_builtin()
            .unwrap()
            .into_iter()
            .find(|t| t.key == "custom_report")
            .unwrap();
        custom.source = TemplateSource::User;
        custom.name = "My House Style".to_string();
        db.with_conn(|conn| upsert(conn, &custom)).unwrap();

        db.with_conn(sync_builtins).unwrap();
        let after = db.with_conn(|conn| get(conn, "custom_report")).unwrap();
        assert_eq!(after.name, "My House Style");
    }

    #[test]
    fn user_templates_import_from_disk_and_bad_files_are_skipped() {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(sync_builtins).unwrap();
        let dir = tempfile::tempdir().unwrap();

        let mut template = parse_builtin().unwrap().into_iter().next().unwrap();
        template.key = "house_style".to_string();
        template.name = "House Style".to_string();
        std::fs::write(
            dir.path().join("house_style.json"),
            serde_json::to_string(&template).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "irrelevant").unwrap();

        let imported = db
            .with_conn(|conn| import_user_templates(conn, dir.path()))
            .unwrap();
        assert_eq!(imported, vec!["house_style".to_string()]);
        let loaded = db.with_conn(|conn| get(conn, "house_style")).unwrap();
        assert_eq!(loaded.source, TemplateSource::User);
    }

    #[test]
    fn findings_and_evidence_sections_are_data_driven_not_question_driven() {
        let template = parse_builtin()
            .unwrap()
            .into_iter()
            .find(|t| t.key == "penetration_test")
            .unwrap();
        let findings = template.section("findings").unwrap();
        assert_eq!(findings.kind, SectionKind::Findings);
        assert!(findings.questions.is_empty());
        let evidence = template.section("evidence").unwrap();
        assert_eq!(evidence.kind, SectionKind::Evidence);
    }
}
