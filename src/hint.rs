//! Open hint metadata for diagnostics, exports, and runtime operations.
//!
//! Hints are data records: libraries choose the `kind` symbol and optional
//! fields, while the kernel only defines the common transport shape. A hint can
//! be attached to existing diagnostics through a related diagnostic carrier,
//! keeping older diagnostic constructors source-compatible.

use crate::{
    capability::CapabilityName,
    datum::Datum,
    env::Cx,
    error::{Diagnostic, Result, Severity},
    id::Symbol,
    value::Value,
};

/// Open metadata that helps tools explain or route a diagnostic or operation.
///
/// The `kind` symbol is intentionally open. The remaining fields are common
/// slots used by shape checkers, runtime libraries, and agent-facing indexes to
/// expose expected inputs, argument names, capability requirements, examples,
/// and codec-safe surface forms without adding closed kernel enums.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintMetadata {
    /// Open symbol naming the hint kind.
    pub kind: Symbol,
    /// Short human-readable title.
    pub title: String,
    /// Optional longer detail text.
    pub detail: Option<String>,
    /// Searchable open tags.
    pub tags: Vec<Symbol>,
    /// Argument names or positions the hint describes.
    pub arguments: Vec<Symbol>,
    /// Capabilities required to follow the hint.
    pub capabilities: Vec<CapabilityName>,
    /// Codec-safe forms related to this hint.
    pub codec_forms: Vec<Symbol>,
    /// Short examples a tool can show or index.
    pub examples: Vec<String>,
}

impl HintMetadata {
    /// Builds a hint with a kind and title.
    pub fn new(kind: Symbol, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: None,
            tags: Vec::new(),
            arguments: Vec::new(),
            capabilities: Vec::new(),
            codec_forms: Vec::new(),
            examples: Vec::new(),
        }
    }

    /// Adds longer detail text.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Adds a searchable tag.
    pub fn with_tag(mut self, tag: Symbol) -> Self {
        self.tags.push(tag);
        self
    }

    /// Adds an argument name or position.
    pub fn with_argument(mut self, argument: Symbol) -> Self {
        self.arguments.push(argument);
        self
    }

    /// Adds a capability requirement.
    pub fn with_capability(mut self, capability: CapabilityName) -> Self {
        self.capabilities.push(capability);
        self
    }

    /// Adds a codec-safe form tag.
    pub fn with_codec_form(mut self, form: Symbol) -> Self {
        self.codec_forms.push(form);
        self
    }

    /// Adds a short example.
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples.push(example.into());
        self
    }

    /// Appends this hint to a diagnostic as a related metadata carrier.
    pub fn attach_to(self, mut diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.related.push(self.to_diagnostic());
        diagnostic
    }

    /// Converts this hint into a related diagnostic carrier.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diagnostic = Diagnostic::info(self.title.clone());
        diagnostic.code = Some(hint_metadata_code());
        diagnostic
            .related
            .push(hint_field("kind", self.kind.to_string()));
        if let Some(detail) = &self.detail {
            diagnostic
                .related
                .push(hint_field("detail", detail.clone()));
        }
        for tag in &self.tags {
            diagnostic.related.push(hint_field("tag", tag.to_string()));
        }
        for argument in &self.arguments {
            diagnostic
                .related
                .push(hint_field("argument", argument.to_string()));
        }
        for capability in &self.capabilities {
            diagnostic
                .related
                .push(hint_field("capability", capability.as_str().to_owned()));
        }
        for form in &self.codec_forms {
            diagnostic
                .related
                .push(hint_field("codec-form", form.to_string()));
        }
        for example in &self.examples {
            diagnostic
                .related
                .push(hint_field("example", example.clone()));
        }
        diagnostic
    }

    /// Reconstructs hint metadata from a related diagnostic carrier.
    pub fn from_diagnostic(diagnostic: &Diagnostic) -> Option<Self> {
        if !Self::is_hint_diagnostic(diagnostic) {
            return None;
        }

        let mut kind = None;
        let mut detail = None;
        let mut tags = Vec::new();
        let mut arguments = Vec::new();
        let mut capabilities = Vec::new();
        let mut codec_forms = Vec::new();
        let mut examples = Vec::new();

        for field in &diagnostic.related {
            let Some(name) = hint_field_name(field) else {
                continue;
            };
            match name {
                "kind" => kind = Some(parse_symbol(&field.message)),
                "detail" => detail = Some(field.message.clone()),
                "tag" => tags.push(parse_symbol(&field.message)),
                "argument" => arguments.push(parse_symbol(&field.message)),
                "capability" => capabilities.push(CapabilityName::new(field.message.clone())),
                "codec-form" => codec_forms.push(parse_symbol(&field.message)),
                "example" => examples.push(field.message.clone()),
                _ => {}
            }
        }

        Some(Self {
            kind: kind?,
            title: diagnostic.message.clone(),
            detail,
            tags,
            arguments,
            capabilities,
            codec_forms,
            examples,
        })
    }

    /// Returns whether a diagnostic is a hint metadata carrier.
    pub fn is_hint_diagnostic(diagnostic: &Diagnostic) -> bool {
        let expected = hint_metadata_code();
        diagnostic.code.as_ref() == Some(&expected)
    }

    /// Collects the direct hint carriers attached to a diagnostic.
    pub fn collect_from_diagnostic(diagnostic: &Diagnostic) -> Vec<Self> {
        diagnostic
            .related
            .iter()
            .filter_map(Self::from_diagnostic)
            .collect()
    }

    /// Builds a text field suitable for simple agent or Radar-style indexing.
    pub fn radar_text(&self) -> String {
        let mut parts = vec![self.kind.to_string(), self.title.clone()];
        if let Some(detail) = &self.detail {
            parts.push(detail.clone());
        }
        parts.extend(self.tags.iter().map(ToString::to_string));
        parts.extend(self.arguments.iter().map(ToString::to_string));
        parts.extend(
            self.capabilities
                .iter()
                .map(|capability| capability.as_str().to_owned()),
        );
        parts.extend(self.codec_forms.iter().map(ToString::to_string));
        parts.extend(self.examples.iter().cloned());
        parts.join(" ")
    }

    /// Projects the hint as a runtime table value.
    pub fn as_value(&self, cx: &mut Cx) -> Result<Value> {
        let tags = symbol_list_value(cx, &self.tags)?;
        let arguments = symbol_list_value(cx, &self.arguments)?;
        let capabilities = cx.factory().list(
            self.capabilities
                .iter()
                .map(|capability| cx.factory().symbol(capability.as_symbol()))
                .collect::<Result<Vec<_>>>()?,
        )?;
        let codec_forms = symbol_list_value(cx, &self.codec_forms)?;
        let examples = cx.factory().list(
            self.examples
                .iter()
                .map(|example| cx.factory().string(example.clone()))
                .collect::<Result<Vec<_>>>()?,
        )?;
        let detail = match &self.detail {
            Some(detail) => cx.factory().string(detail.clone())?,
            None => cx.factory().nil()?,
        };
        cx.factory().table(vec![
            (Symbol::new("kind"), cx.factory().symbol(self.kind.clone())?),
            (
                Symbol::new("title"),
                cx.factory().string(self.title.clone())?,
            ),
            (Symbol::new("detail"), detail),
            (Symbol::new("tags"), tags),
            (Symbol::new("arguments"), arguments),
            (Symbol::new("capabilities"), capabilities),
            (Symbol::new("codec-forms"), codec_forms),
            (Symbol::new("examples"), examples),
            (
                Symbol::new("radar-text"),
                cx.factory().string(self.radar_text())?,
            ),
        ])
    }

    /// Projects the hint as a datum node.
    pub fn as_datum(&self) -> Datum {
        let mut fields = vec![
            (Symbol::new("kind"), Datum::Symbol(self.kind.clone())),
            (Symbol::new("title"), Datum::String(self.title.clone())),
            (
                Symbol::new("tags"),
                Datum::Vector(self.tags.iter().cloned().map(Datum::Symbol).collect()),
            ),
            (
                Symbol::new("arguments"),
                Datum::Vector(self.arguments.iter().cloned().map(Datum::Symbol).collect()),
            ),
            (
                Symbol::new("capabilities"),
                Datum::Vector(
                    self.capabilities
                        .iter()
                        .map(|capability| Datum::Symbol(capability.as_symbol()))
                        .collect(),
                ),
            ),
            (
                Symbol::new("codec-forms"),
                Datum::Vector(
                    self.codec_forms
                        .iter()
                        .cloned()
                        .map(Datum::Symbol)
                        .collect(),
                ),
            ),
            (
                Symbol::new("examples"),
                Datum::Vector(self.examples.iter().cloned().map(Datum::String).collect()),
            ),
            (Symbol::new("radar-text"), Datum::String(self.radar_text())),
        ];
        if let Some(detail) = &self.detail {
            fields.push((Symbol::new("detail"), Datum::String(detail.clone())));
        }
        Datum::Node {
            tag: Symbol::qualified("core", "HintMetadata"),
            fields,
        }
    }
}

/// Builds a runtime list value containing the hints attached to `diagnostic`.
pub(crate) fn diagnostic_hints_value(cx: &mut Cx, diagnostic: &Diagnostic) -> Result<Value> {
    let values = HintMetadata::collect_from_diagnostic(diagnostic)
        .into_iter()
        .map(|hint| hint.as_value(cx))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values)
}

fn hint_metadata_code() -> Symbol {
    Symbol::qualified("hint", "metadata")
}

fn hint_field(name: &'static str, value: String) -> Diagnostic {
    let mut field = Diagnostic::info(value);
    field.severity = Severity::Note;
    field.code = Some(Symbol::qualified("hint-field", name));
    field
}

fn hint_field_name(diagnostic: &Diagnostic) -> Option<&str> {
    let code = diagnostic.code.as_ref()?;
    if code.namespace.as_deref() != Some("hint-field") {
        return None;
    }
    Some(code.name.as_ref())
}

fn parse_symbol(value: &str) -> Symbol {
    match value.split_once('/') {
        Some((namespace, name)) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(value.to_owned()),
    }
}

fn symbol_list_value(cx: &mut Cx, symbols: &[Symbol]) -> Result<Value> {
    let values = symbols
        .iter()
        .cloned()
        .map(|symbol| cx.factory().symbol(symbol))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values)
}
