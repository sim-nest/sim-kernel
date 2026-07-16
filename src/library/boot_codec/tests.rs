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

#[test]
fn open_export_declaration_round_trips_through_the_manifest_codec() {
    let kind = ExportKind::new(Symbol::qualified("surface", "projection"));
    let export = Export::Open {
        kind: kind.clone(),
        symbol: Symbol::new("graph-view"),
    };
    let mut manifest = sample_manifest(LibTarget::HostRegistered);
    manifest.exports.push(export.clone());

    let decoded = manifest_from_datum(&manifest_datum(&manifest)).expect("decode manifest");

    assert_eq!(decoded.exports, vec![export]);
    assert_eq!(decoded.exports[0].kind_symbol(), kind);
}

#[test]
fn open_export_declaration_rejects_stable_id() {
    let datum = node(
        "export",
        vec![
            (
                "kind",
                Datum::Symbol(Symbol::qualified("surface", "projection")),
            ),
            ("symbol", Datum::Symbol(Symbol::new("graph-view"))),
            ("stable-id", u32_datum(7)),
        ],
    );

    let err = export_from_datum(&datum).unwrap_err();

    assert!(matches!(
        err,
        Error::Lib(message)
            if message == "open export kind surface/projection cannot carry stable-id"
    ));
}
