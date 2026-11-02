//! Assembly structure of the 13006 base: `ASSEMBLY` (10), `INSTANCE` (11) and
//! `TRANSFORM` (100).
//!
//! Layouts follow the public XT Format Reference, April 2008, printed pages
//! 78-79 (transform) and 83-86 (assembly, instance); project field labels are
//! assigned independently after the `BODY` declaration. Three public transmit
//! files carry these types: a `SolidWorks` 2020 assembly export
//! (`SCH_3101290_31100_13006`, whose embedded edits insert `mesh` and
//! `polyline` after the seventh base field of `ASSEMBLY` and append
//! `mesh_offset_data`), an NX transmit file (`SCH_2600169_25001_13006`,
//! unchanged layouts) and an NX part under the standard key
//! `SCH_2400000_20000`. `KEY` (102) is referenced by no observed file and stays
//! undeclared. No Siemens catalog is used.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};

/// The reviewed assembly, instance and transform declarations.
pub(crate) fn definitions() -> Vec<TypeDefinition> {
    use FieldType::{Double as F, Integer as D, PointerIndex as P, UnsignedByte as U, Vector as V};
    let field = |name: &str, field_type, pointer_class| FieldDefinition {
        name: name.to_owned(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    };
    vec![
        // Printed pages 83-84: the part header shared with BODY, then the
        // assembly kind and the head of its instance chain.
        TypeDefinition::from_fields(
            10,
            "sch13006_type_10",
            "Reviewed 13006 assembly",
            vec![
                field("max_local_id", D, 0),
                field("annotations", P, 1019),
                field("attribute_lists", P, 70),
                field("list_ref", P, 70),
                field("construction_surfaces", P, 1006),
                field("construction_curves", P, 1008),
                field("construction_points", P, 29),
                field("key_ref", P, 102),
                field("size_precision", F, 0),
                field("linear_precision", F, 0),
                field("instances", P, 11),
                field("next_assembly", P, 10),
                field("previous_assembly", P, 10),
                field("storage_state", U, 0),
                field("legacy_owner", P, 101),
                field("assembly_kind", U, 0),
                field("sub_instances", P, 11),
            ],
            SchemaSource::Base,
        ),
        // Printed pages 85-86: the positioned part (BODY or ASSEMBLY), its
        // transform and the two instance chains the instance belongs to.
        TypeDefinition::from_fields(
            11,
            "sch13006_type_11",
            "Reviewed 13006 instance",
            vec![
                field("local_id", D, 0),
                field("annotations", P, 1019),
                field("instance_kind", U, 0),
                field("part_ref", P, 1005),
                field("transform_ref", P, 100),
                field("assembly_ref", P, 10),
                field("next_in_assembly", P, 11),
                field("previous_in_assembly", P, 11),
                field("next_of_part", P, 11),
                field("previous_of_part", P, 11),
            ],
            SchemaSource::Base,
        ),
        // Printed pages 78-79: x' = (rotation . x + translation) * scale. The
        // flag bits mark the non-trivial components; the perspective vector is
        // always the null vector.
        TypeDefinition::from_fields(
            100,
            "sch13006_type_100",
            "Reviewed 13006 transform",
            vec![
                field("local_id", D, 0),
                field("owner_ref", P, 1023),
                field("next_transform", P, 100),
                field("previous_transform", P, 100),
                FieldDefinition {
                    element_count: 9,
                    ..field("rotation_matrix", F, 0)
                },
                field("translation", V, 0),
                field("scale", F, 0),
                field("flag", D, 0),
                field("perspective", V, 0),
            ],
            SchemaSource::Base,
        ),
    ]
}
