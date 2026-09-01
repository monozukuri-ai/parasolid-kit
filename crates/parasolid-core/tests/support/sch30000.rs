//! Development-only serialization and independent encoding of decoded values.
#![allow(dead_code)] // Shared by an integration test and a smaller example harness.

use std::error::Error;

use parasolid_core::{BuiltinSchemaProfile, FieldValue, RawNode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const KEY: &str = "SCH_3000000_30000";
const MODELLER: &str = ": TRANSMIT FILE created by modeller version 3000000";
const NULL_REAL: f64 = -3.14158e13;

/// The hash excludes descriptions, source paths and the hash itself.
pub fn canonical_profile(profile: &BuiltinSchemaProfile) -> Value {
    json!({
        "profile_id": profile.metadata().profile_id,
        "revision": profile.metadata().revision,
        "schema_keys": profile.accepted_schema_keys().map(parasolid_core::SchemaKey::raw).collect::<Vec<_>>(),
        "types": profile.definitions().map(|definition| json!({
            "node_type": definition.node_type,
            "name": definition.name,
            "variable": definition.variable,
            "fields": definition.fields.iter().map(|field| json!([
                field.name, field.field_type.code(), field.pointer_class,
                field.element_count, field.transmitted,
            ])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub fn profile_hash(profile: &BuiltinSchemaProfile) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical_profile(profile))?)
    ))
}

pub fn value_json(value: &FieldValue) -> Value {
    match value {
        FieldValue::UnsignedByte(value) | FieldValue::Character(value) => json!(value),
        FieldValue::Logical(value) => json!(value),
        FieldValue::ShortInteger(value) => json!(value),
        FieldValue::UnicodeCharacter(value) => json!(value),
        FieldValue::Integer(value) | FieldValue::Tag(value) => json!(value),
        FieldValue::PointerIndex(value) => json!(value),
        FieldValue::Double(value) => json!(value),
        FieldValue::Interval(value) => json!(value),
        FieldValue::Vector(value) | FieldValue::IntersectionPoint(value) => json!(value),
        FieldValue::Box3(value) => json!(value),
    }
}

pub fn nodes_json(nodes: &[RawNode]) -> Value {
    json!(nodes.iter().map(|node| json!({
        "node_type": node.node_type, "index": node.index, "variable_length": node.variable_length,
        "range": [node.byte_range.start, node.byte_range.end],
        "fields": node.fields.iter().map(|field| json!({
            "name": field.definition.name, "code": field.definition.field_type.code(),
            "range": [field.byte_range.start, field.byte_range.end],
            "values": field.values.iter().map(value_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })).collect::<Vec<_>>())
}

/// Published compact index rule; no original byte segment is used.
pub fn pointer(value: u32) -> Result<Vec<u8>> {
    if value < 32_767 {
        return Ok(i16::try_from(value + 1)?.to_be_bytes().to_vec());
    }
    let quotient = i16::try_from(value / 32_767)?;
    let remainder = i16::try_from(value % 32_767 + 1)?;
    Ok([(-remainder).to_be_bytes(), quotient.to_be_bytes()].concat())
}

pub fn xb_header(key: &str, user_fields: i32) -> Result<Vec<u8>> {
    let mut output = b"PS\0\0".to_vec();
    output.extend_from_slice(&u16::try_from(MODELLER.len())?.to_be_bytes());
    output.extend_from_slice(MODELLER.as_bytes());
    output.extend_from_slice(&i32::try_from(key.len())?.to_be_bytes());
    output.extend_from_slice(key.as_bytes());
    if key.matches('_').count() == 3 {
        output.extend_from_slice(&205_u16.to_be_bytes());
    }
    output.extend_from_slice(&user_fields.to_be_bytes());
    Ok(output)
}

pub fn xt_header(key: &str, user_fields: i32) -> Vec<u8> {
    format!(
        "T{} {}{} {}{}{user_fields} ",
        MODELLER.len(),
        MODELLER,
        key.len(),
        key,
        if key.matches('_').count() == 3 {
            "205 "
        } else {
            ""
        }
    )
    .into_bytes()
}

pub fn terminate(output: &mut Vec<u8>) {
    output.extend_from_slice(&[0, 1, 0, 1]);
}

pub fn pair(
    node_type: u16,
    length: Option<i32>,
    text: &[u8],
    binary: &[u8],
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut xt = xt_header(KEY, 0);
    xt.extend_from_slice(format!("{node_type} ").as_bytes());
    let mut xb = xb_header(KEY, 0)?;
    xb.extend_from_slice(&node_type.to_be_bytes());
    if let Some(length) = length {
        xt.extend_from_slice(format!("{length} ").as_bytes());
        xb.extend_from_slice(&length.to_be_bytes());
    }
    xt.extend_from_slice(b"1 ");
    xb.extend_from_slice(&pointer(1)?);
    xt.extend_from_slice(text);
    xb.extend_from_slice(binary);
    xt.extend_from_slice(b"1 0");
    terminate(&mut xb);
    Ok((xt, xb))
}

/// Encode only values of the supported profile, not `raw_bytes` or source ranges.
pub fn encode_nodes(nodes: &[RawNode]) -> Result<Vec<u8>> {
    let mut output = xb_header(KEY, 0)?;
    for node in nodes {
        if !node.user_fields.is_empty() {
            return Err("development encoder requires zero user fields".into());
        }
        output.extend_from_slice(&node.node_type.to_be_bytes());
        if let Some(length) = node.variable_length {
            output.extend_from_slice(&i32::try_from(length)?.to_be_bytes());
        }
        output.extend_from_slice(&pointer(node.index)?);
        for field in &node.fields {
            for value in &field.values {
                match value {
                    FieldValue::UnsignedByte(value) | FieldValue::Character(value) => {
                        output.push(*value);
                    }
                    FieldValue::Logical(value) => output.push(u8::from(*value)),
                    FieldValue::UnicodeCharacter(value) => {
                        output.extend_from_slice(&value.to_be_bytes());
                    }
                    FieldValue::Integer(value) => {
                        output.extend_from_slice(&value.unwrap_or(-32_764).to_be_bytes());
                    }
                    FieldValue::PointerIndex(value) => output.extend_from_slice(&pointer(*value)?),
                    FieldValue::Double(value) => {
                        output.extend_from_slice(&value.unwrap_or(NULL_REAL).to_be_bytes());
                    }
                    FieldValue::Vector(values) => {
                        for value in values {
                            output.extend_from_slice(&value.unwrap_or(NULL_REAL).to_be_bytes());
                        }
                    }
                    _ => return Err("field type is outside the development encoder subset".into()),
                }
            }
        }
    }
    terminate(&mut output);
    Ok(output)
}
