//! B-Rep roles reviewed against the M8.1 definitions and M8.3 geometry evidence.
//! Ordinals belong to this exact profile revision; raw names remain unchanged.

use crate::{
    BuiltinProfileCoverage, ErrorDetails, ErrorKind, ParseError, RawField, RawNode,
    SchemaProviderResolution,
};

// Independently pin the definition revision reviewed with these semantic roles.
const ROLE_PROFILE_SHA256: &str =
    "e28a5e11a7713573a7134025bd8c3d83f194fc663662f079e839a95ea5981f80";

#[derive(Clone, Copy)]
pub(super) enum RoleAccess {
    Named,
    OnshapeSch30000,
}

impl RoleAccess {
    pub(super) fn select(
        provenance: &SchemaProviderResolution,
        key: &str,
    ) -> Result<Self, ParseError> {
        match provenance {
            SchemaProviderResolution::CallerSupplied => Ok(Self::Named),
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage,
                profile_sha256,
            } if profile_id == "onshape-sch30000-r1"
                && *profile_revision == 1
                && schema_key == "SCH_3000000_30000"
                && key == schema_key
                && *coverage == BuiltinProfileCoverage::VerifiedSubset
                && profile_sha256 == ROLE_PROFILE_SHA256 =>
            {
                Ok(Self::OnshapeSch30000)
            }
            SchemaProviderResolution::Builtin { .. } => Err(ParseError::new(
                ErrorKind::InvalidBrepField,
                0,
                "no reviewed B-Rep roles for the selected built-in profile",
                ErrorDetails::InvalidText {
                    field: "schema_profile",
                    value: format!("{provenance:?}"),
                },
            )),
        }
    }

    pub(super) fn type_name(self, node: &RawNode) -> &str {
        match self {
            Self::Named => &node.definition.name,
            Self::OnshapeSch30000 => match node.node_type {
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
                50 => "PLANE",
                51 => "CYLINDER",
                _ => "", // List/attribute types cannot acquire roles from their names.
            },
        }
    }

    pub(super) fn field<'a>(self, node: &'a RawNode, role: &str) -> Option<&'a RawField> {
        match self {
            Self::Named => node.fields.iter().find(|f| f.definition.name == role),
            Self::OnshapeSch30000 => field_roles(node.node_type)
                .iter()
                .find(|(name, _, _, _)| *name == role)
                .and_then(|(_, ordinal, _, _)| node.fields.get(*ordinal)),
        }
    }

    pub(super) fn is_curve(self, node: &RawNode) -> bool {
        match self {
            Self::OnshapeSch30000 => matches!(node.node_type, 30 | 31),
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
            Self::OnshapeSch30000 => matches!(node.node_type, 50 | 51),
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

// (mapper role, wire ordinal, independently reviewed project name, scalar code).
// Names/codes are assertions for the definition-order test, not runtime selectors.
#[allow(clippy::too_many_lines)]
fn field_roles(node_type: u16) -> &'static [(&'static str, usize, &'static str, &'static str)] {
    match node_type {
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
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                let field = &definition.fields[*ordinal];
                assert_eq!(field.name, *name, "type {} / {role}", definition.node_type);
                assert_eq!(field.field_type.code(), *code);
                assert_eq!(field.element_count, 0);
                assert!(field.transmitted);
            }
        }
        assert_eq!(mapped_types, 13);
        Ok(())
    }
}
