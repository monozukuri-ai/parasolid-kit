//! B-Rep roles reviewed against the M8.1 definitions and M8.3 geometry evidence.
//! Ordinals belong to this exact profile revision; raw names remain unchanged.

use crate::{
    BuiltinProfileCoverage, ErrorDetails, ErrorKind, ParseError, RawField, RawNode, SchemaEdit,
    SchemaProviderResolution, SchemaResolution, SchemaSource,
};

// Independently pin the definition revision reviewed with these semantic roles.
const ROLE_PROFILE_SHA256: &str =
    "67e0f3f90c9025c16269c0b03d2365834d949797e1f4c0eb4255b153960b7bf4";
const ROLE_BASE_SHA256: &str = "2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb";
const ROLE_EMBEDDED_SHA256: &str =
    "29d58f5c4aba74913689b9034b0532a17601bb660fafc01d21692ef5330f4f8e";

#[derive(Clone, Copy)]
pub(super) enum RoleAccess {
    Named,
    OnshapeSch30000,
    Sch13006,
    TopsolidCatalog,
    Sch3600188,
}

impl RoleAccess {
    #[allow(clippy::too_many_lines)]
    pub(super) fn select(
        provenance: &SchemaProviderResolution,
        key: &str,
        schemas: &[SchemaResolution],
    ) -> Result<Self, ParseError> {
        match provenance {
            SchemaProviderResolution::CallerSupplied => Ok(Self::Named),
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage,
                profile_sha256,
            } if key == schema_key
                && *profile_revision == 1
                && *coverage == BuiltinProfileCoverage::VerifiedSubset
                && matches!(
                    (
                        profile_id.as_str(),
                        schema_key.as_str(),
                        profile_sha256.as_str()
                    ),
                    (
                        "topsolid-v6-psolid-13006-r1",
                        "SCH_3600188_36001_13006",
                        "0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3"
                    ) | (
                        "topsolid-v6-psolid-36001-r1",
                        "SCH_3700192_36001_13006",
                        "42ec5f0bda547618007f3ebeac1479b73d8444084d24ac228e099eb9c5b67430"
                    ) | (
                        "topsolid-v6-psolid-32001-r1",
                        "SCH_3300259_32001_13006",
                        "5ffe6a8942c686e400b3cad1edc19fbe39457589079769d9f7dbcafec535a159"
                    ) | (
                        "topsolid-v6-psolid-32001-r1",
                        "SCH_3300261_32001_13006",
                        "5ffe6a8942c686e400b3cad1edc19fbe39457589079769d9f7dbcafec535a159"
                    ) | (
                        "topsolid-v6-psolid-26105-r1",
                        "SCH_2701192_26105_13006",
                        "c21517ad14061ff14e2636bb90842ba74b3ad4980b7d041abd78732d80249e95"
                    )
                ) =>
            {
                let registry = crate::BuiltinProfileRegistry::compiled()?;
                let exact_key = crate::SchemaKey::parse(schema_key)?;
                let profile = registry
                    .profile_for_key(&exact_key)
                    .ok_or_else(|| unreviewed(provenance))?;
                let base = profile.definitions().collect::<Vec<_>>();
                validate_topsolid_base_roles(schemas, &base)?;
                Ok(if key == "SCH_3600188_36001_13006" {
                    Self::Sch3600188
                } else {
                    Self::TopsolidCatalog
                })
            }
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage,
                profile_sha256,
            } if profile_id == "onshape-sch30000-r3"
                && *profile_revision == 3
                && schema_key == "SCH_3000000_30000"
                && key == schema_key
                && *coverage == BuiltinProfileCoverage::VerifiedSubset
                && profile_sha256 == ROLE_PROFILE_SHA256 =>
            {
                Ok(Self::OnshapeSch30000)
            }
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage,
                profile_sha256,
            } if schema_key == key
                && *coverage == BuiltinProfileCoverage::VerifiedSubset
                && ((profile_id == "onshape-sch13006-r6"
                    && *profile_revision == 6
                    && key == "SCH_1300000_13006"
                    && profile_sha256 == ROLE_BASE_SHA256)
                    || (profile_id == "icad-sch30000-13006-r6"
                        && *profile_revision == 6
                        && key == "SCH_3000310_30000_13006"
                        && profile_sha256 == ROLE_EMBEDDED_SHA256)
                    || (profile_id == "icad-sch34101-13006-r3"
                        && *profile_revision == 3
                        && key == "SCH_3401212_34101_13006"
                        && profile_sha256 == crate::schema::profiles::ICAD_V34_PROFILE_SHA256)
                    || (profile_id == "solidworks-sch37102-13006-r3"
                        && *profile_revision == 3
                        && key == "SCH_3701229_37102_13006"
                        && profile_sha256
                            == crate::schema::profiles::SOLIDWORKS_PROFILE_SHA256)
                    || (profile_id == "onshape-sch37102-13006-r3"
                        && *profile_revision == 3
                        && key == "SCH_3701212_37102_13006"
                        && profile_sha256
                            == crate::schema::profiles::ONSHAPE_CURRENT_PROFILE_SHA256)
                    || crate::schema::profiles::icad_legacy::PROFILES
                        .iter()
                        .any(|spec| {
                            key == spec.key
                                && profile_id == spec.id
                                && *profile_revision == spec.revision
                                && profile_sha256 == spec.sha256
                        })
                    || crate::schema::profiles::solidworks::PROFILES
                        .iter()
                        .any(|spec| {
                            key == spec.key
                                && profile_id == spec.id
                                && *profile_revision == spec.revision
                                && profile_sha256 == spec.sha256
                        })
                    || crate::schema::profiles::transmit::PROFILES
                        .iter()
                        .any(|spec| {
                            key == spec.key
                                && profile_id == spec.id
                                && *profile_revision == spec.revision
                                && profile_sha256 == spec.sha256
                        })) =>
            {
                // The exact profile is completed by its schema family, so the
                // reviewed layouts are the family's when the registry has one.
                let reviewed =
                    family_definitions(key).unwrap_or_else(|| reviewed_base_definitions(key));
                validate_base_roles(schemas, &reviewed)?;
                Ok(Self::Sch13006)
            }
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage,
                profile_sha256,
            } if schema_key == key && *coverage == BuiltinProfileCoverage::VerifiedSubset => {
                // Standard iCAD layouts keep the reviewed 13006 field names.
                let own = standard_definitions(key, profile_id, *profile_revision, profile_sha256)
                    .ok_or_else(|| unreviewed(provenance))??;
                let reviewed = family_definitions(key).unwrap_or(own);
                validate_standard_roles(schemas, &reviewed)?;
                Ok(Self::Sch13006)
            }
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage: BuiltinProfileCoverage::FamilyInferred,
                profile_sha256,
            } if schema_key == key => {
                // A family profile merges the reviewed declarations of exact
                // profiles whose digests pin the roles below; the registry
                // recomputes the family digest from those members.
                let requested = crate::SchemaKey::parse(key)?;
                let family = crate::BuiltinProfileRegistry::compiled()?
                    .profile(profile_id)
                    .filter(|family| {
                        let metadata = family.metadata();
                        metadata.coverage == BuiltinProfileCoverage::FamilyInferred
                            && metadata.revision == *profile_revision
                            && metadata.profile_sha256 == *profile_sha256
                            && family.accepts_schema_key(&requested)
                    })
                    .ok_or_else(|| unreviewed(provenance))?;
                if family
                    .members()
                    .iter()
                    .any(|member| member.profile_id == "onshape-sch30000-r3")
                {
                    // V30 roles are wire ordinals of that single reviewed layout set.
                    if family.members().len() != 1 {
                        return Err(unreviewed(provenance));
                    }
                    return Ok(Self::OnshapeSch30000);
                }
                let reviewed = family.definitions().cloned().collect::<Vec<_>>();
                if requested.base().is_some() {
                    validate_base_roles(schemas, &reviewed)?;
                } else {
                    validate_standard_roles(schemas, &reviewed)?;
                }
                Ok(Self::Sch13006)
            }
            SchemaProviderResolution::Builtin { .. } => Err(unreviewed(provenance)),
        }
    }

    pub(super) fn type_name(self, node: &RawNode) -> &str {
        match self {
            Self::Named => &node.definition.name,
            Self::TopsolidCatalog
                if matches!(node.node_type, 118 | 201) && node.definition.name == "MESH" =>
            {
                "MESH"
            }
            Self::TopsolidCatalog if node.definition.name == "POLYLINE" => "POLYLINE",
            Self::OnshapeSch30000 | Self::Sch13006 | Self::TopsolidCatalog | Self::Sch3600188 => {
                match node.node_type {
                    10 => "ASSEMBLY",
                    11 => "INSTANCE",
                    12 => "BODY",
                    13 => "SHELL",
                    14 => "FACE",
                    15 => "LOOP",
                    16 => "EDGE",
                    17 => "HALFEDGE",
                    18 => "VERTEX",
                    19 => "REGION",
                    29 => "POINT",
                    30 => "LINE",
                    31 => "CIRCLE",
                    32 => "ELLIPSE",
                    38 => "INTERSECTION",
                    40 => "CHART",
                    41 => "LIMIT",
                    45 => "BSPLINE_VERTICES",
                    124 => "B_SURFACE",
                    126 => "NURBS_SURF",
                    127 => "KNOT_MULT",
                    128 => "KNOT_SET",
                    134 => "B_CURVE",
                    136 => "NURBS_CURVE",
                    137 => "SP_CURVE",
                    50 => "PLANE",
                    51 => "CYLINDER",
                    52 => "CONE",
                    53 => "SPHERE",
                    54 => "TORUS",
                    56 => "BLENDED_EDGE",
                    59 => "BLEND_BOUND",
                    60 => "OFFSET_SURF",
                    67 if matches!(self, Self::OnshapeSch30000 | Self::Sch13006) => "SWEPT_SURF",
                    68 => "SPUN_SURF",
                    133 => "TRIMMED_CURVE",
                    100 => "TRANSFORM",
                    204 => "INTERSECTION_DATA",
                    79 => "ATT_DEF_ID",
                    80 => "ATTRIB_DEF",
                    81 => "ATTRIBUTE",
                    82 => "INT_VALUES",
                    83 => "REAL_VALUES",
                    84 => "CHAR_VALUES",
                    85 => "POINT_VALUES",
                    86 => "VECTOR_VALUES",
                    87 => "AXIS_VALUES",
                    88 => "TAG_VALUES",
                    89 => "DIRECTION_VALUES",
                    90 if matches!(self, Self::TopsolidCatalog) => "GROUP",
                    91 if matches!(self, Self::TopsolidCatalog) => "MEMBER_OF_GROUP",
                    98 => "UNICODE_VALUES",
                    _ => "", // List types cannot acquire roles from their names.
                }
            }
        }
    }

    pub(super) fn field<'a>(self, node: &'a RawNode, role: &str) -> Option<&'a RawField> {
        match self {
            Self::Named | Self::TopsolidCatalog | Self::Sch3600188 => {
                node.fields.iter().find(|f| f.definition.name == role)
            }
            Self::OnshapeSch30000 if node.node_type == 38 && role == "intersection_data" => {
                node.fields.get(11)
            }
            Self::OnshapeSch30000 if node.node_type == 41 && role == "hvec" => node.fields.get(2),
            Self::OnshapeSch30000 => field_roles(node.node_type)
                .iter()
                .find(|(name, _, _, _)| *name == role)
                .and_then(|(_, ordinal, _, _)| node.fields.get(*ordinal)),
            // This optional source reference has a separately reviewed Append
            // contract; validate_base_roles checks its codec, class and origin.
            Self::Sch13006 if node.node_type == 38 && role == "intersection_data" => {
                node.fields.iter().find(|f| f.definition.name == role)
            }
            // Only copied/unchanged base fields acquire these identities; the
            // resolution check below prevents inserted names from claiming roles.
            Self::Sch13006 => field_roles(node.node_type)
                .iter()
                .find(|(name, _, _, _)| *name == role)
                .and_then(|(_, _, name, _)| {
                    node.fields.iter().find(|f| f.definition.name == *name)
                }),
        }
    }

    pub(super) fn is_curve(self, node: &RawNode) -> bool {
        match self {
            Self::OnshapeSch30000 => matches!(node.node_type, 30..=32 | 38 | 133 | 134 | 137),
            Self::TopsolidCatalog => {
                node.definition.name == "POLYLINE"
                    || matches!(node.node_type, 30..=32 | 38 | 133 | 134 | 137)
            }
            Self::Sch13006 | Self::Sch3600188 => {
                matches!(node.node_type, 30..=32 | 38 | 133 | 134 | 137)
            }
            Self::Named => {
                self.has_common_geometry(node)
                    && self
                        .field(node, "owner")
                        .is_some_and(|f| f.definition.pointer_class == 1010)
            }
        }
    }

    pub(super) fn is_surface(self, node: &RawNode) -> bool {
        match self {
            Self::OnshapeSch30000 => matches!(node.node_type, 50..=54 | 124),
            Self::TopsolidCatalog
                if matches!(node.node_type, 118 | 201) && node.definition.name == "MESH" =>
            {
                true
            }
            Self::Sch13006 => {
                matches!(node.node_type, 50..=54 | 56 | 59 | 60 | 67 | 68 | 124)
            }
            Self::TopsolidCatalog | Self::Sch3600188 => {
                matches!(node.node_type, 50..=54 | 56 | 59 | 60 | 68 | 124)
            }
            Self::Named => {
                self.has_common_geometry(node)
                    && self
                        .field(node, "owner")
                        .is_some_and(|f| f.definition.pointer_class == 1007)
            }
        }
    }

    fn has_common_geometry(self, node: &RawNode) -> bool {
        ["owner", "next", "previous", "geometric_owner", "sense"]
            .into_iter()
            .all(|role| self.field(node, role).is_some())
    }
}

/// The reviewed base definitions behind one exact embedded 13006 key.
/// TopSolid's exact catalog must retain the base fields used as B-Rep roles.
fn validate_topsolid_base_roles(
    schemas: &[SchemaResolution],
    base: &[&crate::TypeDefinition],
) -> Result<(), ParseError> {
    for resolution in schemas {
        let definition = &resolution.definition;
        let roles = field_roles(definition.node_type);
        if roles.is_empty() {
            continue;
        }
        let invalid = || {
            ParseError::new(
                ErrorKind::InvalidBrepField,
                resolution.byte_range.start,
                "TopSolid definition does not preserve hash-pinned base fields required by B-Rep",
                ErrorDetails::SchemaLookup {
                    schema: "13006".to_owned(),
                    node_type: definition.node_type,
                },
            )
        };
        let original = base
            .iter()
            .find(|candidate| candidate.node_type == definition.node_type)
            .ok_or_else(invalid)?;
        let mut copied = Vec::new();
        match definition.source {
            SchemaSource::Base | SchemaSource::EmbeddedUnchanged => {
                if definition.fields != original.fields {
                    return Err(invalid());
                }
                copied.extend(0..original.fields.len());
            }
            SchemaSource::EmbeddedDelta => {
                let mut base_index = 0;
                let mut output_index = 0;
                for edit in &resolution.edits {
                    match edit {
                        SchemaEdit::Copy { .. } => {
                            let expected = original.fields.get(base_index).ok_or_else(invalid)?;
                            if definition.fields.get(output_index) != Some(expected) {
                                return Err(invalid());
                            }
                            copied.push(output_index);
                            base_index += 1;
                            output_index += 1;
                        }
                        SchemaEdit::Delete { .. } => base_index += 1,
                        SchemaEdit::Insert { .. } | SchemaEdit::Append { .. } => output_index += 1,
                        SchemaEdit::End { .. } => break,
                    }
                }
            }
            SchemaSource::EmbeddedFull => return Err(invalid()),
        }
        for (name, _, _, _) in roles {
            let base_matches: Vec<_> = original
                .fields
                .iter()
                .enumerate()
                .filter(|(_, field)| field.name == *name)
                .collect();
            let definition_matches: Vec<_> = definition
                .fields
                .iter()
                .enumerate()
                .filter(|(_, field)| field.name == *name)
                .collect();
            if base_matches.len() != 1
                || definition_matches.len() != 1
                || !copied.contains(&definition_matches[0].0)
                || definition_matches[0].1 != base_matches[0].1
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

/// Caller catalogs may use names as roles only when every transmitted
/// attribute type exactly matches a reviewed declaration for its schema key.
pub(super) fn validate_named_attribute_roles(
    key: &str,
    schemas: &[SchemaResolution],
) -> Result<(), ParseError> {
    let attribute_schemas = schemas
        .iter()
        .filter(|resolution| matches!(resolution.definition.node_type, 79..=89 | 98))
        .collect::<Vec<_>>();
    if attribute_schemas.is_empty() {
        return Ok(());
    }
    if key == "SCH_3700192_36001_13006" {
        let definitions = crate::schema::profiles::topsolid_36001_definitions();
        if attribute_schemas.iter().all(|resolution| {
            let actual = &resolution.definition;
            definitions.iter().any(|expected| {
                actual.node_type == expected.node_type
                    && actual.name == expected.name
                    && actual.fields == expected.fields
                    && actual.variable == expected.variable
            })
        }) {
            return Ok(());
        }
    }
    let registry = crate::BuiltinProfileRegistry::compiled()?;
    let matches_reviewed_profile = registry.profiles().any(|profile| {
        let definitions = profile
            .definitions()
            .filter(|definition| matches!(definition.node_type, 79..=89 | 98))
            .collect::<Vec<_>>();
        attribute_schemas.iter().all(|resolution| {
            let definition = &resolution.definition;
            definitions.iter().any(|expected| {
                definition.node_type == expected.node_type
                    && definition.name == expected.name
                    && definition.fields == expected.fields
                    && definition.variable == expected.variable
            })
        })
    });
    if !matches_reviewed_profile {
        let resolution = attribute_schemas[0];
        return Err(ParseError::new(
            ErrorKind::InvalidBrepField,
            resolution.byte_range.start,
            "caller-supplied attribute definitions do not match one reviewed profile",
            ErrorDetails::NodeType {
                node_type: resolution.definition.node_type,
            },
        ));
    }
    Ok(())
}

fn reviewed_base_definitions(key: &str) -> Vec<crate::TypeDefinition> {
    if let Some(spec) = crate::schema::profiles::icad_legacy::PROFILES
        .iter()
        .find(|spec| key == spec.key)
    {
        spec.definitions()
    } else if key == "SCH_3701229_37102_13006"
        || crate::schema::profiles::solidworks::PROFILES
            .iter()
            .any(|spec| key == spec.key)
        || crate::schema::profiles::transmit::PROFILES
            .iter()
            .any(|spec| key == spec.key)
    {
        crate::schema::profiles::solidworks::solidworks_definitions()
    } else if key == "SCH_3701212_37102_13006" {
        crate::schema::profiles::onshape_current_definitions()
    } else if key == "SCH_3000310_30000_13006" {
        crate::schema::profiles::icad_sch30000_13006_definitions()
    } else if key == "SCH_3401212_34101_13006" {
        crate::schema::profiles::icad_v34_definitions()
    } else {
        let mut base = crate::schema::profiles::sch13006_definitions();
        base.extend(crate::schema::profiles::sch13006_sp_curve_definitions());
        base.extend(crate::schema::profiles::sch13006_bspline_surface_definitions());
        base
    }
}

/// The merged reviewed declarations of the key's schema family in the compiled
/// registry: every exact profile of the key's provider schema contributes.
fn family_definitions(key: &str) -> Option<Vec<crate::TypeDefinition>> {
    let requested = crate::SchemaKey::parse(key).ok()?;
    let family = crate::BuiltinProfileRegistry::compiled()
        .ok()?
        .family_for_key(&requested)?;
    Some(family.definitions().cloned().collect())
}

fn standard_definitions(
    key: &str,
    profile_id: &str,
    revision: u32,
    sha256: &str,
) -> Option<Result<Vec<crate::TypeDefinition>, ParseError>> {
    crate::schema::profiles::icad_standard::PROFILES
        .iter()
        .chain(
            crate::schema::profiles::legacy::PROFILES
                .iter()
                .map(|item| &item.profile),
        )
        .find(|spec| {
            key == spec.key
                && profile_id == spec.id
                && revision == spec.revision
                && sha256 == spec.sha256
        })
        .map(crate::schema::profiles::icad_standard::ProfileSpec::definitions)
}

fn unreviewed(provenance: &SchemaProviderResolution) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepField,
        0,
        "no reviewed B-Rep roles for the selected built-in profile",
        ErrorDetails::InvalidText {
            field: "schema_profile",
            value: format!("{provenance:?}"),
        },
    )
}

/// A standard key resolves every type from the compiled profile. Roles are
/// assigned only when each resolution is exactly that reviewed definition.
fn validate_standard_roles(
    schemas: &[SchemaResolution],
    reviewed: &[crate::TypeDefinition],
) -> Result<(), ParseError> {
    for resolution in schemas {
        let definition = &resolution.definition;
        let unchanged = definition.source == SchemaSource::Base
            && resolution.edits.is_empty()
            && reviewed.iter().any(|d| {
                d.node_type == definition.node_type
                    && d.fields == definition.fields
                    && d.variable == definition.variable
            });
        let unique_roles = field_roles(definition.node_type)
            .iter()
            .all(|(_, _, name, _)| {
                definition.fields.iter().filter(|f| f.name == *name).count() == 1
            });
        if !unchanged || !unique_roles {
            return Err(ParseError::new(
                ErrorKind::InvalidBrepField,
                resolution.byte_range.start,
                "standard definition is not the reviewed layout required by B-Rep",
                ErrorDetails::NodeType {
                    node_type: definition.node_type,
                },
            ));
        }
    }
    Ok(())
}

/// Embedded edits can move fields; trusted B-Rep roles must survive as copies
/// of reviewed base fields. A matching scalar codec or inserted name is not
/// sufficient to assign meaning to an input-defined field.
fn validate_base_roles(
    schemas: &[SchemaResolution],
    base: &[crate::TypeDefinition],
) -> Result<(), ParseError> {
    for resolution in schemas {
        let definition = &resolution.definition;
        let roles = field_roles(definition.node_type);
        if roles.is_empty() {
            continue;
        }
        let invalid = || {
            ParseError::new(
                ErrorKind::InvalidBrepField,
                resolution.byte_range.start,
                "embedded definition does not preserve reviewed base fields required by B-Rep",
                ErrorDetails::SchemaLookup {
                    schema: "13006".to_owned(),
                    node_type: definition.node_type,
                },
            )
        };
        let original = base
            .iter()
            .find(|d| d.node_type == definition.node_type)
            .ok_or_else(invalid)?;
        let mut copied = Vec::new();
        let mut appended = Vec::new();
        match definition.source {
            SchemaSource::Base | SchemaSource::EmbeddedUnchanged => {
                if definition.fields != original.fields {
                    return Err(invalid());
                }
                copied.extend(0..original.fields.len());
            }
            SchemaSource::EmbeddedDelta => {
                let mut base_index = 0;
                let mut output_index = 0;
                for edit in &resolution.edits {
                    match edit {
                        SchemaEdit::Copy { .. } => {
                            let expected = original.fields.get(base_index).ok_or_else(invalid)?;
                            if definition.fields.get(output_index) != Some(expected) {
                                return Err(invalid());
                            }
                            copied.push(output_index);
                            base_index += 1;
                            output_index += 1;
                        }
                        SchemaEdit::Delete { .. } => base_index += 1,
                        SchemaEdit::Insert { .. } => output_index += 1,
                        SchemaEdit::Append { field, .. } => {
                            if definition.fields.get(output_index) != Some(field) {
                                return Err(invalid());
                            }
                            appended.push(output_index);
                            output_index += 1;
                        }
                        SchemaEdit::End { .. } => break,
                    }
                }
            }
            SchemaSource::EmbeddedFull => return Err(invalid()),
        }
        for (_, _, name, _) in roles {
            let matches: Vec<_> = definition
                .fields
                .iter()
                .enumerate()
                .filter(|(_, field)| field.name == *name)
                .collect();
            if matches.len() != 1 || !copied.contains(&matches[0].0) {
                return Err(invalid());
            }
        }
        if definition.node_type == 38 {
            let data_fields: Vec<_> = definition
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.name == "intersection_data")
                .collect();
            if !data_fields.is_empty() {
                let (index, field) = data_fields[0];
                if data_fields.len() != 1
                    || !appended.contains(&index)
                    || field.field_type != crate::FieldType::PointerIndex
                    || field.pointer_class != 204
                    || field.element_count != 0
                    || !field.transmitted
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

// (mapper role, wire ordinal, independently reviewed project name, scalar code).
// V30 uses ordinals. The base/embedded mapper uses names only after validating
// that each role was copied from the reviewed 13006 field definition.
#[allow(clippy::too_many_lines)]
fn field_roles(node_type: u16) -> &'static [(&'static str, usize, &'static str, &'static str)] {
    match node_type {
        10 => &[
            ("res_size", 8, "size_precision", "f"),
            ("res_linear", 9, "linear_precision", "f"),
            ("ref_instance", 10, "instances", "p"),
            ("type", 15, "assembly_kind", "u"),
            ("sub_instance", 16, "sub_instances", "p"),
        ],
        11 => &[
            ("node_id", 0, "local_id", "d"),
            ("type", 2, "instance_kind", "u"),
            ("part", 3, "part_ref", "p"),
            ("transform", 4, "transform_ref", "p"),
            ("assembly", 5, "assembly_ref", "p"),
            ("next_in_part", 6, "next_in_assembly", "p"),
            ("next_of_part", 8, "next_of_part", "p"),
        ],
        12 => &[
            ("res_size", 9, "size_precision", "f"),
            ("res_linear", 10, "linear_precision", "f"),
            ("body_type", 16, "body_kind", "u"),
            ("region", 24, "region_head", "p"),
            ("edge", 25, "edge_head", "p"),
            ("vertex", 26, "vertex_head", "p"),
        ],
        13 => &[
            ("node_id", 0, "local_id", "d"),
            ("next", 3, "next_shell", "p"),
            ("face", 4, "back_faces", "p"),
            ("edge", 5, "wire_edges", "p"),
            ("vertex", 6, "isolated_vertex", "p"),
            ("region", 7, "region_ref", "p"),
            ("front_face", 8, "front_faces", "p"),
        ],
        14 => &[
            ("node_id", 0, "local_id", "d"),
            ("next", 3, "next_back", "p"),
            ("loop", 5, "loop_head", "p"),
            ("shell", 6, "back_shell", "p"),
            ("surface", 7, "surface_ref", "p"),
            ("sense", 8, "orientation", "c"),
            ("next_front", 11, "next_front", "p"),
            ("front_shell", 13, "front_shell", "p"),
        ],
        15 => &[
            ("node_id", 0, "local_id", "d"),
            ("halfedge", 2, "fin_ref", "p"),
            ("face", 3, "face_ref", "p"),
            ("next", 4, "next_loop", "p"),
        ],
        16 => &[
            ("node_id", 0, "local_id", "d"),
            ("tolerance", 2, "precision", "f"),
            ("halfedge", 3, "fin_ref", "p"),
            ("next", 5, "next_edge", "p"),
            ("curve", 6, "curve_ref", "p"),
            ("owner", 9, "owner_ref", "p"),
        ],
        17 => &[
            ("loop", 1, "loop_ref", "p"),
            ("forward", 2, "forward_fin", "p"),
            ("backward", 3, "backward_fin", "p"),
            ("vertex", 4, "end_vertex", "p"),
            ("other", 5, "radial_fin", "p"),
            ("edge", 6, "edge_ref", "p"),
            ("curve", 7, "curve_ref", "p"),
            ("sense", 9, "orientation", "c"),
        ],
        18 => &[
            ("node_id", 0, "local_id", "d"),
            ("next", 4, "next_vertex", "p"),
            ("point", 5, "point_ref", "p"),
            ("tolerance", 6, "precision", "f"),
            ("owner", 7, "owner_ref", "p"),
        ],
        19 => &[
            ("node_id", 0, "local_id", "d"),
            ("body", 2, "body_ref", "p"),
            ("next", 3, "next_region", "p"),
            ("shell", 5, "shell_head", "p"),
            ("type", 6, "region_kind", "c"),
        ],
        29 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("pvec", 5, "position", "v"),
        ],
        30 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("pvec", 7, "origin", "v"),
            ("direction", 8, "tangent", "v"),
        ],
        31 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("centre", 7, "center", "v"),
            ("normal", 8, "normal", "v"),
            ("x_axis", 9, "x_direction", "v"),
            ("radius", 10, "radius", "f"),
        ],
        32 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("centre", 7, "center", "v"),
            ("normal", 8, "normal", "v"),
            ("x_axis", 9, "x_direction", "v"),
            ("major_radius", 10, "major_radius", "f"),
            ("minor_radius", 11, "minor_radius", "f"),
        ],
        38 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("surface", 7, "supporting_surfaces", "p"),
            ("chart", 8, "chart_ref", "p"),
            ("start", 9, "start_limit", "p"),
            ("end", 10, "end_limit", "p"),
        ],
        40 => &[
            ("base_parameter", 0, "base_parameter", "f"),
            ("base_scale", 1, "base_scale", "f"),
            ("chart_count", 2, "chart_count", "d"),
            ("chordal_error", 3, "chordal_error", "f"),
            ("angular_error", 4, "angular_error", "f"),
            ("hvec", 6, "sample_positions", "h"),
        ],
        41 => &[
            ("type", 0, "limit_kind", "c"),
            ("hvec", 1, "limit_positions", "h"),
        ],
        133 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("basis_curve", 7, "basis_curve", "p"),
            ("point_1", 8, "start_point", "v"),
            ("point_2", 9, "end_point", "v"),
            ("parm_1", 10, "start_parameter", "f"),
            ("parm_2", 11, "end_parameter", "f"),
        ],
        124 | 134 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("nurbs", 7, "nurbs_ref", "p"),
        ],
        137 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("surface", 7, "surface_ref", "p"),
            ("b_curve", 8, "parameter_curve", "p"),
            ("original", 9, "original_curve", "p"),
            ("tolerance_to_original", 10, "original_tolerance", "f"),
        ],
        126 => &[
            ("u_periodic", 0, "u_periodic", "l"),
            ("v_periodic", 1, "v_periodic", "l"),
            ("u_degree", 2, "u_degree", "n"),
            ("v_degree", 3, "v_degree", "n"),
            ("n_u_vertices", 4, "u_control_count", "d"),
            ("n_v_vertices", 5, "v_control_count", "d"),
            ("u_knot_type", 6, "u_knot_kind", "u"),
            ("v_knot_type", 7, "v_knot_kind", "u"),
            ("n_u_knots", 8, "u_knot_count", "d"),
            ("n_v_knots", 9, "v_knot_count", "d"),
            ("rational", 10, "rational", "l"),
            ("u_closed", 11, "u_closed", "l"),
            ("v_closed", 12, "v_closed", "l"),
            ("surface_form", 13, "surface_form", "u"),
            ("vertex_dim", 14, "vertex_dimension", "n"),
            ("bspline_vertices", 15, "control_vertices", "p"),
            ("u_knot_mult", 16, "u_multiplicities", "p"),
            ("v_knot_mult", 17, "v_multiplicities", "p"),
            ("u_knots", 18, "u_knots", "p"),
            ("v_knots", 19, "v_knots", "p"),
        ],
        136 => &[
            ("degree", 0, "degree", "n"),
            ("n_vertices", 1, "control_count", "d"),
            ("vertex_dim", 2, "vertex_dimension", "n"),
            ("n_knots", 3, "knot_count", "d"),
            ("knot_type", 4, "knot_kind", "u"),
            ("periodic", 5, "periodic", "l"),
            ("closed", 6, "closed", "l"),
            ("rational", 7, "rational", "l"),
            ("curve_form", 8, "curve_form", "u"),
            ("bspline_vertices", 9, "control_vertices", "p"),
            ("knot_mult", 10, "multiplicities", "p"),
            ("knots", 11, "knots", "p"),
        ],
        45 => &[("vertices", 0, "vertices", "f")],
        127 => &[("mult", 0, "multiplicities", "n")],
        128 => &[("knots", 0, "knots", "f")],
        50 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("pvec", 7, "origin", "v"),
            ("normal", 8, "normal", "v"),
            ("x_axis", 9, "x_direction", "v"),
        ],
        51 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("pvec", 7, "origin", "v"),
            ("axis", 8, "axis", "v"),
            ("radius", 9, "radius", "f"),
            ("x_axis", 10, "x_direction", "v"),
        ],
        52 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("pvec", 7, "origin", "v"),
            ("axis", 8, "axis", "v"),
            ("radius", 9, "radius", "f"),
            ("sin_half_angle", 10, "sin_half_angle", "f"),
            ("cos_half_angle", 11, "cos_half_angle", "f"),
            ("x_axis", 12, "x_direction", "v"),
        ],
        54 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("centre", 7, "center", "v"),
            ("axis", 8, "axis", "v"),
            ("major_radius", 9, "major_radius", "f"),
            ("minor_radius", 10, "minor_radius", "f"),
            ("x_axis", 11, "x_direction", "v"),
        ],
        56 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("blend_type", 7, "blend_kind", "c"),
            ("surface", 8, "support_surfaces", "p"),
            ("spine", 9, "spine_curve", "p"),
            ("range", 10, "offsets", "f"),
            ("thumb_weight", 11, "weights", "f"),
            ("boundary", 12, "boundary_surfaces", "p"),
            ("start", 13, "start_limit", "p"),
            ("end", 14, "end_limit", "p"),
        ],
        59 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("boundary", 7, "boundary_index", "n"),
            ("blend", 8, "blend_surface", "p"),
        ],
        60 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("surface", 9, "basis_surface", "p"),
            ("offset", 10, "offset", "f"),
        ],
        // The scale is for internal use only (XT reference) and stays unread.
        67 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("section", 7, "section_curve", "p"),
            ("sweep", 8, "sweep_direction", "v"),
        ],
        // Retain the source surface and report unsupported geometry. No spun
        // surface parameters are assigned geometry semantics by this mapper.
        68 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("profile", 7, "profile_curve", "p"),
            ("base", 8, "axis_origin", "v"),
            ("axis", 9, "axis_direction", "v"),
            ("start", 10, "start_point", "v"),
            ("end", 11, "end_point", "v"),
            ("start_param", 12, "start_parameter", "f"),
            ("end_param", 13, "end_parameter", "f"),
            ("x_axis", 14, "x_direction", "v"),
        ],
        53 => &[
            ("node_id", 0, "local_id", "d"),
            ("owner", 2, "owner_ref", "p"),
            ("sense", 6, "orientation", "c"),
            ("centre", 7, "center", "v"),
            ("radius", 8, "radius", "f"),
            ("axis", 9, "axis", "v"),
            ("x_axis", 10, "x_direction", "v"),
        ],
        100 => &[
            ("node_id", 0, "local_id", "d"),
            ("rotation_matrix", 4, "rotation_matrix", "f"),
            ("translation_vector", 5, "translation", "v"),
            ("scale", 6, "scale", "f"),
            ("flag", 7, "flag", "d"),
            ("perspective_vector", 8, "perspective", "v"),
        ],
        // Attributes (public reference pp. 98-110): the definition, its
        // identifier string and the value nodes of each field.
        79 => &[("string", 0, "identifier_bytes", "c")],
        80 => &[
            ("identifier", 1, "identifier_ref", "p"),
            ("type_id", 2, "kind_id", "d"),
            ("fields", 6, "value_kinds", "u"),
        ],
        81 => &[
            ("definition", 1, "definition_ref", "p"),
            ("owner", 2, "owner_ref", "p"),
            ("fields", 7, "value_arrays", "p"),
        ],
        82 => &[("values", 0, "integers", "d")],
        83 => &[("values", 0, "reals", "f")],
        84 => &[("values", 0, "text_bytes", "c")],
        // The reference's own field name; the V30 table carries 87 and 89 so.
        85 | 86 | 87 | 89 => &[("values", 0, "values", "v")],
        88 => &[("values", 0, "values", "t")],
        98 => &[("values", 0, "utf16_units", "w")],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_body() -> Result<crate::TypeDefinition, Box<dyn std::error::Error>> {
        crate::schema::profiles::onshape_sch13006()?
            .definition(12)
            .cloned()
            .ok_or_else(|| "missing base BODY".into())
    }

    fn changed_body(replace_region: bool) -> Result<SchemaResolution, Box<dyn std::error::Error>> {
        let mut definition = base_body()?;
        let original = definition.fields.clone();
        definition.fields.clear();
        definition.source = SchemaSource::EmbeddedDelta;
        let mut edits = Vec::new();
        // Inserting an unrelated field shifts every subsequent wire ordinal.
        let extra = crate::FieldDefinition {
            name: "extra".to_owned(),
            field_type: crate::FieldType::Integer,
            pointer_class: 0,
            element_count: 0,
            transmitted: true,
        };
        definition.fields.push(extra.clone());
        edits.push(SchemaEdit::Insert {
            offset: 0,
            field: extra,
        });
        for field in original {
            if replace_region && field.name == "region_head" {
                // Same name, codec and final position as a genuine Copy.
                edits.push(SchemaEdit::Delete { offset: 0 });
                edits.push(SchemaEdit::Insert {
                    offset: 0,
                    field: field.clone(),
                });
            } else {
                edits.push(SchemaEdit::Copy { offset: 0 });
            }
            definition.fields.push(field);
        }
        edits.push(SchemaEdit::End { offset: 0 });
        Ok(SchemaResolution {
            definition,
            edits,
            raw_schema: vec![],
            byte_range: 0..0,
        })
    }

    #[test]
    fn embedded_roles_follow_copied_fields_after_an_insert()
    -> Result<(), Box<dyn std::error::Error>> {
        let resolution = changed_body(false)?;
        validate_base_roles(
            std::slice::from_ref(&resolution),
            &crate::schema::profiles::sch13006_definitions(),
        )?;
        let definition = std::sync::Arc::new(resolution.definition);
        let fields = definition
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| RawField {
                definition: std::sync::Arc::new(field.clone()),
                values: vec![],
                byte_range: std::ops::Range {
                    start: i,
                    end: i + 1,
                },
            })
            .collect();
        let node = RawNode {
            node_type: 12,
            index: 1,
            definition,
            fields,
            variable_length: None,
            first_schema: None,
            user_fields: vec![],
            byte_range: 0..0,
        };
        let region = RoleAccess::Sch13006
            .field(&node, "region")
            .ok_or("missing role")?;
        assert_eq!(region.definition.name, "region_head");
        assert_eq!(region.byte_range.start, 21);
        Ok(())
    }

    #[test]
    fn inserted_field_cannot_impersonate_a_deleted_base_role()
    -> Result<(), Box<dyn std::error::Error>> {
        let good = changed_body(false)?;
        let bad = changed_body(true)?;
        assert_eq!(good.definition, bad.definition);
        assert_eq!(
            validate_base_roles(&[bad], &crate::schema::profiles::sch13006_definitions())
                .err()
                .map(|e| e.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
        Ok(())
    }

    #[test]
    fn duplicate_or_full_input_names_cannot_acquire_base_roles()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut duplicate = changed_body(false)?;
        let region = duplicate.definition.fields[21].clone();
        duplicate.definition.fields[0] = region.clone();
        duplicate.edits[0] = SchemaEdit::Insert {
            offset: 0,
            field: region,
        };
        assert!(
            validate_base_roles(
                &[duplicate],
                &crate::schema::profiles::sch13006_definitions()
            )
            .is_err()
        );
        let mut full = changed_body(false)?;
        full.definition.source = SchemaSource::EmbeddedFull;
        assert!(
            validate_base_roles(&[full], &crate::schema::profiles::sch13006_definitions()).is_err()
        );
        Ok(())
    }

    #[test]
    fn profiles_pin_their_reviewed_base_roles() -> Result<(), Box<dyn std::error::Error>> {
        for (profile, digest) in [
            (
                crate::schema::profiles::onshape_sch13006()?,
                ROLE_BASE_SHA256,
            ),
            (
                crate::schema::profiles::icad_sch30000_13006()?,
                ROLE_EMBEDDED_SHA256,
            ),
            (
                crate::schema::profiles::icad_sch34101_13006()?,
                crate::schema::profiles::ICAD_V34_PROFILE_SHA256,
            ),
        ] {
            assert_eq!(profile.metadata().profile_sha256, digest);
            let schemas = profile
                .definitions()
                .map(|definition| SchemaResolution {
                    definition: definition.clone(),
                    edits: vec![],
                    raw_schema: vec![],
                    byte_range: 0..0,
                })
                .collect::<Vec<_>>();
            let key = profile.accepted_schema_keys().next().ok_or("missing key")?;
            let meta = profile.metadata();
            let provenance = SchemaProviderResolution::Builtin {
                profile_id: meta.profile_id.clone(),
                profile_revision: meta.revision,
                schema_key: key.raw().into(),
                coverage: meta.coverage,
                profile_sha256: meta.profile_sha256.clone(),
            };
            RoleAccess::select(&provenance, key.raw(), &schemas)?;
            let standard = profile.metadata().profile_id.starts_with("onshape");
            validate_base_roles(
                &schemas,
                &profile.definitions().cloned().collect::<Vec<_>>(),
            )?;
            assert_eq!(
                schemas
                    .iter()
                    .filter(|s| !field_roles(s.definition.node_type).is_empty())
                    .count(),
                if standard {
                    35
                } else if key.raw() == "SCH_3401212_34101_13006" {
                    36
                } else {
                    38
                }
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_profiles_reject_replaced_geometry_roles_and_unreviewed_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        for profile in crate::schema::profiles::icad_legacy_13006()? {
            let base = profile.definitions().cloned().collect::<Vec<_>>();
            let schemas = base
                .iter()
                .map(|definition| SchemaResolution {
                    definition: definition.clone(),
                    edits: vec![],
                    raw_schema: vec![],
                    byte_range: 0..0,
                })
                .collect::<Vec<_>>();
            let key = profile.accepted_schema_keys().next().ok_or("key")?.raw();
            let meta = profile.metadata();
            let mut provenance = SchemaProviderResolution::Builtin {
                profile_id: meta.profile_id.clone(),
                profile_revision: meta.revision,
                schema_key: key.into(),
                coverage: meta.coverage,
                profile_sha256: meta.profile_sha256.clone(),
            };
            RoleAccess::select(&provenance, key, &schemas)?;
            if let SchemaProviderResolution::Builtin { profile_sha256, .. } = &mut provenance {
                *profile_sha256 = "0".repeat(64);
            }
            assert!(RoleAccess::select(&provenance, key, &schemas).is_err());
            for original in base
                .iter()
                .filter(|d| matches!(d.node_type, 56 | 59 | 60 | 68 | 137))
            {
                for (_, _, role_name, _) in field_roles(original.node_type) {
                    let mut definition = original.clone();
                    definition.source = SchemaSource::EmbeddedDelta;
                    let mut edits = vec![];
                    for field in &definition.fields {
                        if field.name == *role_name {
                            edits.push(SchemaEdit::Delete { offset: 0 });
                            edits.push(SchemaEdit::Insert {
                                offset: 0,
                                field: field.clone(),
                            });
                        } else {
                            edits.push(SchemaEdit::Copy { offset: 0 });
                        }
                    }
                    edits.push(SchemaEdit::End { offset: 0 });
                    let changed = SchemaResolution {
                        definition,
                        edits,
                        raw_schema: vec![],
                        byte_range: 0..0,
                    };
                    assert!(validate_base_roles(&[changed], &base).is_err());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn standard_profiles_require_the_reviewed_layout_and_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        for profile in crate::schema::profiles::icad_legacy_standard()? {
            let reviewed = profile.definitions().cloned().collect::<Vec<_>>();
            let resolution = |definition: &crate::TypeDefinition| SchemaResolution {
                definition: definition.clone(),
                edits: vec![],
                raw_schema: vec![],
                byte_range: 0..0,
            };
            let schemas = reviewed.iter().map(resolution).collect::<Vec<_>>();
            let key = profile.accepted_schema_keys().next().ok_or("key")?.raw();
            let meta = profile.metadata();
            let mut provenance = SchemaProviderResolution::Builtin {
                profile_id: meta.profile_id.clone(),
                profile_revision: meta.revision,
                schema_key: key.into(),
                coverage: meta.coverage,
                profile_sha256: meta.profile_sha256.clone(),
            };
            assert!(matches!(
                RoleAccess::select(&provenance, key, &schemas)?,
                RoleAccess::Sch13006
            ));
            let body = reviewed.iter().find(|d| d.node_type == 12).ok_or("body")?;
            // Exchanging two references of one codec keeps every scalar layout.
            let region = body
                .fields
                .iter()
                .position(|f| f.name == "region_head")
                .ok_or("region")?;
            let mut moved = body.clone();
            moved.fields.swap(region, region + 1);
            let mut embedded = body.clone();
            embedded.source = SchemaSource::EmbeddedUnchanged;
            let mut unlisted = body.clone();
            unlisted.node_type = 60;
            for changed in [moved, embedded, unlisted] {
                assert_eq!(
                    RoleAccess::select(&provenance, key, &[resolution(&changed)])
                        .err()
                        .map(|e| e.kind()),
                    Some(ErrorKind::InvalidBrepField)
                );
            }
            if let SchemaProviderResolution::Builtin { profile_sha256, .. } = &mut provenance {
                *profile_sha256 = "0".repeat(64);
            }
            assert!(RoleAccess::select(&provenance, key, &schemas).is_err());
        }
        Ok(())
    }

    #[test]
    fn reviewed_roles_match_the_pinned_definition_order() -> Result<(), ParseError> {
        let profile = crate::schema::profiles::onshape_sch30000()?;
        assert_eq!(profile.metadata().profile_sha256, ROLE_PROFILE_SHA256);
        let mut mapped_types = 0;
        for definition in profile.definitions() {
            let roles = field_roles(definition.node_type);
            if !roles.is_empty() {
                mapped_types += 1;
            }
            let mut unique = std::collections::BTreeSet::new();
            for (role, ordinal, name, code) in roles {
                assert!(unique.insert(role));
                let ordinal = if definition.node_type == 41 && *role == "hvec" {
                    2 // V30 inserts a limit-state character before its points.
                } else {
                    *ordinal
                };
                let field = &definition.fields[ordinal];
                assert_eq!(field.name, *name, "type {} / {role}", definition.node_type);
                assert_eq!(field.field_type.code(), *code);
                let count = match (definition.node_type, *role) {
                    (38, "surface") => 2,
                    (40 | 41, "hvec")
                    | (45 | 127 | 128, _)
                    | (79, "string")
                    | (80 | 81, "fields")
                    | (82..=89 | 98 | 204, "values") => 1,
                    _ => 0,
                };
                assert_eq!(
                    field.element_count, count,
                    "type {} / {role}",
                    definition.node_type
                );
                assert!(field.transmitted);
            }
        }
        assert_eq!(mapped_types, 38);
        Ok(())
    }
}
