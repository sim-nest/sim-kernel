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
fn unqualified_lisp_source_tag_decodes_to_codec_source() {
    // The `lisp-source` wire tag is accepted as a closed spelling for the
    // loadable Lisp codec source.
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
    let decoded = manifest_from_datum(&patched).expect("decode unqualified manifest");
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

#[test]
fn open_lib_source_specs_round_trip_through_the_boot_codec() {
    for source in [
        LibSourceSpec::Open {
            kind: Symbol::new("path"),
            payload: Datum::String("./lib/example.simlib".to_owned()),
        },
        LibSourceSpec::Open {
            kind: Symbol::new("url"),
            payload: Datum::String("https://example.invalid/lib.simlib".to_owned()),
        },
        LibSourceSpec::Open {
            kind: Symbol::new("content-address"),
            payload: Datum::Node {
                tag: Symbol::qualified("content", "address"),
                fields: vec![
                    (
                        Symbol::new("algorithm"),
                        Datum::Symbol(Symbol::qualified("core", "sha256-datum-v1")),
                    ),
                    (Symbol::new("digest"), Datum::Bytes(vec![1, 2, 3, 4])),
                ],
            },
        },
        LibSourceSpec::Open {
            kind: Symbol::qualified("loader", "new-source"),
            payload: Datum::Map(vec![(
                Datum::Symbol(Symbol::new("opaque")),
                Datum::String("payload".to_owned()),
            )]),
        },
    ] {
        let decoded = LibSourceSpec::from_datum(&source.to_datum()).expect("decode source");
        assert_eq!(decoded, source);
    }
}
