//! Local evidence harness for extracted `SolidWorks` Parasolid streams.
//!
//! Run with `PARASOLID_SOLIDWORKS_STREAMS=<dir>` and optionally
//! `PARASOLID_SOLIDWORKS_EVIDENCE=<output.jsonl>`:
//! `cargo test -p parasolid-core --test solidworks_partition_evidence -- --ignored`.
//! The directory holds extracted `.xb` streams; no CAD data is committed.

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, InspectionLimits, SchemaKey, brep::map_xb_brep,
    inspect_xb, parse_xb,
};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn streams(root: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            streams(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "xb") {
            found.push(path);
        }
    }
    Ok(())
}

fn evidence(registry: &BuiltinProfileRegistry, data: &[u8]) -> Value {
    let header = match inspect_xb(data, InspectionLimits::default()) {
        Ok(header) => header,
        Err(error) => {
            return json!({"inspect_error": error.kind().code(), "message": error.message()});
        }
    };
    let Ok(key) = SchemaKey::parse(&header.schema_key) else {
        return json!({"key": header.schema_key, "key_error": true});
    };
    let Some(provider) = registry.provider_for_key(&key) else {
        return json!({"key": header.schema_key, "registered": false});
    };
    let profile = provider.profile().metadata().profile_id.clone();
    let document = match parse_xb(data, &provider, DocumentLimits::default()) {
        Ok(document) => document,
        Err(error) => {
            return json!({
                "key": header.schema_key, "profile": profile, "parsed": false,
                "error": error.kind().code(), "offset": error.offset(),
                "message": error.message(), "details": format!("{:?}", error.details()),
            });
        }
    };
    let mut node_types = BTreeMap::<u16, usize>::new();
    for node in &document.nodes {
        *node_types.entry(node.node_type).or_default() += 1;
    }
    let mut result = json!({
        "key": header.schema_key, "profile": profile, "parsed": true,
        "nodes": document.nodes.len(), "node_types": node_types,
    });
    match map_xb_brep(&document) {
        Ok(brep) => {
            let mut codes = BTreeMap::<&str, usize>::new();
            for diagnostic in &brep.diagnostics {
                *codes.entry(diagnostic.code).or_default() += 1;
            }
            result["complete"] = json!(brep.complete);
            result["topology_valid"] = json!(brep.topology.valid);
            result["bodies"] = json!(brep.bodies.len());
            result["faces"] = json!(brep.faces.len());
            result["edges"] = json!(brep.edges.len());
            result["vertices"] = json!(brep.vertices.len());
            result["diagnostics"] = json!(codes);
            if std::env::var_os("PARASOLID_SOLIDWORKS_POINTS").is_some() {
                result["points"] = json!(
                    brep.vertices
                        .iter()
                        .filter_map(|vertex| brep
                            .points
                            .iter()
                            .find(|point| point.id == vertex.point))
                        .take(20_000)
                        .map(|point| json!([point.position.x, point.position.y, point.position.z]))
                        .collect::<Vec<_>>()
                );
            }
            result["bbox"] = brep
                .metrics
                .bounding_box
                .as_ref()
                .map_or(Value::Null, |bbox| {
                    let point = |v: &parasolid_core::brep::Vector3| json!([v.x, v.y, v.z]);
                    json!([point(&bbox.minimum), point(&bbox.maximum)])
                });
        }
        Err(error) => {
            result["map_error"] = json!(error.kind().code());
            result["map_message"] = json!(error.message());
        }
    }
    result
}

#[test]
#[ignore = "requires PARASOLID_SOLIDWORKS_STREAMS"]
fn solidworks_partition_evidence() -> Result<()> {
    let Some(root) = std::env::var_os("PARASOLID_SOLIDWORKS_STREAMS") else {
        return Ok(());
    };
    let registry = BuiltinProfileRegistry::compiled()?;
    let mut paths = Vec::new();
    streams(Path::new(&root), &mut paths)?;
    paths.sort();
    let mut output: Box<dyn Write> = match std::env::var_os("PARASOLID_SOLIDWORKS_EVIDENCE") {
        Some(path) => Box::new(fs::File::create(path)?),
        None => Box::new(std::io::stdout()),
    };
    for path in paths {
        let mut record = evidence(registry, &fs::read(&path)?);
        record["stream"] = json!(path.strip_prefix(&root)?.to_string_lossy());
        writeln!(output, "{record}")?;
    }
    Ok(())
}
