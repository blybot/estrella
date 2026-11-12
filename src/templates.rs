//! # Templates
//!
//! Document templates loaded from a directory of `*.json` files. A template
//! is a normal document plus a list of fields the user fills in:
//!
//! ```json
//! {
//!   "name": "TODO",
//!   "fields": [
//!     {"name": "title", "label": "Title", "type": "text", "required": true},
//!     {"name": "url", "type": "url"}
//!   ],
//!   "document": [
//!     {"type": "header", "content": "{{title}}"},
//!     {"type": "qr_code", "data": "{{url}}", "when": "url"}
//!   ],
//!   "cut": true
//! }
//! ```
//!
//! - `{{name}}` in any string is replaced by a field value or a builtin
//!   variable (`date`, `time`, ...). A field with the same name as a builtin
//!   wins.
//! - `"when": "field"` on a top-level component drops it when that field is
//!   empty.
//! - Values are substituted after the JSON is parsed, so they can't break or
//!   inject document structure.
//! - Mistakes in a template (unknown placeholders, `when` on an undeclared
//!   field, invalid components) are reported when it's loaded.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::document::{Document, builtin_variables};

/// Largest template file read.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// Most template files read from the directory.
const MAX_FILES: usize = 200;
/// Longest accepted field value, in characters.
const MAX_VALUE_CHARS: usize = 2000;

/// Top-level keys a template may have besides its document options.
const TEMPLATE_KEYS: &[&str] = &["name", "description", "fields"];
/// Document options a template may set.
const DOCUMENT_KEYS: &[&str] = &["document", "cut", "raster", "beep"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    /// Single-line text.
    #[default]
    Text,
    /// Multi-line text.
    Multiline,
    /// A date, `YYYY-MM-DD`.
    Date,
    /// An http(s) URL.
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: Vec<Field>,
    /// The document part of the file (`document`, `cut`, ...), unfilled.
    #[serde(skip)]
    body: Map<String, Value>,
}

/// A template that failed to load, with the reason.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrokenTemplate {
    pub id: String,
    pub error: String,
}

/// Why filling a template failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillError {
    /// Problems with submitted values, keyed by field name.
    Invalid(BTreeMap<String, String>),
    /// The filled template isn't a valid document.
    Document(String),
}

/// Every template in a directory, valid or not.
#[derive(Debug, Default)]
pub struct TemplateLibrary {
    pub templates: Vec<Template>,
    pub broken: Vec<BrokenTemplate>,
}

impl TemplateLibrary {
    /// Load every `*.json` file directly in `dir`, sorted by id.
    ///
    /// Symlinks are followed only to files inside `dir` (Kubernetes mounts
    /// ConfigMap files as symlinks into a hidden `..data` directory). Dot
    /// files and subdirectories are ignored.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let root = dir
            .canonicalize()
            .map_err(|e| format!("Can't read templates directory {}: {e}", dir.display()))?;
        let entries = std::fs::read_dir(&root)
            .map_err(|e| format!("Can't read templates directory {}: {e}", dir.display()))?;

        let mut files = BTreeMap::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = file_name.strip_suffix(".json") else {
                continue;
            };
            if file_name.starts_with('.') {
                continue;
            }
            files.insert(id.to_string(), entry.path());
            if files.len() >= MAX_FILES {
                break;
            }
        }

        let mut library = TemplateLibrary::default();
        for (id, path) in files {
            match read_template_file(&root, &id, &path) {
                Ok(Some(template)) => library.templates.push(template),
                Ok(None) => {}
                Err(error) => library.broken.push(BrokenTemplate { id, error }),
            }
        }
        Ok(library)
    }

    pub fn get(&self, id: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.id == id)
    }
}

/// Read one template file. `Ok(None)` means it isn't a regular file inside
/// the directory and is skipped.
fn read_template_file(root: &Path, id: &str, path: &Path) -> Result<Option<Template>, String> {
    let Ok(target) = path.canonicalize() else {
        return Ok(None);
    };
    if !target.starts_with(root) || !target.is_file() {
        return Ok(None);
    }
    if !is_valid_id(id) {
        return Err("file name must only use a-z, 0-9, '-' and '_'".into());
    }
    let size = target.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("file is larger than {MAX_FILE_BYTES} bytes"));
    }
    let text = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
    Template::parse(id, &text).map(Some)
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
}

impl Template {
    /// Parse and fully validate a template.
    pub fn parse(id: &str, text: &str) -> Result<Template, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
        let Value::Object(mut object) = value else {
            return Err("a template must be a JSON object".into());
        };

        for key in object.keys() {
            if !TEMPLATE_KEYS.contains(&key.as_str()) && !DOCUMENT_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "unknown key '{key}' (expected one of: {})",
                    [TEMPLATE_KEYS, DOCUMENT_KEYS].concat().join(", ")
                ));
            }
        }

        let name = match object.remove("name") {
            None => id.to_string(),
            Some(Value::String(name)) => name,
            Some(_) => return Err("'name' must be a string".into()),
        };
        let description = match object.remove("description") {
            None => None,
            Some(Value::String(d)) => Some(d),
            Some(_) => return Err("'description' must be a string".into()),
        };
        let fields: Vec<Field> = match object.remove("fields") {
            None => Vec::new(),
            Some(v) => serde_json::from_value(v).map_err(|e| format!("fields: {e}"))?,
        };

        let mut seen = Vec::new();
        for field in &fields {
            if !is_valid_name(&field.name) {
                return Err(format!(
                    "field name '{}' must use letters, digits and '_', and not start with a digit",
                    field.name
                ));
            }
            if seen.contains(&field.name.as_str()) {
                return Err(format!("field '{}' is declared twice", field.name));
            }
            seen.push(&field.name);
        }

        match object.get("document") {
            Some(Value::Array(_)) => {}
            Some(_) => return Err("'document' must be an array of components".into()),
            None => return Err("missing 'document'".into()),
        }

        let template = Template {
            id: id.to_string(),
            name,
            description,
            fields,
            body: object,
        };
        template.check_references()?;
        template.check_fills()?;
        Ok(template)
    }

    /// Every placeholder and `when` must name a field or builtin.
    fn check_references(&self) -> Result<(), String> {
        let known = self.known_names();

        let mut problems = Vec::new();
        visit_strings(&Value::Object(self.body.clone()), "", &mut |path, s| {
            for name in placeholders(s) {
                if !known.contains(&name.to_string()) {
                    problems.push(format!("{path}: unknown placeholder {{{{{name}}}}}"));
                }
            }
        });

        let components = self.body["document"].as_array().unwrap();
        for (i, component) in components.iter().enumerate() {
            match component.get("when") {
                None => {}
                Some(Value::String(name)) if self.field(name).is_some() => {}
                Some(Value::String(name)) => problems.push(format!(
                    "document[{i}].when: '{name}' isn't a declared field"
                )),
                Some(_) => problems.push(format!("document[{i}].when must be a field name")),
            }
            // `when` only works on top-level components; anywhere deeper
            // would be silently ignored when the document is read.
            if let Value::Object(inner) = component {
                for (key, value) in inner {
                    if key != "when" && contains_key(value, "when") {
                        problems.push(format!(
                            "document[{i}].{key}: 'when' only works on top-level components"
                        ));
                    }
                }
            }
        }

        match problems.is_empty() {
            true => Ok(()),
            false => Err(problems.join("; ")),
        }
    }

    /// Filling with sample values must produce a valid document, with every
    /// optional field both filled and empty.
    fn check_fills(&self) -> Result<(), String> {
        let sample = |field: &Field| match field.kind {
            FieldType::Text | FieldType::Multiline => "Sample".to_string(),
            FieldType::Date => "2026-01-31".to_string(),
            FieldType::Url => "https://example.com/".to_string(),
        };
        let all: HashMap<_, _> = self
            .fields
            .iter()
            .map(|f| (f.name.clone(), sample(f)))
            .collect();
        let required_only: HashMap<_, _> = self
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| (f.name.clone(), sample(f)))
            .collect();

        for values in [all, required_only] {
            match self.fill(&values) {
                Ok(_) => {}
                Err(FillError::Document(e)) => return Err(e),
                Err(FillError::Invalid(errors)) => {
                    return Err(format!("sample values rejected: {errors:?}"));
                }
            }
        }
        Ok(())
    }

    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    fn known_names(&self) -> Vec<String> {
        let mut names: Vec<String> = builtin_variables().into_keys().collect();
        names.extend(self.fields.iter().map(|f| f.name.clone()));
        names
    }

    /// Check submitted values against the fields.
    pub fn validate(&self, values: &HashMap<String, String>) -> Result<(), FillError> {
        let mut errors = BTreeMap::new();

        for name in values.keys() {
            if self.field(name).is_none() {
                errors.insert(name.clone(), "not a field of this template".into());
            }
        }

        for field in &self.fields {
            let value = values.get(&field.name).map(|v| v.trim()).unwrap_or("");
            if value.is_empty() {
                if field.required {
                    errors.insert(field.name.clone(), "required".into());
                }
                continue;
            }
            if value.chars().count() > MAX_VALUE_CHARS {
                errors.insert(
                    field.name.clone(),
                    format!("longer than {MAX_VALUE_CHARS} characters"),
                );
                continue;
            }
            let problem = match field.kind {
                FieldType::Text if value.contains('\n') => Some("must be a single line"),
                FieldType::Date
                    if chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() =>
                {
                    Some("must be a date (YYYY-MM-DD)")
                }
                FieldType::Url => match reqwest::Url::parse(value) {
                    Ok(url) if matches!(url.scheme(), "http" | "https") => None,
                    _ => Some("must be an http:// or https:// URL"),
                },
                _ => None,
            };
            if let Some(problem) = problem {
                errors.insert(field.name.clone(), problem.into());
            }
        }

        match errors.is_empty() {
            true => Ok(()),
            false => Err(FillError::Invalid(errors)),
        }
    }

    /// Validate `values` and fill the template into a document.
    pub fn fill(&self, values: &HashMap<String, String>) -> Result<Document, FillError> {
        self.validate(values)?;

        let mut vars = builtin_variables();
        for field in &self.fields {
            let value = values.get(&field.name).map(|v| v.trim()).unwrap_or("");
            vars.insert(field.name.clone(), value.to_string());
        }

        let mut body = self.body.clone();
        if let Some(Value::Array(components)) = body.get_mut("document") {
            components.retain_mut(|component| {
                let Value::Object(object) = component else {
                    return true;
                };
                match object.remove("when") {
                    Some(Value::String(name)) => {
                        !vars.get(&name).map(|v| v.is_empty()).unwrap_or(true)
                    }
                    _ => true,
                }
            });
        }

        let mut filled = Value::Object(body);
        substitute_strings(&mut filled, &vars);

        let mut document: Document = serde_json::from_value(filled)
            .map_err(|e| FillError::Document(format!("invalid document: {e}")))?;
        document.interpolate = false;
        Ok(document)
    }
}

/// Names of the `{{name}}` placeholders in `s`, in order.
fn placeholders(s: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) if is_valid_name(after[..end].trim()) => {
                names.push(after[..end].trim());
                rest = &after[end + 2..];
            }
            _ => rest = &rest[start + 1..],
        }
    }
    names
}

/// Replace `{{name}}` placeholders in one pass, so values containing `{{...}}`
/// are left as written. Text between braces that isn't a name is kept.
fn substitute(s: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) if is_valid_name(after[..end].trim()) => {
                let name = after[..end].trim();
                match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            _ => {
                out.push('{');
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn substitute_strings(value: &mut Value, vars: &HashMap<String, String>) {
    match value {
        Value::String(s) => {
            if s.contains("{{") {
                *s = substitute(s, vars);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| substitute_strings(v, vars)),
        Value::Object(map) => map.values_mut().for_each(|v| substitute_strings(v, vars)),
        _ => {}
    }
}

fn visit_strings(value: &Value, path: &str, f: &mut dyn FnMut(&str, &str)) {
    match value {
        Value::String(s) => f(path, s),
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                visit_strings(v, &format!("{path}[{i}]"), f);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                visit_strings(v, &child, f);
            }
        }
        _ => {}
    }
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map.contains_key(key) || map.values().any(|v| contains_key(v, key)),
        Value::Array(items) => items.iter().any(|v| contains_key(v, key)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Component;

    const TODO: &str = r#"{
        "name": "TODO",
        "fields": [
            {"name": "title", "label": "Title", "type": "text", "required": true},
            {"name": "description", "type": "multiline"},
            {"name": "due", "label": "Due date", "type": "date"},
            {"name": "url", "type": "url"}
        ],
        "document": [
            {"type": "header", "content": "{{title}}"},
            {"type": "text", "content": "{{description}}", "when": "description"},
            {"type": "text", "content": "Due: {{due}}", "when": "due"},
            {"type": "qr_code", "data": "{{url}}", "cell_size": 16, "when": "url"}
        ],
        "cut": true
    }"#;

    fn todo() -> Template {
        Template::parse("todo", TODO).unwrap()
    }

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.document
            .iter()
            .filter_map(|c| match c {
                Component::Text(t) => Some(t.content.clone()),
                Component::Header(h) => Some(h.content.clone()),
                Component::QrCode(q) => Some(format!("QR:{}", q.data)),
                _ => None,
            })
            .collect()
    }

    fn invalid(result: Result<Document, FillError>) -> BTreeMap<String, String> {
        match result {
            Err(FillError::Invalid(errors)) => errors,
            other => panic!("expected invalid values, got {other:?}"),
        }
    }

    #[test]
    fn fills_every_field() {
        let doc = todo()
            .fill(&values(&[
                ("title", "Buy milk"),
                ("description", "Oat, not dairy"),
                ("due", "2026-11-20"),
                ("url", "https://example.com/list"),
            ]))
            .unwrap();

        assert_eq!(
            texts(&doc),
            [
                "Buy milk",
                "Oat, not dairy",
                "Due: 2026-11-20",
                "QR:https://example.com/list"
            ]
        );
        assert!(doc.cut);
        assert!(!doc.interpolate);
    }

    #[test]
    fn components_whose_when_field_is_empty_are_dropped() {
        let doc = todo()
            .fill(&values(&[("title", "Buy milk"), ("url", "  ")]))
            .unwrap();

        assert_eq!(texts(&doc), ["Buy milk"]);
    }

    #[test]
    fn values_cannot_break_out_of_strings() {
        let doc = todo()
            .fill(&values(&[
                (
                    "description",
                    "line one\nsays \"hi\"}, {\"type\": \"divider\"",
                ),
                ("title", "x"),
            ]))
            .unwrap();

        assert_eq!(
            texts(&doc)[1],
            "line one\nsays \"hi\"}, {\"type\": \"divider\""
        );
        assert_eq!(doc.document.len(), 2);
    }

    #[test]
    fn placeholders_inside_values_are_not_expanded() {
        let doc = todo()
            .fill(&values(&[("title", "{{date}} {{url}}")]))
            .unwrap();

        assert_eq!(texts(&doc)[0], "{{date}} {{url}}");
    }

    #[test]
    fn builtins_are_available_and_fields_override_them() {
        let template = Template::parse(
            "t",
            r#"{"fields": [{"name": "date"}],
                "document": [{"type": "text", "content": "{{date}} / {{year}}"}]}"#,
        )
        .unwrap();

        let doc = template.fill(&values(&[("date", "someday")])).unwrap();

        let year = chrono::Local::now().format("%Y").to_string();
        assert_eq!(texts(&doc)[0], format!("someday / {year}"));
    }

    #[test]
    fn required_and_typed_fields_are_validated() {
        let errors = invalid(todo().fill(&values(&[
            ("title", "   "),
            ("due", "next tuesday"),
            ("url", "javascript:alert(1)"),
            ("colour", "red"),
        ])));

        assert_eq!(errors["title"], "required");
        assert_eq!(errors["due"], "must be a date (YYYY-MM-DD)");
        assert_eq!(errors["url"], "must be an http:// or https:// URL");
        assert_eq!(errors["colour"], "not a field of this template");
    }

    #[test]
    fn single_line_text_rejects_newlines_and_long_values_are_rejected() {
        let long = "a".repeat(MAX_VALUE_CHARS + 1);
        let errors = invalid(todo().fill(&values(&[("title", "a\nb"), ("description", &long)])));

        assert_eq!(errors["title"], "must be a single line");
        assert!(errors["description"].starts_with("longer than"));
    }

    #[test]
    fn values_are_trimmed() {
        let doc = todo().fill(&values(&[("title", "  Buy milk \n")])).unwrap();
        assert_eq!(texts(&doc)[0], "Buy milk");
    }

    #[test]
    fn name_defaults_to_the_id_and_fields_serialize_for_the_ui() {
        let template = Template::parse(
            "note",
            r#"{"document": [{"type": "text", "content": "hi"}]}"#,
        )
        .unwrap();
        assert_eq!(template.name, "note");

        let json = serde_json::to_value(todo()).unwrap();
        assert_eq!(json["fields"][2]["type"], "date");
        assert_eq!(json["fields"][2]["label"], "Due date");
        assert!(json.get("body").is_none());
    }

    fn parse_error(text: &str) -> String {
        Template::parse("t", text).unwrap_err()
    }

    #[test]
    fn unknown_placeholders_are_reported_with_their_location() {
        let error = parse_error(
            r#"{"fields": [{"name": "title"}],
                "document": [{"type": "text", "content": "{{titel}}"}]}"#,
        );
        assert!(error.contains("document[0].content"), "{error}");
        assert!(error.contains("{{titel}}"), "{error}");
    }

    #[test]
    fn when_must_name_a_declared_field() {
        let error = parse_error(
            r#"{"fields": [{"name": "url", "type": "url"}],
                "document": [{"type": "qr_code", "data": "{{url}}", "when": "link"}]}"#,
        );
        assert!(error.contains("document[0].when: 'link'"), "{error}");
    }

    #[test]
    fn when_is_rejected_below_the_top_level() {
        let error = parse_error(
            r#"{"fields": [{"name": "a"}],
                "document": [{"type": "canvas", "elements": [{"type": "text", "content": "x", "when": "a"}]}]}"#,
        );
        assert!(error.contains("only works on top-level"), "{error}");
    }

    #[test]
    fn invalid_components_are_reported() {
        let error = parse_error(r#"{"document": [{"type": "sparkles"}]}"#);
        assert!(error.contains("invalid document"), "{error}");
    }

    #[test]
    fn unknown_keys_and_field_typos_are_reported() {
        assert!(parse_error(r#"{"documnet": []}"#).contains("unknown key 'documnet'"));
        assert!(parse_error(r#"{"variables": {}, "document": []}"#).contains("unknown key"));
        assert!(
            parse_error(r#"{"fields": [{"name": "a", "typ": "url"}], "document": []}"#)
                .contains("fields")
        );
        assert!(
            parse_error(r#"{"fields": [{"name": "a", "type": "uri"}], "document": []}"#)
                .contains("fields")
        );
    }

    #[test]
    fn bad_field_names_and_duplicates_are_reported() {
        assert!(
            parse_error(r#"{"fields": [{"name": "due-date"}], "document": []}"#)
                .contains("field name")
        );
        assert!(
            parse_error(r#"{"fields": [{"name": "a"}, {"name": "a"}], "document": []}"#)
                .contains("declared twice")
        );
    }

    #[test]
    fn substitute_handles_edge_cases() {
        let vars = values(&[("a", "1"), ("b", "2")]);
        assert_eq!(substitute("{{a}}{{b}}", &vars), "12");
        assert_eq!(substitute("{{ a }}", &vars), "1");
        assert_eq!(substitute("{{a", &vars), "{{a");
        assert_eq!(
            substitute("{{not a name}} {{a}}", &vars),
            "{{not a name}} 1"
        );
        assert_eq!(substitute("{{{a}}}", &vars), "{1}");
        assert_eq!(substitute("café {{a}} ☕", &vars), "café 1 ☕");
    }

    #[test]
    fn placeholders_finds_names() {
        assert_eq!(
            placeholders("{{a}} and {{ b_2 }} not {{c d}}"),
            ["a", "b_2"]
        );
        assert_eq!(placeholders("{{{a}}}"), ["a"]);
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("estrella-templates-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir).unwrap();
            TempDir(dir)
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn library_loads_valid_and_reports_broken_templates() {
        let dir = TempDir::new();
        dir.write("todo.json", TODO);
        dir.write(
            "broken.json",
            r#"{"document": [{"type": "text", "content": "{{nope}}"}]}"#,
        );
        dir.write("Bad Name.json", TODO);
        dir.write("notes.txt", "ignored");
        dir.write(".hidden.json", TODO);
        std::fs::create_dir(dir.0.join("sub.json")).unwrap();

        let library = TemplateLibrary::load(&dir.0).unwrap();

        let ids: Vec<_> = library.templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["todo"]);
        let broken: Vec<_> = library.broken.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(broken, ["Bad Name", "broken"]);
        assert!(library.get("todo").is_some());
        assert!(library.get("../todo").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn library_follows_symlinks_inside_the_directory_only() {
        let dir = TempDir::new();
        let outside = TempDir::new();
        // ConfigMap layout: todo.json -> ..data/todo.json
        std::fs::create_dir(dir.0.join("..data")).unwrap();
        dir.write("..data/todo.json", TODO);
        std::os::unix::fs::symlink("..data/todo.json", dir.0.join("todo.json")).unwrap();
        outside.write("secret.json", TODO);
        std::os::unix::fs::symlink(outside.0.join("secret.json"), dir.0.join("escape.json"))
            .unwrap();

        let library = TemplateLibrary::load(&dir.0).unwrap();

        let ids: Vec<_> = library.templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["todo"]);
        assert!(library.broken.is_empty());
    }

    #[test]
    fn missing_directory_is_an_error() {
        let error = TemplateLibrary::load(Path::new("/nonexistent/estrella")).unwrap_err();
        assert!(error.contains("/nonexistent/estrella"));
    }
}
