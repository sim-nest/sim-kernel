use std::path::PathBuf;

use crate::{
    AbiVersion, CapabilityName, ClassId, CodecId, Datum, Error, Export, ExportKind, ExportRecord,
    ExportState, FunctionId, LibId, LibManifest, LibTarget, MacroId, NumberDomainId, Result,
    RuntimeId, ShapeId, SiteId, Symbol, Version,
};

use super::{
    boot::{LibBootDependency, LibBootReceipt, LibSourceSpec, RegistryBootState},
    loaders::{CatalogSource, LibSource},
};

const BOOT_STATE_FORMAT: &str = "registry-boot-state-v1";

impl RegistryBootState {
    /// Encodes the boot state as canonical SIM data.
    pub fn to_datum(&self) -> Datum {
        node(
            "registry-boot-state",
            vec![
                ("format", Datum::String(BOOT_STATE_FORMAT.to_owned())),
                (
                    "receipts",
                    Datum::List(self.receipts.iter().map(LibBootReceipt::to_datum).collect()),
                ),
            ],
        )
    }

    /// Decodes a boot state from canonical SIM data.
    pub fn from_datum(datum: &Datum) -> Result<Self> {
        let fields = expect_node(datum, "registry-boot-state")?;
        let format = expect_string(required_field(fields, "format")?)?;
        if format != BOOT_STATE_FORMAT {
            return Err(Error::Lib(format!(
                "unsupported registry boot state {format}"
            )));
        }
        let receipts = expect_list(required_field(fields, "receipts")?)?
            .iter()
            .map(LibBootReceipt::from_datum)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { receipts })
    }
}

impl LibBootReceipt {
    /// Encodes the receipt as canonical SIM data.
    pub fn to_datum(&self) -> Datum {
        node(
            "lib-boot-receipt",
            vec![
                ("lib-id", u32_datum(self.lib_id.0)),
                ("requested-source", self.requested_source.to_datum()),
                ("resolved-source", self.resolved_source.to_datum()),
                ("manifest", manifest_datum(&self.manifest)),
                (
                    "dependencies",
                    Datum::List(
                        self.dependencies
                            .iter()
                            .map(LibBootDependency::to_datum)
                            .collect(),
                    ),
                ),
                (
                    "exports",
                    Datum::List(self.exports.iter().map(export_record_datum).collect()),
                ),
            ],
        )
    }

    /// Decodes the receipt from canonical SIM data.
    pub fn from_datum(datum: &Datum) -> Result<Self> {
        let fields = expect_node(datum, "lib-boot-receipt")?;
        Ok(Self {
            lib_id: LibId(expect_u32(required_field(fields, "lib-id")?)?),
            requested_source: LibSourceSpec::from_datum(required_field(
                fields,
                "requested-source",
            )?)?,
            resolved_source: LibSourceSpec::from_datum(required_field(fields, "resolved-source")?)?,
            manifest: manifest_from_datum(required_field(fields, "manifest")?)?,
            dependencies: expect_list(required_field(fields, "dependencies")?)?
                .iter()
                .map(LibBootDependency::from_datum)
                .collect::<Result<Vec<_>>>()?,
            exports: expect_list(required_field(fields, "exports")?)?
                .iter()
                .map(export_record_from_datum)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl LibBootDependency {
    fn to_datum(&self) -> Datum {
        node(
            "lib-boot-dependency",
            vec![
                ("lib-id", u32_datum(self.lib_id.0)),
                ("symbol", Datum::Symbol(self.symbol.clone())),
            ],
        )
    }

    fn from_datum(datum: &Datum) -> Result<Self> {
        let fields = expect_node(datum, "lib-boot-dependency")?;
        Ok(Self {
            lib_id: LibId(expect_u32(required_field(fields, "lib-id")?)?),
            symbol: expect_symbol(required_field(fields, "symbol")?)?.clone(),
        })
    }
}

impl LibSourceSpec {
    /// Encodes the source spec as canonical SIM data.
    pub fn to_datum(&self) -> Datum {
        match self {
            Self::Symbol(symbol) => source_node("symbol", Datum::Symbol(symbol.clone())),
            Self::Path(path) => {
                source_node("path", Datum::String(path.to_string_lossy().into_owned()))
            }
            Self::Url(url) => source_node("url", Datum::String(url.clone())),
            Self::Bytes(bytes) => source_node("bytes", Datum::Bytes(bytes.clone())),
        }
    }

    /// Decodes the source spec from canonical SIM data.
    pub fn from_datum(datum: &Datum) -> Result<Self> {
        let fields = expect_node(datum, "lib-source")?;
        let kind = expect_symbol(required_field(fields, "kind")?)?;
        let value = required_field(fields, "value")?;
        match kind.name.as_ref() {
            "symbol" if kind.namespace.is_none() => Ok(Self::Symbol(expect_symbol(value)?.clone())),
            "path" if kind.namespace.is_none() => {
                Ok(Self::Path(PathBuf::from(expect_string(value)?)))
            }
            "url" if kind.namespace.is_none() => Ok(Self::Url(expect_string(value)?.to_owned())),
            "bytes" if kind.namespace.is_none() => Ok(Self::Bytes(expect_bytes(value)?.to_vec())),
            _ => Err(Error::Lib(format!("unknown lib source kind {kind}"))),
        }
    }
}

impl From<CatalogSource> for LibSourceSpec {
    fn from(source: CatalogSource) -> Self {
        match source {
            CatalogSource::Path(path) => Self::Path(path),
            CatalogSource::Url(url) => Self::Url(url),
            CatalogSource::Bytes(bytes) => Self::Bytes(bytes),
        }
    }
}

impl From<LibSourceSpec> for LibSource {
    fn from(source: LibSourceSpec) -> Self {
        match source {
            LibSourceSpec::Symbol(symbol) => Self::Symbol(symbol),
            LibSourceSpec::Path(path) => Self::Path(path),
            LibSourceSpec::Url(url) => Self::Url(url),
            LibSourceSpec::Bytes(bytes) => Self::Bytes(bytes),
        }
    }
}

impl TryFrom<LibSource> for LibSourceSpec {
    type Error = Error;

    fn try_from(source: LibSource) -> Result<Self> {
        match source {
            LibSource::Symbol(symbol) => Ok(Self::Symbol(symbol)),
            LibSource::Path(path) => Ok(Self::Path(path)),
            LibSource::Url(url) => Ok(Self::Url(url)),
            LibSource::Bytes(bytes) => Ok(Self::Bytes(bytes)),
            LibSource::Host(_) => Err(Error::Lib(
                "host lib sources are live values, not boot data".to_owned(),
            )),
        }
    }
}

fn manifest_datum(manifest: &LibManifest) -> Datum {
    node(
        "lib-manifest",
        vec![
            ("id", Datum::Symbol(manifest.id.clone())),
            ("version", Datum::String(manifest.version.0.clone())),
            ("abi-major", u16_datum(manifest.abi.major)),
            ("abi-minor", u16_datum(manifest.abi.minor)),
            ("target", Datum::Symbol(manifest.target.to_symbol())),
            (
                "requires",
                Datum::List(manifest.requires.iter().map(dependency_datum).collect()),
            ),
            (
                "capabilities",
                Datum::List(
                    manifest
                        .capabilities
                        .iter()
                        .map(|capability| Datum::String(capability.as_str().to_owned()))
                        .collect(),
                ),
            ),
            (
                "exports",
                Datum::List(manifest.exports.iter().map(export_datum).collect()),
            ),
        ],
    )
}

fn manifest_from_datum(datum: &Datum) -> Result<LibManifest> {
    let fields = expect_node(datum, "lib-manifest")?;
    Ok(LibManifest {
        id: expect_symbol(required_field(fields, "id")?)?.clone(),
        version: Version(expect_string(required_field(fields, "version")?)?.to_owned()),
        abi: AbiVersion {
            major: expect_u16(required_field(fields, "abi-major")?)?,
            minor: expect_u16(required_field(fields, "abi-minor")?)?,
        },
        target: LibTarget::from_symbol(expect_symbol(required_field(fields, "target")?)?),
        requires: expect_list(required_field(fields, "requires")?)?
            .iter()
            .map(dependency_from_datum)
            .collect::<Result<Vec<_>>>()?,
        capabilities: expect_list(required_field(fields, "capabilities")?)?
            .iter()
            .map(|datum| Ok(CapabilityName::new(expect_string(datum)?.to_owned())))
            .collect::<Result<Vec<_>>>()?,
        exports: expect_list(required_field(fields, "exports")?)?
            .iter()
            .map(export_from_datum)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn dependency_datum(dependency: &crate::Dependency) -> Datum {
    node(
        "dependency",
        vec![
            ("id", Datum::Symbol(dependency.id.clone())),
            (
                "minimum-version",
                dependency
                    .minimum_version
                    .as_ref()
                    .map(|version| Datum::String(version.0.clone()))
                    .unwrap_or(Datum::Nil),
            ),
        ],
    )
}

fn dependency_from_datum(datum: &Datum) -> Result<crate::Dependency> {
    let fields = expect_node(datum, "dependency")?;
    let minimum_version = match required_field(fields, "minimum-version")? {
        Datum::Nil => None,
        other => Some(Version(expect_string(other)?.to_owned())),
    };
    Ok(crate::Dependency {
        id: expect_symbol(required_field(fields, "id")?)?.clone(),
        minimum_version,
    })
}

fn export_datum(export: &Export) -> Datum {
    let (kind, symbol, stable_id) = match export {
        Export::Class { symbol, class_id } => ("class", symbol, class_id.map(|id| id.0)),
        Export::Function {
            symbol,
            function_id,
        } => ("function", symbol, function_id.map(|id| id.0)),
        Export::Macro { symbol, macro_id } => ("macro", symbol, macro_id.map(|id| id.0)),
        Export::Shape { symbol, shape_id } => ("shape", symbol, shape_id.map(|id| id.0)),
        Export::Codec { symbol, codec_id } => ("codec", symbol, codec_id.map(|id| id.0)),
        Export::NumberDomain {
            symbol,
            number_domain_id,
        } => ("number-domain", symbol, number_domain_id.map(|id| id.0)),
        Export::Value { symbol } => ("value", symbol, None),
        Export::Site { symbol, runtime_id } => {
            let stable_id = match runtime_id {
                Some(RuntimeId::Site(id)) => Some(id.0),
                _ => None,
            };
            ("site", symbol, stable_id)
        }
    };
    node(
        "export",
        vec![
            ("kind", Datum::Symbol(Symbol::new(kind))),
            ("symbol", Datum::Symbol(symbol.clone())),
            ("stable-id", stable_id.map(u32_datum).unwrap_or(Datum::Nil)),
        ],
    )
}

fn export_from_datum(datum: &Datum) -> Result<Export> {
    let fields = expect_node(datum, "export")?;
    let kind = expect_symbol(required_field(fields, "kind")?)?;
    let symbol = expect_symbol(required_field(fields, "symbol")?)?.clone();
    let stable_id = optional_u32(required_field(fields, "stable-id")?)?;
    match kind.name.as_ref() {
        "class" if kind.namespace.is_none() => Ok(Export::Class {
            symbol,
            class_id: stable_id.map(ClassId),
        }),
        "function" if kind.namespace.is_none() => Ok(Export::Function {
            symbol,
            function_id: stable_id.map(FunctionId),
        }),
        "macro" if kind.namespace.is_none() => Ok(Export::Macro {
            symbol,
            macro_id: stable_id.map(MacroId),
        }),
        "shape" if kind.namespace.is_none() => Ok(Export::Shape {
            symbol,
            shape_id: stable_id.map(ShapeId),
        }),
        "codec" if kind.namespace.is_none() => Ok(Export::Codec {
            symbol,
            codec_id: stable_id.map(CodecId),
        }),
        "number-domain" if kind.namespace.is_none() => Ok(Export::NumberDomain {
            symbol,
            number_domain_id: stable_id.map(NumberDomainId),
        }),
        "value" if kind.namespace.is_none() => Ok(Export::Value { symbol }),
        "site" if kind.namespace.is_none() => Ok(Export::Site {
            symbol,
            runtime_id: stable_id.map(|id| RuntimeId::Site(SiteId(id))),
        }),
        _ => Err(Error::Lib(format!("unknown export kind {kind}"))),
    }
}

fn export_record_datum(record: &ExportRecord) -> Datum {
    node(
        "export-record",
        vec![
            ("kind", Datum::Symbol(record.kind.symbol().clone())),
            ("symbol", Datum::Symbol(record.symbol.clone())),
            ("state", export_state_datum(&record.state)),
        ],
    )
}

fn export_record_from_datum(datum: &Datum) -> Result<ExportRecord> {
    let fields = expect_node(datum, "export-record")?;
    Ok(ExportRecord {
        kind: ExportKind::new(expect_symbol(required_field(fields, "kind")?)?.clone()),
        symbol: expect_symbol(required_field(fields, "symbol")?)?.clone(),
        state: export_state_from_datum(required_field(fields, "state")?)?,
    })
}

fn export_state_datum(state: &ExportState) -> Datum {
    match state {
        ExportState::Resolved { id } => node(
            "export-state",
            vec![
                ("kind", Datum::Symbol(Symbol::new("resolved"))),
                ("runtime-id", runtime_id_datum(*id)),
            ],
        ),
        ExportState::Declared => state_node("declared", Datum::Nil),
        ExportState::Unsupported { reason } => {
            state_node("unsupported", Datum::String(reason.clone()))
        }
        ExportState::Invalid { error } => state_node("invalid", Datum::String(error.clone())),
    }
}

fn export_state_from_datum(datum: &Datum) -> Result<ExportState> {
    let fields = expect_node(datum, "export-state")?;
    let kind = expect_symbol(required_field(fields, "kind")?)?;
    let value = fields
        .iter()
        .find_map(|(field, value)| (field.name.as_ref() == "value").then_some(value));
    match kind.name.as_ref() {
        "resolved" if kind.namespace.is_none() => Ok(ExportState::Resolved {
            id: runtime_id_from_datum(required_field(fields, "runtime-id")?)?,
        }),
        "declared" if kind.namespace.is_none() => Ok(ExportState::Declared),
        "unsupported" if kind.namespace.is_none() => Ok(ExportState::Unsupported {
            reason: expect_string(value.unwrap_or(&Datum::Nil))?.to_owned(),
        }),
        "invalid" if kind.namespace.is_none() => Ok(ExportState::Invalid {
            error: expect_string(value.unwrap_or(&Datum::Nil))?.to_owned(),
        }),
        _ => Err(Error::Lib(format!("unknown export state {kind}"))),
    }
}

fn runtime_id_datum(id: RuntimeId) -> Datum {
    let (kind, value) = match id {
        RuntimeId::Class(id) => ("class", Some(id.0)),
        RuntimeId::Function(id) => ("function", Some(id.0)),
        RuntimeId::Macro(id) => ("macro", Some(id.0)),
        RuntimeId::Shape(id) => ("shape", Some(id.0)),
        RuntimeId::Codec(id) => ("codec", Some(id.0)),
        RuntimeId::NumberDomain(id) => ("number-domain", Some(id.0)),
        RuntimeId::Site(id) => ("site", Some(id.0)),
        RuntimeId::Value => ("value", None),
    };
    node(
        "runtime-id",
        vec![
            ("kind", Datum::Symbol(Symbol::new(kind))),
            ("value", value.map(u32_datum).unwrap_or(Datum::Nil)),
        ],
    )
}

fn runtime_id_from_datum(datum: &Datum) -> Result<RuntimeId> {
    let fields = expect_node(datum, "runtime-id")?;
    let kind = expect_symbol(required_field(fields, "kind")?)?;
    let value = optional_u32(required_field(fields, "value")?)?;
    match kind.name.as_ref() {
        "class" if kind.namespace.is_none() => {
            Ok(RuntimeId::Class(ClassId(required_id(value, kind)?)))
        }
        "function" if kind.namespace.is_none() => {
            Ok(RuntimeId::Function(FunctionId(required_id(value, kind)?)))
        }
        "macro" if kind.namespace.is_none() => {
            Ok(RuntimeId::Macro(MacroId(required_id(value, kind)?)))
        }
        "shape" if kind.namespace.is_none() => {
            Ok(RuntimeId::Shape(ShapeId(required_id(value, kind)?)))
        }
        "codec" if kind.namespace.is_none() => {
            Ok(RuntimeId::Codec(CodecId(required_id(value, kind)?)))
        }
        "number-domain" if kind.namespace.is_none() => Ok(RuntimeId::NumberDomain(NumberDomainId(
            required_id(value, kind)?,
        ))),
        "site" if kind.namespace.is_none() => {
            Ok(RuntimeId::Site(SiteId(required_id(value, kind)?)))
        }
        "value" if kind.namespace.is_none() => Ok(RuntimeId::Value),
        _ => Err(Error::Lib(format!("unknown runtime id kind {kind}"))),
    }
}

fn required_id(value: Option<u32>, kind: &Symbol) -> Result<u32> {
    value.ok_or_else(|| Error::Lib(format!("runtime id kind {kind} requires an id")))
}

fn source_node(kind: &'static str, value: Datum) -> Datum {
    node(
        "lib-source",
        vec![("kind", Datum::Symbol(Symbol::new(kind))), ("value", value)],
    )
}

fn state_node(kind: &'static str, value: Datum) -> Datum {
    node(
        "export-state",
        vec![("kind", Datum::Symbol(Symbol::new(kind))), ("value", value)],
    )
}

fn node(tag: &'static str, fields: Vec<(&'static str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("library", tag),
        fields: fields
            .into_iter()
            .map(|(field, value)| (Symbol::new(field), value))
            .collect(),
    }
}

fn expect_node<'a>(datum: &'a Datum, tag: &'static str) -> Result<&'a [(Symbol, Datum)]> {
    let Datum::Node {
        tag: actual,
        fields,
    } = datum
    else {
        return Err(Error::TypeMismatch {
            expected: "datum node",
            found: datum_kind(datum),
        });
    };
    let expected = Symbol::qualified("library", tag);
    if actual != &expected {
        return Err(Error::Lib(format!(
            "expected datum node {expected}, found {actual}"
        )));
    }
    Ok(fields)
}

fn required_field<'a>(fields: &'a [(Symbol, Datum)], name: &'static str) -> Result<&'a Datum> {
    fields
        .iter()
        .find_map(|(field, value)| {
            (field.namespace.is_none() && field.name.as_ref() == name).then_some(value)
        })
        .ok_or_else(|| Error::Lib(format!("missing boot datum field {name}")))
}

fn expect_list(datum: &Datum) -> Result<&[Datum]> {
    match datum {
        Datum::List(items) => Ok(items),
        other => Err(Error::TypeMismatch {
            expected: "datum list",
            found: datum_kind(other),
        }),
    }
}

fn expect_symbol(datum: &Datum) -> Result<&Symbol> {
    match datum {
        Datum::Symbol(symbol) => Ok(symbol),
        other => Err(Error::TypeMismatch {
            expected: "datum symbol",
            found: datum_kind(other),
        }),
    }
}

fn expect_string(datum: &Datum) -> Result<&str> {
    match datum {
        Datum::String(value) => Ok(value),
        other => Err(Error::TypeMismatch {
            expected: "datum string",
            found: datum_kind(other),
        }),
    }
}

fn expect_bytes(datum: &Datum) -> Result<&[u8]> {
    match datum {
        Datum::Bytes(value) => Ok(value),
        other => Err(Error::TypeMismatch {
            expected: "datum bytes",
            found: datum_kind(other),
        }),
    }
}

fn u16_datum(value: u16) -> Datum {
    Datum::String(value.to_string())
}

fn u32_datum(value: u32) -> Datum {
    Datum::String(value.to_string())
}

fn expect_u16(datum: &Datum) -> Result<u16> {
    expect_string(datum)?
        .parse::<u16>()
        .map_err(|err| Error::Lib(format!("invalid u16 boot datum: {err}")))
}

fn expect_u32(datum: &Datum) -> Result<u32> {
    expect_string(datum)?
        .parse::<u32>()
        .map_err(|err| Error::Lib(format!("invalid u32 boot datum: {err}")))
}

fn optional_u32(datum: &Datum) -> Result<Option<u32>> {
    match datum {
        Datum::Nil => Ok(None),
        other => expect_u32(other).map(Some),
    }
}

fn datum_kind(datum: &Datum) -> &'static str {
    match datum {
        Datum::Nil => "datum nil",
        Datum::Bool(_) => "datum bool",
        Datum::Number(_) => "datum number",
        Datum::Symbol(_) => "datum symbol",
        Datum::String(_) => "datum string",
        Datum::Bytes(_) => "datum bytes",
        Datum::List(_) => "datum list",
        Datum::Vector(_) => "datum vector",
        Datum::Map(_) => "datum map",
        Datum::Set(_) => "datum set",
        Datum::Node { .. } => "datum node",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(target: LibTarget) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("demo", "lib"),
            version: Version("1.0.0".to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: Vec::new(),
        }
    }

    #[test]
    fn codec_source_target_round_trips_through_the_manifest_codec() {
        let target = LibTarget::CodecSource(Symbol::qualified("codec", "lisp"));
        let manifest = sample_manifest(target.clone());
        let decoded = manifest_from_datum(&manifest_datum(&manifest)).expect("decode manifest");
        assert_eq!(decoded.target, target);
    }

    #[test]
    fn legacy_lisp_source_tag_still_decodes_to_codec_source() {
        // A pre-CodecSource manifest serialized the lisp codec as the closed
        // symbol `lisp-source`; it must still load.
        let datum = manifest_datum(&sample_manifest(LibTarget::HostRegistered));
        let Datum::Node { tag, fields } = datum else {
            panic!("manifest datum is a node");
        };
        let patched = Datum::Node {
            tag,
            fields: fields
                .into_iter()
                .map(|(field, value)| {
                    if field.name.as_ref() == "target" {
                        (field, Datum::Symbol(Symbol::new("lisp-source")))
                    } else {
                        (field, value)
                    }
                })
                .collect(),
        };
        let decoded = manifest_from_datum(&patched).expect("decode legacy manifest");
        assert_eq!(
            decoded.target,
            LibTarget::CodecSource(Symbol::qualified("codec", "lisp"))
        );
    }

    #[test]
    fn closed_targets_round_trip() {
        for target in [
            LibTarget::Native,
            LibTarget::WasmComponent,
            LibTarget::DataOnly,
            LibTarget::HostRegistered,
        ] {
            let manifest = sample_manifest(target.clone());
            let decoded = manifest_from_datum(&manifest_datum(&manifest)).expect("decode manifest");
            assert_eq!(decoded.target, target);
        }
    }
}
