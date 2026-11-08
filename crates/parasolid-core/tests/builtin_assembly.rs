#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, ComparisonOptions, DocumentLimits, ErrorKind, FieldValue, SchemaKey,
    SchemaProvider, SchemaTypeLookup, brep::map_xb_brep, compare_xt_xb_documents, parse_xb,
    parse_xt,
};

/// Keys under which ASSEMBLY (10), INSTANCE (11) and TRANSFORM (100) are read:
/// the observed `SolidWorks` and NX keys, another build of the `SolidWorks` 2020
/// schema (its family), an iCAD key completed by its family, and the standard
/// 20000 key with another build of it.
const DECLARING_KEYS: [(&str, &str); 7] = [
    ("SCH_3701229_37102_13006", "solidworks-sch37102-13006-r3"),
    (
        "SCH_3101290_31100_13006",
        "solidworks-3101290-31100-13006-r2",
    ),
    ("SCH_2600169_25001_13006", "nx-2600169-25001-13006-r2"),
    ("SCH_3101299_31100_13006", "family-31100-13006"),
    ("SCH_1901315_19008_13006", "icad-1901315-19008-13006-r2"),
    ("SCH_2401000_20000", "icad-2401000-20000-r3"),
    ("SCH_2400000_20000", "family-20000"),
];

#[test]
fn assembly_structure_is_declared_with_the_reference_layouts() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for (key, profile_id) in DECLARING_KEYS {
        let key = SchemaKey::parse(key)?;
        let provider = registry.provider_for_key(&key).ok_or("provider")?;
        assert_eq!(provider.profile().metadata().profile_id, profile_id);
        let schema = key.provider_schema();
        for (kind, fields) in [(10, 17), (11, 10), (100, 9)] {
            let SchemaTypeLookup::Defined(definition) = provider.lookup_type(schema, kind) else {
                return Err(format!("{profile_id} does not define type {kind}").into());
            };
            assert_eq!(definition.fields.len(), fields, "{profile_id} type {kind}");
        }
        // KEY (102) is referenced by no observed file and stays unknown.
        assert_eq!(provider.lookup_type(schema, 102), SchemaTypeLookup::Unknown);
    }
    // A standard schema whose public files carry no assembly keeps the types undeclared.
    let other = registry
        .provider_for_key(&SchemaKey::parse("SCH_1700000_16100")?)
        .ok_or("provider")?;
    assert_eq!(other.lookup_type("16100", 10), SchemaTypeLookup::Unknown);
    Ok(())
}

/// Text and binary encodings of the same three records, authored field by field.
struct Pair {
    embedded: bool,
    text: Vec<u8>,
    binary: Vec<u8>,
}

impl Pair {
    fn new(key: &str) -> support::Result<Self> {
        Ok(Self {
            embedded: key.matches('_').count() == 3,
            text: support::xt_header(key, 0),
            binary: support::xb_header(key, 0)?,
        })
    }

    fn record(&mut self, kind: u16, index: u32) -> support::Result<()> {
        self.text.extend_from_slice(format!("{kind} ").as_bytes());
        self.binary.extend_from_slice(&kind.to_be_bytes());
        if self.embedded {
            // Every type occurs once, so each record carries the unchanged marker.
            self.text.extend_from_slice(b"255 ");
            self.binary.push(0xff);
        }
        self.pointer(index)
    }

    fn pointer(&mut self, value: u32) -> support::Result<()> {
        self.text.extend_from_slice(format!("{value} ").as_bytes());
        self.binary.extend(support::pointer(value)?);
        Ok(())
    }

    fn integer(&mut self, value: i32) {
        self.text.extend_from_slice(format!("{value} ").as_bytes());
        self.binary.extend_from_slice(&value.to_be_bytes());
    }

    fn byte(&mut self, value: u8) {
        self.text.extend_from_slice(format!("{value} ").as_bytes());
        self.binary.push(value);
    }

    fn double(&mut self, value: f64) {
        self.text.extend_from_slice(format!("{value} ").as_bytes());
        self.binary.extend_from_slice(&value.to_be_bytes());
    }

    fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        self.text.extend_from_slice(b"1 0");
        support::terminate(&mut self.binary);
        (self.text, self.binary)
    }
}

/// An assembly (index 1) with one instance (2) of a part at index 4, placed by
/// a translation (3). Parsing resolves no pointers, so the part is absent here.
fn assembly_records(key: &str) -> support::Result<(Vec<u8>, Vec<u8>)> {
    let mut pair = Pair::new(key)?;
    pair.record(10, 1)?;
    pair.integer(3);
    for _ in 0..7 {
        pair.pointer(0)?;
    }
    pair.double(1.0e3);
    pair.double(1.0e-8);
    for _ in 0..3 {
        pair.pointer(0)?;
    }
    pair.byte(1);
    pair.pointer(0)?;
    pair.byte(1);
    pair.pointer(2)?;
    pair.record(11, 2)?;
    pair.integer(1);
    pair.pointer(0)?;
    pair.byte(1);
    pair.pointer(4)?;
    pair.pointer(3)?;
    pair.pointer(1)?;
    for _ in 0..4 {
        pair.pointer(0)?;
    }
    pair.record(100, 3)?;
    pair.integer(2);
    pair.pointer(2)?;
    pair.pointer(0)?;
    pair.pointer(0)?;
    for value in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        pair.double(value);
    }
    for value in [0.5, 0.0, 0.0] {
        pair.double(value);
    }
    pair.double(1.0);
    pair.integer(1);
    for _ in 0..3 {
        pair.double(0.0);
    }
    Ok(pair.finish())
}

#[test]
fn assembly_records_parse_identically_in_text_and_binary() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for key in ["SCH_3701229_37102_13006", "SCH_2401000_20000"] {
        let provider = registry
            .provider_for_key(&SchemaKey::parse(key)?)
            .ok_or("provider")?;
        let (text, binary) = assembly_records(key)?;
        let xt = parse_xt(&text, &provider, DocumentLimits::default())?;
        let xb = parse_xb(&binary, &provider, DocumentLimits::default())?;
        assert_eq!(
            xt.nodes.iter().map(|n| n.node_type).collect::<Vec<_>>(),
            [10, 11, 100]
        );
        assert_eq!(
            xt.nodes.iter().map(|n| n.fields.len()).collect::<Vec<_>>(),
            [17, 10, 9]
        );
        assert!(compare_xt_xb_documents(&xt, &xb, ComparisonOptions::default())?.equivalent);
        assert_eq!(support::encode_document_nodes(key, &xb.nodes)?, binary);
        // The rotation matrix is one nine-element field and the flag an integer.
        let transform = &xb.nodes[2];
        assert_eq!(transform.fields[4].values.len(), 9);
        assert_eq!(transform.fields[7].values, [FieldValue::Integer(Some(1))]);
        for end in 0..binary.len() {
            assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
        }
        // The structure alone is not a B-Rep.
        assert_eq!(
            map_xb_brep(&xb).err().ok_or("body")?.kind(),
            ErrorKind::MissingBrepBody
        );
    }
    Ok(())
}
