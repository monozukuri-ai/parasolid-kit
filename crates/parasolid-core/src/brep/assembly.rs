//! Parasolid assembly metadata for the B-Rep model and standalone mapping API.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::{
    ErrorDetails, ErrorKind, FieldType, FieldValue, ParseError, RawField, RawNode, SchemaEdit,
    SchemaProviderResolution, SchemaSource, TypeDefinition, XbDocument, XtDocument,
};

/// Source nodes in an assembly graph. All identifiers are transmit indices.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyStructure {
    /// Assemblies in source order.
    pub assemblies: Vec<AssemblyNode>,
    /// Instances in source order.
    pub instances: Vec<InstanceNode>,
    /// Referenced transforms in source order.
    pub transforms: Vec<TransformNode>,
}

#[derive(Clone, Copy)]
struct AssemblyFieldNames {
    assembly_size_precision: &'static str,
    assembly_linear_precision: &'static str,
    assembly_instances: &'static str,
    assembly_kind: &'static str,
    assembly_sub_instances: &'static str,
    instance_kind: &'static str,
    instance_part: &'static str,
    instance_transform: &'static str,
    instance_assembly: &'static str,
    instance_next_in_assembly: &'static str,
    instance_previous_in_assembly: &'static str,
    instance_next_of_part: &'static str,
    instance_previous_of_part: &'static str,
    transform_translation: &'static str,
    transform_perspective: &'static str,
}

impl AssemblyFieldNames {
    const GENERIC: Self = Self {
        assembly_size_precision: "size_precision",
        assembly_linear_precision: "linear_precision",
        assembly_instances: "instances",
        assembly_kind: "assembly_kind",
        assembly_sub_instances: "sub_instances",
        instance_kind: "instance_kind",
        instance_part: "part_ref",
        instance_transform: "transform_ref",
        instance_assembly: "assembly_ref",
        instance_next_in_assembly: "next_in_assembly",
        instance_previous_in_assembly: "previous_in_assembly",
        instance_next_of_part: "next_of_part",
        instance_previous_of_part: "previous_of_part",
        transform_translation: "translation",
        transform_perspective: "perspective",
    };

    const TOPSOLID: Self = Self {
        assembly_size_precision: "res_size",
        assembly_linear_precision: "res_linear",
        assembly_instances: "ref_instance",
        assembly_kind: "type",
        assembly_sub_instances: "sub_instance",
        instance_kind: "type",
        instance_part: "part",
        instance_transform: "transform",
        instance_assembly: "assembly",
        instance_next_in_assembly: "next_in_part",
        instance_previous_in_assembly: "prev_in_part",
        instance_next_of_part: "next_of_part",
        instance_previous_of_part: "prev_of_part",
        transform_translation: "translation_vector",
        transform_perspective: "perspective_vector",
    };

    fn for_key(key: &str) -> Self {
        if crate::schema::profiles::topsolid::SUPPORTED_SCHEMA_KEYS.contains(&key) {
            Self::TOPSOLID
        } else {
            Self::GENERIC
        }
    }
}

fn is_topsolid_builtin_profile(provider: &SchemaProviderResolution, key: &str) -> bool {
    let SchemaProviderResolution::Builtin {
        profile_id,
        profile_revision,
        schema_key,
        coverage: crate::BuiltinProfileCoverage::VerifiedSubset,
        profile_sha256,
    } = provider
    else {
        return false;
    };
    key == schema_key
        && *profile_revision == 1
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
                "SCH_3300259_32001_13006" | "SCH_3300261_32001_13006",
                "5ffe6a8942c686e400b3cad1edc19fbe39457589079769d9f7dbcafec535a159"
            ) | (
                "topsolid-v6-psolid-26105-r1",
                "SCH_2701192_26105_13006",
                "c21517ad14061ff14e2636bb90842ba74b3ad4980b7d041abd78732d80249e95"
            )
        )
}

impl AssemblyStructure {
    /// Enumerate every BODY occurrence below `root_assembly` in source order.
    ///
    /// Each returned path is ordered from the leaf BODY instance to its root
    /// instance, matching [`Self::place_point`]. `max_expanded_instances`
    /// bounds child-instance visits, including repeated visits through shared
    /// assembly DAGs.
    /// # Errors
    /// Returns an error if the root or any referenced instance is invalid, the
    /// graph contains a cycle, or the expansion cap is exceeded.
    pub fn leaf_body_instance_paths(
        &self,
        root_assembly: u32,
        max_expanded_instances: usize,
    ) -> Result<Vec<Vec<u32>>, ParseError> {
        let index = AssemblyPathIndex::new(self)?;
        if !index.assemblies.contains_key(&root_assembly) {
            return Err(invalid_lookup(root_assembly, "ASSEMBLY"));
        }
        validate_instance_links(self, &index)?;

        enumerate_leaf_paths(&index, root_assembly, max_expanded_instances)
    }

    /// Place a point through the listed instances, ordered from leaf to root.
    /// A null instance transform acts as identity; an empty path leaves the
    /// point in its current frame.
    ///
    /// # Errors
    /// Returns an error for non-finite points, unresolved references, or a path
    /// that does not follow nested assemblies.
    pub fn place_point(
        &self,
        mut point: [f64; 3],
        instance_path: &[u32],
    ) -> Result<[f64; 3], ParseError> {
        if point.iter().any(|value| !value.is_finite()) {
            return Err(invalid_field(None, "placement point is non-finite"));
        }
        let mut expected_assembly = None;
        for index in instance_path {
            let instance = unique_by_index(&self.instances, *index, |node| node.index)
                .ok_or_else(|| invalid_lookup(*index, "INSTANCE"))?;
            if let Some(expected) = expected_assembly {
                if !instance.part_is_assembly || instance.part != expected {
                    return Err(invalid_field(
                        None,
                        "instance path does not follow nested assemblies",
                    ));
                }
            }
            if unique_by_index(&self.assemblies, instance.assembly, |node| node.index).is_none() {
                return Err(invalid_lookup(instance.assembly, "ASSEMBLY"));
            }
            if let Some(transform_index) = instance.transform {
                let transform =
                    unique_by_index(&self.transforms, transform_index, |node| node.index)
                        .ok_or_else(|| invalid_lookup(transform_index, "TRANSFORM"))?;
                point = transform.transform_point(point)?;
            }
            expected_assembly = Some(instance.assembly);
        }
        Ok(point)
    }

    /// Place triangle vertices through the listed instances, ordered from leaf to root.
    /// The input is preserved and the returned triangles retain their vertex order.
    ///
    /// # Errors
    /// Returns an error for non-finite vertices, unresolved references, invalid
    /// paths, or non-finite composed placements.
    pub fn place_triangles(
        &self,
        triangles: &[[[f64; 3]; 3]],
        instance_path: &[u32],
    ) -> Result<Vec<[[f64; 3]; 3]>, ParseError> {
        let mut matrix = identity_matrix();
        let mut translation = [0.0; 3];
        let mut expected_assembly = None;

        for index in instance_path {
            let instance = unique_by_index(&self.instances, *index, |node| node.index)
                .ok_or_else(|| invalid_lookup(*index, "INSTANCE"))?;
            if let Some(expected) = expected_assembly {
                if !instance.part_is_assembly || instance.part != expected {
                    return Err(invalid_field(
                        None,
                        "instance path does not follow nested assemblies",
                    ));
                }
            }
            if unique_by_index(&self.assemblies, instance.assembly, |node| node.index).is_none() {
                return Err(invalid_lookup(instance.assembly, "ASSEMBLY"));
            }
            if let Some(transform_index) = instance.transform {
                let transform =
                    unique_by_index(&self.transforms, transform_index, |node| node.index)
                        .ok_or_else(|| invalid_lookup(transform_index, "TRANSFORM"))?;
                let (next_matrix, next_translation) = affine_transform(transform)?;
                let composed_translation = add_vector(
                    multiply_matrix_vector(next_matrix, translation),
                    next_translation,
                );
                matrix = multiply_matrices(next_matrix, matrix);
                translation = composed_translation;
                if matrix
                    .iter()
                    .chain(translation.iter())
                    .any(|value| !value.is_finite())
                {
                    return Err(invalid_field(None, "composed placement is non-finite"));
                }
            }
            expected_assembly = Some(instance.assembly);
        }

        if triangles
            .iter()
            .flatten()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err(invalid_field(None, "triangle vertex is non-finite"));
        }

        let mut placed = Vec::new();
        placed
            .try_reserve_exact(triangles.len())
            .map_err(|_| invalid_field(None, "unable to allocate placed triangles"))?;
        for triangle in triangles {
            let mut vertices = [[0.0; 3]; 3];
            for (vertex, output) in triangle.iter().zip(vertices.iter_mut()) {
                *output = add_vector(multiply_matrix_vector(matrix, *vertex), translation);
                if output.iter().any(|value| !value.is_finite()) {
                    return Err(invalid_field(None, "placed triangle vertex is non-finite"));
                }
            }
            placed.push(vertices);
        }
        Ok(placed)
    }

    /// Tessellate the BODY targeted by a leaf-first instance path and place its
    /// triangle vertices through that path.
    ///
    /// The first instance must target a BODY, joined to `brep` by the BODY's
    /// source transmit index. Triangle vertex order is preserved by placement.
    ///
    /// # Errors
    /// Returns an error for an empty or invalid BODY path, an unresolved BODY,
    /// failed tessellation, or invalid placement data.
    pub fn tessellate_body_instance(
        &self,
        brep: &super::BrepModel,
        instance_path: &[u32],
        tolerance: f64,
    ) -> Result<Vec<[[f64; 3]; 3]>, ParseError> {
        let Some(&leaf_index) = instance_path.first() else {
            return Err(invalid_field(None, "BODY instance path is empty"));
        };
        let leaf = unique_by_index(&self.instances, leaf_index, |instance| instance.index)
            .ok_or_else(|| invalid_lookup(leaf_index, "unique BODY INSTANCE"))?;
        if leaf.part_is_assembly {
            return Err(invalid_field(None, "first instance does not target a BODY"));
        }

        let body = unique_by_index(&brep.bodies, leaf.part, |body| body.source.node_index)
            .ok_or_else(|| invalid_lookup(leaf.part, "unique mapped BODY"))?;
        let mesh = super::tessellation::tessellate_brep_body_to_mesh(brep, body.id, tolerance)
            .ok_or_else(|| invalid_field(None, "mapped BODY faces could not be tessellated"))?;
        self.place_triangles(&mesh.triangles, instance_path)
    }

    /// Tessellate and place every supported leaf BODY occurrence below an assembly.
    ///
    /// Triangle order follows the source instance paths and then each body's
    /// tessellation order. Any invalid leaf makes the whole operation fail.
    ///
    /// # Errors
    /// Returns an error for an invalid assembly graph, exceeded expansion cap,
    /// or any BODY that cannot be tessellated and placed.
    pub fn tessellate_assembly_bodies(
        &self,
        brep: &super::BrepModel,
        root_assembly: u32,
        max_expanded_instances: usize,
        tolerance: f64,
    ) -> Result<Vec<[[f64; 3]; 3]>, ParseError> {
        let paths = self.leaf_body_instance_paths(root_assembly, max_expanded_instances)?;
        let mut triangles = Vec::new();
        for path in paths {
            let placed = self.tessellate_body_instance(brep, &path, tolerance)?;
            triangles
                .try_reserve(placed.len())
                .map_err(|_| invalid_field(None, "unable to allocate assembly triangles"))?;
            triangles.extend(placed);
        }
        Ok(triangles)
    }

    /// Tessellate and place every supported leaf BODY occurrence, expanding
    /// any resolved compound BODY children in their source order.
    ///
    /// # Errors
    /// Returns an error for an invalid assembly graph, exceeded expansion cap,
    /// unresolved BODY hierarchy, or any BODY that cannot be tessellated and
    /// placed.
    pub fn tessellate_resolved_assembly_bodies(
        &self,
        brep: &super::BrepModelWithResolvedBodyHierarchy,
        root_assembly: u32,
        max_expanded_instances: usize,
        tolerance: f64,
    ) -> Result<Vec<[[f64; 3]; 3]>, ParseError> {
        let paths = self.leaf_body_instance_paths(root_assembly, max_expanded_instances)?;
        let mut triangles = Vec::new();
        for path in paths {
            let Some(&leaf_index) = path.first() else {
                return Err(invalid_field(None, "BODY instance path is empty"));
            };
            let leaf = unique_by_index(&self.instances, leaf_index, |instance| instance.index)
                .ok_or_else(|| invalid_lookup(leaf_index, "unique BODY INSTANCE"))?;
            if leaf.part_is_assembly {
                return Err(invalid_field(None, "first instance does not target a BODY"));
            }
            let body =
                unique_by_index(&brep.model.bodies, leaf.part, |body| body.source.node_index)
                    .ok_or_else(|| invalid_lookup(leaf.part, "unique mapped BODY"))?;
            let mesh = super::tessellation::tessellate_resolved_brep_body_to_mesh(
                brep, body.id, tolerance,
            )
            .ok_or_else(|| invalid_field(None, "mapped BODY faces could not be tessellated"))?;
            let placed = self.place_triangles(&mesh.triangles, &path)?;
            triangles
                .try_reserve(placed.len())
                .map_err(|_| invalid_field(None, "unable to allocate assembly triangles"))?;
            triangles.extend(placed);
        }
        Ok(triangles)
    }
}

fn identity_matrix() -> [f64; 9] {
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
}

fn affine_transform(transform: &TransformNode) -> Result<([f64; 9], [f64; 3]), ParseError> {
    if transform
        .perspective
        .iter()
        .flatten()
        .any(|value| !value.is_finite() || *value != 0.0)
        || !transform.scale.is_finite()
        || transform.scale == 0.0
    {
        return Err(invalid_field(
            None,
            "transform cannot place triangle vertices",
        ));
    }
    let mut matrix = [0.0; 9];
    for (index, value) in matrix.iter_mut().enumerate() {
        let correction = transform
            .precision
            .as_ref()
            .map_or(0.0, |precision| precision.rotation_correction[index]);
        *value = (transform.rotation[index] + correction) * transform.scale;
    }
    let mut translation = [0.0; 3];
    for (index, value) in translation.iter_mut().enumerate() {
        let correction = transform
            .precision
            .as_ref()
            .map_or(0.0, |precision| precision.translation_correction[index]);
        *value = (transform.translation[index] + correction) * transform.scale;
    }
    if matrix
        .iter()
        .chain(translation.iter())
        .any(|value| !value.is_finite())
    {
        return Err(invalid_field(None, "transform values are non-finite"));
    }
    Ok((matrix, translation))
}

fn multiply_matrix_vector(matrix: [f64; 9], vector: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| {
        (0..3)
            .map(|column| matrix[row * 3 + column] * vector[column])
            .sum()
    })
}

fn multiply_matrices(left: [f64; 9], right: [f64; 9]) -> [f64; 9] {
    std::array::from_fn(|index| {
        let row = index / 3;
        let column = index % 3;
        (0..3)
            .map(|inner| left[row * 3 + inner] * right[inner * 3 + column])
            .sum()
    })
}

fn add_vector(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|index| left[index] + right[index])
}

struct AssemblyPathIndex<'a> {
    assemblies: BTreeMap<u32, &'a AssemblyNode>,
    instances: BTreeMap<u32, &'a InstanceNode>,
}

impl<'a> AssemblyPathIndex<'a> {
    fn new(structure: &'a AssemblyStructure) -> Result<Self, ParseError> {
        let mut assemblies = BTreeMap::new();
        for assembly in &structure.assemblies {
            if assemblies.insert(assembly.index, assembly).is_some() {
                return Err(invalid_lookup(assembly.index, "unique ASSEMBLY"));
            }
        }
        let mut instances = BTreeMap::new();
        for instance in &structure.instances {
            if instances.insert(instance.index, instance).is_some() {
                return Err(invalid_lookup(instance.index, "unique INSTANCE"));
            }
        }
        Ok(Self {
            assemblies,
            instances,
        })
    }
}

fn validate_instance_links(
    structure: &AssemblyStructure,
    indexes: &AssemblyPathIndex<'_>,
) -> Result<(), ParseError> {
    let mut child_links = BTreeSet::new();
    let mut assembly_references = BTreeSet::new();
    for assembly in &structure.assemblies {
        let mut local_children = BTreeSet::new();
        for index in &assembly.instances {
            if !local_children.insert(*index) || !child_links.insert(*index) {
                return Err(invalid_field(
                    None,
                    "instance is linked more than once as a child",
                ));
            }
            let instance = indexes
                .instances
                .get(index)
                .copied()
                .ok_or_else(|| invalid_lookup(*index, "INSTANCE"))?;
            if instance.assembly != assembly.index {
                return Err(invalid_field(
                    None,
                    "child instance owner does not match its assembly",
                ));
            }
        }

        let mut local_references = BTreeSet::new();
        for index in &assembly.referencing_instances {
            if !local_references.insert(*index) || !assembly_references.insert(*index) {
                return Err(invalid_field(
                    None,
                    "instance is linked more than once as an assembly reference",
                ));
            }
            let instance = indexes
                .instances
                .get(index)
                .copied()
                .ok_or_else(|| invalid_lookup(*index, "INSTANCE"))?;
            if !instance.part_is_assembly || instance.part != assembly.index {
                return Err(invalid_field(
                    None,
                    "referencing instance does not target its listed assembly",
                ));
            }
        }
    }

    for instance in &structure.instances {
        if !child_links.contains(&instance.index) {
            return Err(invalid_lookup(
                instance.index,
                "listed owning ASSEMBLY child",
            ));
        }
        if instance.part_is_assembly != assembly_references.contains(&instance.index) {
            return Err(invalid_lookup(
                instance.index,
                "listed target ASSEMBLY reference",
            ));
        }
        if instance.part_is_assembly && !indexes.assemblies.contains_key(&instance.part) {
            return Err(invalid_lookup(instance.part, "ASSEMBLY"));
        }
    }
    Ok(())
}

fn enumerate_leaf_paths(
    index: &AssemblyPathIndex<'_>,
    root_assembly: u32,
    max_expanded_instances: usize,
) -> Result<Vec<Vec<u32>>, ParseError> {
    let mut paths = Vec::new();
    let mut outer_instances = Vec::new();
    let mut active_assemblies = BTreeSet::new();
    active_assemblies.insert(root_assembly);
    let mut frames = vec![(root_assembly, 0_usize)];
    let mut expanded_instances = 0;

    while let Some((assembly_index, child_position)) = frames.last().copied() {
        let assembly = index
            .assemblies
            .get(&assembly_index)
            .copied()
            .ok_or_else(|| invalid_lookup(assembly_index, "ASSEMBLY"))?;
        if child_position == assembly.instances.len() {
            frames.pop();
            active_assemblies.remove(&assembly_index);
            if !frames.is_empty() {
                outer_instances.pop();
            }
            continue;
        }
        if let Some(frame) = frames.last_mut() {
            frame.1 += 1;
        }

        if expanded_instances >= max_expanded_instances {
            return Err(invalid_field(
                None,
                "assembly path enumeration exceeded its explicit cap",
            ));
        }
        expanded_instances += 1;
        let instance_index = assembly.instances[child_position];
        let instance = index
            .instances
            .get(&instance_index)
            .copied()
            .ok_or_else(|| invalid_lookup(instance_index, "INSTANCE"))?;
        if instance.part_is_assembly {
            if !active_assemblies.insert(instance.part) {
                return Err(invalid_field(
                    None,
                    "assembly instance graph contains a cycle",
                ));
            }
            outer_instances.push(instance.index);
            frames.push((instance.part, 0));
        } else {
            let mut path = Vec::with_capacity(outer_instances.len() + 1);
            path.push(instance.index);
            path.extend(outer_instances.iter().rev().copied());
            paths.push(path);
        }
    }
    Ok(paths)
}

/// A Parasolid assembly and its raw instance-chain heads.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyNode {
    /// Transmit index.
    pub index: u32,
    /// Raw assembly-kind code (known values are 1 through 3).
    pub kind: u8,
    /// Source size precision.
    pub size_precision: f64,
    /// Source linear precision.
    pub linear_precision: f64,
    /// Child instance indices in `sub_instances` chain order.
    pub instances: Vec<u32>,
    /// Referencing instance indices in `instances` chain order.
    pub referencing_instances: Vec<u32>,
    /// Complete source node byte range.
    pub byte_range: Range<usize>,
}

/// A body or nested assembly occurrence, retaining source transform linkage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceNode {
    /// Transmit index.
    pub index: u32,
    /// Raw instance-kind code (1 positive, 2 negative).
    pub kind: u8,
    /// Transmit index of the target BODY or ASSEMBLY.
    pub part: u32,
    /// Whether `part` targets an assembly.
    pub part_is_assembly: bool,
    /// Effective optional transform index; `None` means null or its target was not transmitted.
    pub transform: Option<u32>,
    /// Owning assembly transmit index.
    pub assembly: u32,
    /// Complete source node byte range.
    pub byte_range: Range<usize>,
}

/// Uncomposed transform values as transmitted by the source.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformNode {
    /// Transmit index.
    pub index: u32,
    /// Nine matrix values in original row-major transmit order.
    pub rotation: [f64; 9],
    /// Translation components, retained in source units.
    pub translation: [f64; 3],
    /// Source scale.
    pub scale: f64,
    /// Raw flags value.
    pub flags: i32,
    /// Optional perspective values, retained as nullable source components.
    pub perspective: [Option<f64>; 3],
    /// Optional higher-precision rotation and translation corrections.
    pub precision: Option<TransformPrecision>,
    /// Complete source node byte range.
    pub byte_range: Range<usize>,
}

impl TransformNode {
    /// Apply this transform to a point using `(R * point + T) * scale`.
    /// Precision corrections are added to the raw matrix and translation once
    /// for this application; stored source values are left untouched. Null or
    /// zero perspective terms are accepted, but nonzero perspective is not.
    ///
    /// # Errors
    /// Returns an error if the point or transform is non-finite, the scale is
    /// zero, perspective is nonzero, or the placed point is non-finite.
    pub fn transform_point(&self, point: [f64; 3]) -> Result<[f64; 3], ParseError> {
        if point.iter().any(|value| !value.is_finite())
            || self
                .perspective
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || *value != 0.0)
            || !self.scale.is_finite()
            || self.scale == 0.0
        {
            return Err(invalid_field(None, "transform cannot place this point"));
        }
        let corrected = |raw: f64, correction: Option<f64>| raw + correction.unwrap_or(0.0);
        let mut placed = [0.0; 3];
        for (row, output) in placed.iter_mut().enumerate() {
            *output = (0..3)
                .map(|column| {
                    corrected(
                        self.rotation[row * 3 + column],
                        self.precision
                            .as_ref()
                            .map(|precision| precision.rotation_correction[row * 3 + column]),
                    ) * point[column]
                })
                .sum::<f64>()
                + corrected(
                    self.translation[row],
                    self.precision
                        .as_ref()
                        .map(|precision| precision.translation_correction[row]),
                );
            *output *= self.scale;
        }
        if placed.iter().any(|value| !value.is_finite()) {
            return Err(invalid_field(None, "placed point is non-finite"));
        }
        Ok(placed)
    }
}

/// Raw correction values from a referenced `TRANSFORM_PRECISION` node.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformPrecision {
    /// Transmit index of the source precision node.
    pub index: u32,
    /// Rotation corrections in original row-major transmit order.
    pub rotation_correction: [f64; 9],
    /// Translation corrections in source units.
    pub translation_correction: [f64; 3],
}

/// Map assembly structure from an `X_B` document.
///
/// Only explicit named caller schemas and exact reviewed `SolidWorks` or `TopSolid`
/// profiles are accepted. No placement is composed or applied to B-Rep geometry.
///
/// # Errors
/// Returns an error for unsupported profile identity, malformed fields, bad
/// links, cycles, or a non-finite/zero-scale transform.
pub fn map_xb_assembly(document: &XbDocument) -> Result<AssemblyStructure, ParseError> {
    map_assembly(
        &document.nodes,
        &document.schema_provider,
        document.schema_key.raw(),
    )
}

/// Map assembly structure from an `X_T` document.
///
/// # Errors
/// Returns an error for unsupported profile identity, malformed fields, bad
/// links, cycles, or a non-finite/zero-scale transform.
pub fn map_xt_assembly(document: &XtDocument) -> Result<AssemblyStructure, ParseError> {
    map_assembly(
        &document.nodes,
        &document.schema_provider,
        document.schema_key.raw(),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "Assembly mapping keeps profile, layout, reference, and transform validation in their established order."
)]
fn map_assembly(
    nodes: &[RawNode],
    provider: &SchemaProviderResolution,
    key: &str,
) -> Result<AssemblyStructure, ParseError> {
    let fields = AssemblyFieldNames::for_key(key);
    let topsolid_builtin = is_topsolid_builtin_profile(provider, key);
    let builtin_family = family_has_assembly_roles(provider, key);
    let builtin = builtin_family
        || topsolid_builtin
        || matches!(
            provider,
            SchemaProviderResolution::Builtin {
                profile_id,
                profile_revision,
                schema_key,
                coverage: crate::BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256,
            } if (profile_id == "solidworks-3101290-31100-13006-r2"
                && *profile_revision == 2
                && schema_key == "SCH_3101290_31100_13006"
                && profile_sha256 == "bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b"
                || profile_id == "solidworks-sch37102-13006-r3"
                    && *profile_revision == 3
                    && schema_key == "SCH_3701229_37102_13006"
                    && profile_sha256 == "7300096c034365663a13f40c96a919d19b73d25b4c957f9dbb0e472c0a381730"
                || profile_id == "icad-1901315-19008-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_1901315_19008_13006"
                    && profile_sha256 == "e9d776fdc6cd29bdc9b3bbbc05e0b75c10e287d515a5ac7c6cad01547639ded8"
                || profile_id == "icad-2401000-20000-r3"
                    && *profile_revision == 3
                    && schema_key == "SCH_2401000_20000"
                    && profile_sha256 == "fbdd2b2c31177b9fba0104c4a7d9ead562c38202aff91f2759f30101557fa53f"
                || profile_id == "parasolid-2400164-20000-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_2400164_20000_13006"
                    && profile_sha256 == "ef75cfbf0f21455156dde0b6d346e3be92dd114f9eb2500f4a2e7f7dd9de39ae"
                || profile_id == "nx-2600169-25001-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_2600169_25001_13006"
                    && profile_sha256 == "45a2e1d9ca4e48831c5e4c757978f0cde09e3d67c46d88392055f52ea031eea0"
                || profile_id == "parasolid-2600217-25001-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_2600217_25001_13006"
                    && profile_sha256 == "99becb31c455144e4bb98e25fa97232518754b4ea9b6115c3533f554ad8ce18b"
                || profile_id == "nx-3001232-30100-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_3001232_30100_13006"
                    && profile_sha256 == "a4d96295b72774062ffc6a7fb124664e1254f1df584748bbc5960897ec9b576b"
                || profile_id == "nx-3400176-34001-13006-r2"
                    && *profile_revision == 2
                    && schema_key == "SCH_3400176_34001_13006"
                    && profile_sha256 == "254c0340fcef549d67badc03c25653f518e06d7657b892308a48f1f4b3aabf05")
                && key == schema_key
        );
    let role = |node: &RawNode| {
        if builtin {
            match node.node_type {
                10 => "ASSEMBLY",
                11 => "INSTANCE",
                12 => "BODY",
                100 => "TRANSFORM",
                _ => "",
            }
        } else {
            match node.definition.name.as_str() {
                "ASSEMBLY" => "ASSEMBLY",
                "INSTANCE" => "INSTANCE",
                "TRANSFORM" => "TRANSFORM",
                "BODY" => "BODY",
                _ => "",
            }
        }
    };
    if matches!(provider, SchemaProviderResolution::Builtin { .. }) && !builtin {
        return Err(ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            "assembly mapping requires an exact reviewed profile",
            ErrorDetails::None,
        ));
    }
    let mut by_index = BTreeMap::new();
    for node in nodes {
        if by_index.insert(node.index, node).is_some() {
            return Err(ParseError::new(
                ErrorKind::DuplicateNodeIndex,
                node.byte_range.start,
                format!("assembly transmit index {} is ambiguous", node.index),
                ErrorDetails::NodeIndex {
                    node_index: node.index,
                },
            ));
        }
    }
    let assemblies = nodes
        .iter()
        .filter(|node| role(node) == "ASSEMBLY")
        .collect::<Vec<_>>();
    let instances = nodes
        .iter()
        .filter(|node| role(node) == "INSTANCE")
        .collect::<Vec<_>>();
    let transforms = nodes
        .iter()
        .filter(|node| role(node) == "TRANSFORM")
        .collect::<Vec<_>>();
    if assemblies.is_empty() && instances.is_empty() && transforms.is_empty() {
        return Ok(AssemblyStructure {
            assemblies: vec![],
            instances: vec![],
            transforms: vec![],
        });
    }
    if assemblies.is_empty() || instances.is_empty() {
        return Err(invalid_field(
            nodes.first(),
            "assembly structure is incomplete",
        ));
    }
    let profile = if builtin && !topsolid_builtin {
        crate::BuiltinProfileRegistry::compiled()?.profile_for_key(&crate::SchemaKey::parse(key)?)
    } else {
        None
    };
    let mut seen_role_types = BTreeSet::new();
    let first_role_nodes = nodes
        .iter()
        .filter(|node| matches!(role(node), "ASSEMBLY" | "INSTANCE" | "TRANSFORM"))
        .filter(|node| seen_role_types.insert(node.node_type))
        .map(|node| node.index)
        .collect::<BTreeSet<_>>();
    for node in assemblies.iter().chain(&instances).chain(&transforms) {
        validate_layout(
            node,
            role(node),
            profile.and_then(|profile| profile.definition(node.node_type)),
            first_role_nodes.contains(&node.index),
            fields,
        )?;
    }
    let assembly_indices = assemblies
        .iter()
        .map(|node| node.index)
        .collect::<BTreeSet<_>>();
    let instance_indices = instances
        .iter()
        .map(|node| node.index)
        .collect::<BTreeSet<_>>();
    let transform_indices = transforms
        .iter()
        .map(|node| node.index)
        .collect::<BTreeSet<_>>();
    let mut mapped_instances = Vec::with_capacity(instances.len());
    for node in &instances {
        let kind = byte(node, fields.instance_kind)?;
        if !matches!(kind, 1 | 2) {
            return Err(invalid_field(Some(node), "unsupported instance kind"));
        }
        let part = pointer(node, fields.instance_part)?;
        let part_node = by_index.get(&part).copied().ok_or_else(|| {
            invalid_reference(node, fields.instance_part, part, "BODY or ASSEMBLY")
        })?;
        let part_is_assembly = match role(part_node) {
            "ASSEMBLY" if assembly_indices.contains(&part) => true,
            "BODY" => false,
            _ => {
                return Err(invalid_reference(
                    node,
                    fields.instance_part,
                    part,
                    "BODY or ASSEMBLY",
                ));
            }
        };
        let assembly = pointer(node, fields.instance_assembly)?;
        if !assembly_indices.contains(&assembly) {
            return Err(invalid_reference(
                node,
                fields.instance_assembly,
                assembly,
                "ASSEMBLY",
            ));
        }
        let transform = optional_pointer(node, fields.instance_transform)?;
        if transform.is_some_and(|index| !transform_indices.contains(&index)) {
            return Err(invalid_reference(
                node,
                fields.instance_transform,
                transform.unwrap_or_default(),
                "TRANSFORM",
            ));
        }
        mapped_instances.push(InstanceNode {
            index: node.index,
            kind,
            part,
            part_is_assembly,
            transform,
            assembly,
            byte_range: node.byte_range.clone(),
        });
    }
    let instances_by_index = mapped_instances
        .iter()
        .map(|instance| (instance.index, instance))
        .collect::<BTreeMap<_, _>>();
    let mut mapped_assemblies = Vec::with_capacity(assemblies.len());
    for node in &assemblies {
        let kind = byte(node, fields.assembly_kind)?;
        if !matches!(kind, 1..=3) {
            return Err(invalid_field(Some(node), "unsupported assembly kind"));
        }
        let child = chain(
            node,
            &ChainFields {
                head: fields.assembly_sub_instances,
                next: fields.instance_next_in_assembly,
                previous: fields.instance_previous_in_assembly,
                child_chain: true,
            },
            &by_index,
            &instance_indices,
            &instances_by_index,
        )?;
        let referencing = chain(
            node,
            &ChainFields {
                head: fields.assembly_instances,
                next: fields.instance_next_of_part,
                previous: fields.instance_previous_of_part,
                child_chain: false,
            },
            &by_index,
            &instance_indices,
            &instances_by_index,
        )?;
        mapped_assemblies.push(AssemblyNode {
            index: node.index,
            kind,
            size_precision: double(node, fields.assembly_size_precision)?,
            linear_precision: double(node, fields.assembly_linear_precision)?,
            instances: child,
            referencing_instances: referencing,
            byte_range: node.byte_range.clone(),
        });
    }
    let mut child_links = BTreeSet::new();
    let mut assembly_part_links = BTreeSet::new();
    for assembly in &mapped_assemblies {
        for index in &assembly.instances {
            if !child_links.insert(*index) {
                return Err(invalid_reference(
                    assemblies[0],
                    fields.assembly_sub_instances,
                    *index,
                    "unique owning assembly",
                ));
            }
        }
        for index in &assembly.referencing_instances {
            if !assembly_part_links.insert(*index) {
                return Err(invalid_reference(
                    assemblies[0],
                    fields.assembly_instances,
                    *index,
                    "unique referenced assembly",
                ));
            }
        }
    }
    for instance in &mapped_instances {
        let node = by_index
            .get(&instance.index)
            .copied()
            .unwrap_or(assemblies[0]);
        if !child_links.contains(&instance.index) {
            return Err(invalid_reference(
                node,
                fields.instance_assembly,
                instance.assembly,
                "listed owning assembly",
            ));
        }
        if instance.part_is_assembly && !assembly_part_links.contains(&instance.index) {
            return Err(invalid_reference(
                node,
                fields.instance_part,
                instance.part,
                "listed assembly reference",
            ));
        }
    }
    let mut mapped_transforms = Vec::with_capacity(transforms.len());
    for node in &transforms {
        let rotation = array::<9>(node, "rotation_matrix")?;
        let translation = vector(node, fields.transform_translation)?;
        let scale = double(node, "scale")?;
        if scale == 0.0 {
            return Err(invalid_field(Some(node), "transform scale is zero"));
        }
        let flags = match value(node, "flag")? {
            FieldValue::Integer(Some(flags)) if *flags >= 0 => *flags,
            _ => return Err(invalid_field(Some(node), "invalid transform flags")),
        };
        let precision = precision_target(node, &by_index)?
            .map(|precision_node| {
                Ok(TransformPrecision {
                    index: precision_node.index,
                    rotation_correction: array::<9>(precision_node, "rotation_matrix")?,
                    translation_correction: vector(precision_node, "translation_vector")?,
                })
            })
            .transpose()?;
        mapped_transforms.push(TransformNode {
            index: node.index,
            rotation,
            translation,
            scale,
            flags,
            perspective: optional_vector(node, fields.transform_perspective)?,
            precision,
            byte_range: node.byte_range.clone(),
        });
    }
    Ok(AssemblyStructure {
        assemblies: mapped_assemblies,
        instances: mapped_instances,
        transforms: mapped_transforms,
    })
}

fn precision_target<'a>(
    transform: &RawNode,
    by_index: &BTreeMap<u32, &'a RawNode>,
) -> Result<Option<&'a RawNode>, ParseError> {
    let mut declarations = transform
        .definition
        .fields
        .iter()
        .filter(|field| field.name == "precision");
    let declaration = declarations.next();
    if declarations.next().is_some() {
        return Err(invalid_field(
            Some(transform),
            "invalid transform precision schema",
        ));
    }
    let mut fields = transform
        .fields
        .iter()
        .filter(|field| field.definition.name == "precision");
    let Some(field) = fields.next() else {
        return if declaration.is_none() {
            Ok(None)
        } else {
            Err(invalid_field(
                Some(transform),
                "transform precision link is missing",
            ))
        };
    };
    if fields.next().is_some()
        || declaration != Some(field.definition.as_ref())
        || field.definition.field_type != FieldType::PointerIndex
        || field.definition.element_count != 0
        || field.definition.pointer_class == 0
        || field.values.len() != 1
    {
        return Err(invalid_field(
            Some(transform),
            "invalid transform precision link",
        ));
    }
    let Some(FieldValue::PointerIndex(index)) = field.values.first() else {
        return Err(invalid_field(
            Some(transform),
            "invalid transform precision link",
        ));
    };
    let index = *index;
    if index == 0 {
        return Ok(None);
    }
    let Some(target) = by_index.get(&index).copied() else {
        return Ok(None);
    };
    if target.node_type != field.definition.pointer_class
        || target.definition.name != "TRANSFORM_PRECISION"
    {
        return Err(invalid_reference(
            transform,
            "precision",
            index,
            "TRANSFORM_PRECISION",
        ));
    }
    for (name, field_type, count) in [
        ("rotation_matrix", FieldType::Double, 9),
        ("translation_vector", FieldType::Vector, 0),
    ] {
        let target_field = named_field(target, name)?;
        if target_field.definition.field_type != field_type
            || target_field.definition.element_count != count
        {
            return Err(invalid_field(
                Some(target),
                "invalid transform precision fields",
            ));
        }
    }
    Ok(Some(target))
}

fn family_has_assembly_roles(provider: &SchemaProviderResolution, key: &str) -> bool {
    let SchemaProviderResolution::Builtin {
        profile_id,
        profile_revision,
        schema_key,
        coverage: crate::BuiltinProfileCoverage::FamilyInferred,
        profile_sha256,
    } = provider
    else {
        return false;
    };
    if key != schema_key {
        return false;
    }
    let Ok(requested_key) = crate::SchemaKey::parse(key) else {
        return false;
    };
    let Ok(registry) = crate::BuiltinProfileRegistry::compiled() else {
        return false;
    };
    let Some(family) = registry.profile(profile_id).filter(|family| {
        family.family_id() == Some(requested_key.family_id().as_str())
            && family.metadata().revision == *profile_revision
            && family.metadata().coverage == crate::BuiltinProfileCoverage::FamilyInferred
            && family.metadata().profile_sha256 == *profile_sha256
            && family.metadata().provider_schema == requested_key.provider_schema()
    }) else {
        return false;
    };
    family.members().iter().any(|member| {
        let Some(profile) = registry.profile(&member.profile_id) else {
            return false;
        };
        if profile.metadata().revision != member.revision
            || profile.metadata().profile_sha256 != member.profile_sha256
        {
            return false;
        }
        profile.accepted_schema_keys().any(|exact_key| {
            if exact_key.family_id() != requested_key.family_id() {
                return false;
            }
            matches!(
                (
                    member.profile_id.as_str(),
                    member.revision,
                    exact_key.raw(),
                    member.profile_sha256.as_str()
                ),
                (
                    "solidworks-3101290-31100-13006-r2",
                    2,
                    "SCH_3101290_31100_13006",
                    "bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b"
                ) | (
                    "solidworks-sch37102-13006-r3",
                    3,
                    "SCH_3701229_37102_13006",
                    "7300096c034365663a13f40c96a919d19b73d25b4c957f9dbb0e472c0a381730"
                ) | (
                    "icad-1901315-19008-13006-r2",
                    2,
                    "SCH_1901315_19008_13006",
                    "e9d776fdc6cd29bdc9b3bbbc05e0b75c10e287d515a5ac7c6cad01547639ded8"
                ) | (
                    "icad-2401000-20000-r3",
                    3,
                    "SCH_2401000_20000",
                    "fbdd2b2c31177b9fba0104c4a7d9ead562c38202aff91f2759f30101557fa53f"
                ) | (
                    "parasolid-2400164-20000-13006-r2",
                    2,
                    "SCH_2400164_20000_13006",
                    "ef75cfbf0f21455156dde0b6d346e3be92dd114f9eb2500f4a2e7f7dd9de39ae"
                ) | (
                    "nx-2600169-25001-13006-r2",
                    2,
                    "SCH_2600169_25001_13006",
                    "45a2e1d9ca4e48831c5e4c757978f0cde09e3d67c46d88392055f52ea031eea0"
                ) | (
                    "parasolid-2600217-25001-13006-r2",
                    2,
                    "SCH_2600217_25001_13006",
                    "99becb31c455144e4bb98e25fa97232518754b4ea9b6115c3533f554ad8ce18b"
                ) | (
                    "nx-3001232-30100-13006-r2",
                    2,
                    "SCH_3001232_30100_13006",
                    "a4d96295b72774062ffc6a7fb124664e1254f1df584748bbc5960897ec9b576b"
                ) | (
                    "nx-3400176-34001-13006-r2",
                    2,
                    "SCH_3400176_34001_13006",
                    "254c0340fcef549d67badc03c25653f518e06d7657b892308a48f1f4b3aabf05"
                )
            )
        })
    })
}

fn validate_layout(
    node: &RawNode,
    role: &str,
    reviewed: Option<&TypeDefinition>,
    first_of_type: bool,
    names: AssemblyFieldNames,
) -> Result<(), ParseError> {
    validate_expected_fields(node, role, reviewed, names)?;
    let precision_fields = node
        .definition
        .fields
        .iter()
        .filter(|field| field.name == "precision")
        .collect::<Vec<_>>();
    if !precision_fields.is_empty()
        && (precision_fields.len() != 1
            || precision_fields[0].field_type != FieldType::PointerIndex
            || precision_fields[0].element_count != 0
            || precision_fields[0].pointer_class == 0
            || reviewed.is_some_and(|base| {
                base.fields
                    .iter()
                    .filter(|field| field.name == "precision")
                    .count()
                    != 1
                    || base.fields.iter().find(|field| field.name == "precision")
                        != Some(precision_fields[0])
            }))
    {
        return Err(invalid_field(
            Some(node),
            "invalid transform precision schema",
        ));
    }
    if let Some(base) = reviewed {
        if first_of_type || node.first_schema.is_some() {
            let schema = node.first_schema.as_ref().ok_or_else(|| {
                invalid_field(
                    Some(node),
                    "first built-in assembly node has no schema provenance",
                )
            })?;
            if schema.definition.node_type != node.node_type
                || schema.definition.fields != node.definition.fields
                || base.node_type != node.node_type
                || schema.definition.source == SchemaSource::EmbeddedFull
                || !embedded_roles_are_copies(
                    node,
                    role,
                    base,
                    schema.definition.source,
                    &schema.edits,
                    names,
                )
            {
                return Err(invalid_field(
                    Some(node),
                    "assembly roles do not come from the reviewed base schema",
                ));
            }
        }
    }
    Ok(())
}

fn validate_expected_fields(
    node: &RawNode,
    role: &str,
    reviewed: Option<&TypeDefinition>,
    names: AssemblyFieldNames,
) -> Result<(), ParseError> {
    match role {
        "ASSEMBLY" => {
            for (name, field_type, count, pointer_class) in [
                (names.assembly_size_precision, FieldType::Double, 0, 0),
                (names.assembly_linear_precision, FieldType::Double, 0, 0),
                (names.assembly_instances, FieldType::PointerIndex, 0, 11),
                (names.assembly_kind, FieldType::UnsignedByte, 0, 0),
                (names.assembly_sub_instances, FieldType::PointerIndex, 0, 11),
            ] {
                validate_expected_field(node, reviewed, name, field_type, count, pointer_class)?;
            }
        }
        "INSTANCE" => {
            for (name, field_type, count, pointer_class) in [
                (names.instance_kind, FieldType::UnsignedByte, 0, 0),
                (names.instance_part, FieldType::PointerIndex, 0, 1005),
                (names.instance_transform, FieldType::PointerIndex, 0, 100),
                (names.instance_assembly, FieldType::PointerIndex, 0, 10),
                (
                    names.instance_next_in_assembly,
                    FieldType::PointerIndex,
                    0,
                    11,
                ),
                (
                    names.instance_previous_in_assembly,
                    FieldType::PointerIndex,
                    0,
                    11,
                ),
                (names.instance_next_of_part, FieldType::PointerIndex, 0, 11),
                (
                    names.instance_previous_of_part,
                    FieldType::PointerIndex,
                    0,
                    11,
                ),
            ] {
                validate_expected_field(node, reviewed, name, field_type, count, pointer_class)?;
            }
        }
        "TRANSFORM" => {
            for (name, field_type, count, pointer_class) in [
                ("rotation_matrix", FieldType::Double, 9, 0),
                (names.transform_translation, FieldType::Vector, 0, 0),
                ("scale", FieldType::Double, 0, 0),
                ("flag", FieldType::Integer, 0, 0),
                (names.transform_perspective, FieldType::Vector, 0, 0),
            ] {
                validate_expected_field(node, reviewed, name, field_type, count, pointer_class)?;
            }
        }
        _ => return Err(invalid_field(Some(node), "unknown assembly node role")),
    }
    Ok(())
}

fn validate_expected_field(
    node: &RawNode,
    reviewed: Option<&TypeDefinition>,
    name: &'static str,
    field_type: FieldType,
    count: u32,
    pointer_class: u16,
) -> Result<(), ParseError> {
    let field = named_field(node, name)?;
    if node
        .definition
        .fields
        .iter()
        .filter(|f| f.name == name)
        .count()
        != 1
        || field.definition.field_type != field_type
        || field.definition.element_count != count
        || field.definition.pointer_class != pointer_class
        || reviewed.is_some_and(|base| {
            base.fields
                .iter()
                .filter(|field| field.name == name)
                .count()
                != 1
                || base.fields.iter().find(|field| field.name == name)
                    != Some(field.definition.as_ref())
        })
    {
        return Err(invalid_field(
            Some(node),
            "assembly field does not match the reviewed schema",
        ));
    }
    Ok(())
}

fn embedded_roles_are_copies(
    node: &RawNode,
    role: &str,
    base: &TypeDefinition,
    source: SchemaSource,
    edits: &[SchemaEdit],
    fields: AssemblyFieldNames,
) -> bool {
    let names: &[&str] = match role {
        "ASSEMBLY" => &[
            fields.assembly_size_precision,
            fields.assembly_linear_precision,
            fields.assembly_instances,
            fields.assembly_kind,
            fields.assembly_sub_instances,
        ],
        "INSTANCE" => &[
            fields.instance_kind,
            fields.instance_part,
            fields.instance_transform,
            fields.instance_assembly,
            fields.instance_next_in_assembly,
            fields.instance_next_of_part,
        ],
        "TRANSFORM" => &[
            "rotation_matrix",
            fields.transform_translation,
            "scale",
            "flag",
            fields.transform_perspective,
        ],
        _ => return false,
    };
    match source {
        SchemaSource::Base | SchemaSource::EmbeddedUnchanged => names.iter().all(|name| {
            base.fields.iter().find(|field| field.name == *name)
                == node
                    .definition
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
        }),
        SchemaSource::EmbeddedDelta => {
            let mut base_index = 0;
            let mut output_index = 0;
            let mut copied = BTreeSet::new();
            for edit in edits {
                match edit {
                    SchemaEdit::Copy { .. } => {
                        let Some(base_field) = base.fields.get(base_index) else {
                            return false;
                        };
                        if names.contains(&base_field.name.as_str())
                            && node.definition.fields.get(output_index) == Some(base_field)
                        {
                            copied.insert(base_field.name.as_str());
                        }
                        base_index += 1;
                        output_index += 1;
                    }
                    SchemaEdit::Delete { .. } => base_index += 1,
                    SchemaEdit::Insert { .. } | SchemaEdit::Append { .. } => output_index += 1,
                    SchemaEdit::End { .. } => break,
                }
            }
            names.iter().all(|name| copied.contains(name))
        }
        SchemaSource::EmbeddedFull => false,
    }
}

struct ChainFields {
    head: &'static str,
    next: &'static str,
    previous: &'static str,
    child_chain: bool,
}

fn chain(
    owner: &RawNode,
    fields: &ChainFields,
    by_index: &BTreeMap<u32, &RawNode>,
    instances: &BTreeSet<u32>,
    mapped: &BTreeMap<u32, &InstanceNode>,
) -> Result<Vec<u32>, ParseError> {
    let mut index = pointer(owner, fields.head)?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    let mut previous = 0;
    while index != 0 {
        if !instances.contains(&index) || !seen.insert(index) {
            return Err(invalid_reference(
                owner,
                fields.head,
                index,
                "acyclic INSTANCE chain",
            ));
        }
        let instance = mapped
            .get(&index)
            .ok_or_else(|| invalid_reference(owner, fields.head, index, "INSTANCE"))?;
        if (fields.child_chain && instance.assembly != owner.index)
            || (!fields.child_chain && (!instance.part_is_assembly || instance.part != owner.index))
        {
            return Err(invalid_reference(
                owner,
                fields.head,
                index,
                "matching INSTANCE ownership",
            ));
        }
        let node = by_index
            .get(&index)
            .copied()
            .ok_or_else(|| invalid_reference(owner, fields.head, index, "INSTANCE"))?;
        let previous_index = pointer(node, fields.previous)?;
        if previous_index != previous {
            return Err(invalid_reference(
                owner,
                fields.previous,
                previous_index,
                "inverse INSTANCE link",
            ));
        }
        result.push(index);
        index = pointer(node, fields.next)?;
        previous = node.index;
    }
    Ok(result)
}

fn named_field<'a>(node: &'a RawNode, name: &'static str) -> Result<&'a RawField, ParseError> {
    let mut fields = node
        .fields
        .iter()
        .filter(|field| field.definition.name == name);
    let field = fields
        .next()
        .ok_or_else(|| invalid_field(Some(node), "required assembly field is missing"))?;
    if fields.next().is_some() {
        return Err(invalid_field(
            Some(node),
            "assembly field name is duplicated",
        ));
    }
    Ok(field)
}
fn value<'a>(node: &'a RawNode, name: &'static str) -> Result<&'a FieldValue, ParseError> {
    let field = named_field(node, name)?;
    if field.values.len() != 1 {
        return Err(invalid_field(
            Some(node),
            "assembly scalar has an invalid count",
        ));
    }
    field
        .values
        .first()
        .ok_or_else(|| invalid_field(Some(node), "assembly scalar is empty"))
}
fn pointer(node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
    match value(node, name)? {
        FieldValue::PointerIndex(index) => Ok(*index),
        _ => Err(invalid_field(
            Some(node),
            "assembly pointer has the wrong type",
        )),
    }
}
fn optional_pointer(node: &RawNode, name: &'static str) -> Result<Option<u32>, ParseError> {
    pointer(node, name).map(|index| (index != 0).then_some(index))
}
fn byte(node: &RawNode, name: &'static str) -> Result<u8, ParseError> {
    match value(node, name)? {
        FieldValue::UnsignedByte(value) => Ok(*value),
        _ => Err(invalid_field(
            Some(node),
            "assembly code has the wrong type",
        )),
    }
}
fn double(node: &RawNode, name: &'static str) -> Result<f64, ParseError> {
    match value(node, name)? {
        FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
        _ => Err(invalid_field(
            Some(node),
            "assembly real is null or non-finite",
        )),
    }
}
fn vector(node: &RawNode, name: &'static str) -> Result<[f64; 3], ParseError> {
    match value(node, name)? {
        FieldValue::Vector([Some(x), Some(y), Some(z)])
            if x.is_finite() && y.is_finite() && z.is_finite() =>
        {
            Ok([*x, *y, *z])
        }
        _ => Err(invalid_field(
            Some(node),
            "transform vector is null or non-finite",
        )),
    }
}
fn optional_vector(node: &RawNode, name: &'static str) -> Result<[Option<f64>; 3], ParseError> {
    match value(node, name)? {
        FieldValue::Vector(values) if values.iter().flatten().all(|value| value.is_finite()) => {
            Ok(*values)
        }
        _ => Err(invalid_field(
            Some(node),
            "transform perspective has the wrong type",
        )),
    }
}
fn array<const N: usize>(node: &RawNode, name: &'static str) -> Result<[f64; N], ParseError> {
    let field = named_field(node, name)?;
    if field.values.len() != N {
        return Err(invalid_field(
            Some(node),
            "transform matrix has an invalid count",
        ));
    }
    let values = field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
            _ => Err(invalid_field(
                Some(node),
                "transform matrix has a null or non-finite value",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    values
        .try_into()
        .map_err(|_| invalid_field(Some(node), "transform matrix has an invalid count"))
}
fn invalid_field(node: Option<&RawNode>, message: &str) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepField,
        node.map_or(0, |node| node.byte_range.start),
        message,
        node.map_or(ErrorDetails::None, |node| ErrorDetails::BrepField {
            node_index: node.index,
            field: "assembly",
        }),
    )
}
fn unique_by_index<T>(items: &[T], index: u32, get_index: impl Fn(&T) -> u32) -> Option<&T> {
    let mut matches = items.iter().filter(|item| get_index(item) == index);
    let item = matches.next()?;
    matches.next().is_none().then_some(item)
}
fn invalid_lookup(index: u32, expected_type: &'static str) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepReference,
        0,
        "assembly placement reference is unresolved or ambiguous",
        ErrorDetails::BrepReference {
            node_index: index,
            field: "instance_path",
            target_index: index,
            expected_type,
        },
    )
}
fn invalid_reference(
    node: &RawNode,
    field: &'static str,
    target: u32,
    expected_type: &'static str,
) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepReference,
        node.byte_range.start,
        "assembly pointer is unresolved or targets the wrong node type",
        ErrorDetails::BrepReference {
            node_index: node.index,
            field,
            target_index: target,
            expected_type,
        },
    )
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "Fixture indices and values are bounded and kept explicit for regression checks."
)]
#[allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::brep::{
        Body, BodyKind, BrepMetrics, BrepModel, BrepModelWithResolvedBodyHierarchy,
        BrepSourceFormat, Face, MappedBodyChildren, RawIntegerPool, RawMeshData, RawVectorPool,
        Region, RegionKind, Sense, Shell, SourceNodeRef, SurfaceGeometry, SurfaceKind,
        TopologyValidation,
    };
    use crate::{FieldDefinition, SchemaSource, TypeDefinition};

    fn node(
        index: u32,
        name: &str,
        fields: Vec<(&str, FieldType, u32, u16, Vec<FieldValue>)>,
    ) -> RawNode {
        RawNode {
            node_type: match name {
                "ASSEMBLY" => 10,
                "INSTANCE" => 11,
                "TRANSFORM" => 100,
                "TRANSFORM_PRECISION" => 229,
                "BODY" => 12,
                _ => 0,
            },
            index,
            variable_length: None,
            definition: TypeDefinition::from_fields(
                0,
                name,
                name,
                fields
                    .iter()
                    .map(
                        |(name, field_type, count, pointer_class, _)| FieldDefinition {
                            name: (*name).into(),
                            field_type: *field_type,
                            pointer_class: *pointer_class,
                            element_count: *count,
                            transmitted: true,
                        },
                    )
                    .collect(),
                SchemaSource::Base,
            )
            .into(),
            first_schema: None,
            fields: fields
                .into_iter()
                .map(
                    |(name, field_type, count, pointer_class, values)| RawField {
                        definition: FieldDefinition {
                            name: name.into(),
                            field_type,
                            pointer_class,
                            element_count: count,
                            transmitted: true,
                        }
                        .into(),
                        values,
                        byte_range: 0..0,
                    },
                )
                .collect(),
            byte_range: index as usize..index as usize,
            user_fields: vec![],
        }
    }

    fn pointer(
        name: &'static str,
        value: u32,
        class: u16,
    ) -> (&'static str, FieldType, u32, u16, Vec<FieldValue>) {
        (
            name,
            FieldType::PointerIndex,
            0,
            class,
            vec![FieldValue::PointerIndex(value)],
        )
    }
    fn set_pointer(nodes: &mut [RawNode], node_index: u32, field_name: &str, value: u32) {
        let field = nodes
            .iter_mut()
            .find(|node| node.index == node_index)
            .unwrap()
            .fields
            .iter_mut()
            .find(|field| field.definition.name == field_name)
            .unwrap();
        field.values[0] = FieldValue::PointerIndex(value);
    }
    fn raw_pointer(node: &RawNode, field_name: &str) -> u32 {
        match &node
            .fields
            .iter()
            .find(|field| field.definition.name == field_name)
            .unwrap()
            .values[0]
        {
            FieldValue::PointerIndex(index) => *index,
            _ => unreachable!(),
        }
    }
    fn byte(name: &'static str, value: u8) -> (&'static str, FieldType, u32, u16, Vec<FieldValue>) {
        (
            name,
            FieldType::UnsignedByte,
            0,
            0,
            vec![FieldValue::UnsignedByte(value)],
        )
    }
    fn real(
        name: &'static str,
        value: f64,
    ) -> (&'static str, FieldType, u32, u16, Vec<FieldValue>) {
        (
            name,
            FieldType::Double,
            0,
            0,
            vec![FieldValue::Double(Some(value))],
        )
    }

    fn attach_precision(nodes: &mut [RawNode], target: u32, class: u16) {
        let transform = nodes.iter_mut().find(|node| node.index == 3).unwrap();
        let definition = FieldDefinition {
            name: "precision".into(),
            field_type: FieldType::PointerIndex,
            pointer_class: class,
            element_count: 0,
            transmitted: true,
        };
        std::sync::Arc::make_mut(&mut transform.definition)
            .fields
            .push(definition.clone());
        transform.fields.push(RawField {
            definition: definition.into(),
            values: vec![FieldValue::PointerIndex(target)],
            byte_range: 0..0,
        });
    }

    fn precision_node(index: u32) -> RawNode {
        node(
            index,
            "TRANSFORM_PRECISION",
            vec![
                (
                    "rotation_matrix",
                    FieldType::Double,
                    9,
                    0,
                    (1..=9)
                        .map(|value| FieldValue::Double(Some(f64::from(value) / 100.0)))
                        .collect(),
                ),
                (
                    "translation_vector",
                    FieldType::Vector,
                    0,
                    0,
                    vec![FieldValue::Vector([Some(0.1), Some(0.2), Some(0.3)])],
                ),
            ],
        )
    }

    fn placement_transform(index: u32, rotation: [f64; 9], translation: [f64; 3]) -> TransformNode {
        TransformNode {
            index,
            rotation,
            translation,
            scale: 1.0,
            flags: 0,
            perspective: [None; 3],
            precision: None,
            byte_range: 0..0,
        }
    }

    fn plain_assembly(
        index: u32,
        instances: Vec<u32>,
        referencing_instances: Vec<u32>,
    ) -> AssemblyNode {
        AssemblyNode {
            index,
            kind: 1,
            size_precision: 0.0,
            linear_precision: 0.0,
            instances,
            referencing_instances,
            byte_range: 0..0,
        }
    }

    fn plain_instance(
        index: u32,
        part: u32,
        part_is_assembly: bool,
        assembly: u32,
    ) -> InstanceNode {
        InstanceNode {
            index,
            kind: 1,
            part,
            part_is_assembly,
            transform: None,
            assembly,
            byte_range: 0..0,
        }
    }

    fn body_only_brep(source_indices: &[u32]) -> BrepModel {
        let source = |node_index| SourceNodeRef {
            node_index,
            node_type: 0,
            type_name: "BODY".into(),
            node_id: None,
            byte_range: 0..0,
        };
        BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: String::new(),
            complete: true,
            bodies: source_indices
                .iter()
                .enumerate()
                .map(|(id, node_index)| Body {
                    id: id as u32 + 1,
                    kind: BodyKind::Solid,
                    size_resolution: 0.0,
                    linear_resolution: 0.0,
                    regions: vec![],
                    edges: vec![],
                    vertices: vec![],
                    attributes: vec![],
                    source: source(*node_index),
                })
                .collect(),
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            points: vec![],
            curves: vec![],
            surfaces: vec![],
            topology: TopologyValidation {
                valid: true,
                closed_loop_count: 0,
                closed_edge_ring_count: 0,
                euler_characteristic: 0,
            },
            metrics: BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            diagnostics: vec![],
        }
    }

    fn fixture(transform: u32) -> Vec<RawNode> {
        vec![
            node(
                1,
                "ASSEMBLY",
                vec![
                    real("size_precision", 0.01),
                    real("linear_precision", 0.02),
                    pointer("instances", 0, 11),
                    byte("assembly_kind", 1),
                    pointer("sub_instances", 2, 11),
                ],
            ),
            node(
                2,
                "INSTANCE",
                vec![
                    byte("instance_kind", 1),
                    pointer("part_ref", 4, 1005),
                    pointer("transform_ref", transform, 100),
                    pointer("assembly_ref", 1, 10),
                    pointer("next_in_assembly", 0, 11),
                    pointer("next_of_part", 0, 11),
                    pointer("previous_in_assembly", 0, 11),
                    pointer("previous_of_part", 0, 11),
                ],
            ),
            node(
                3,
                "TRANSFORM",
                vec![
                    (
                        "rotation_matrix",
                        FieldType::Double,
                        9,
                        0,
                        (1..=9)
                            .map(|value| FieldValue::Double(Some(f64::from(value))))
                            .collect(),
                    ),
                    (
                        "translation",
                        FieldType::Vector,
                        0,
                        0,
                        vec![FieldValue::Vector([Some(10.0), Some(20.0), Some(30.0)])],
                    ),
                    real("scale", 2.0),
                    (
                        "flag",
                        FieldType::Integer,
                        0,
                        0,
                        vec![FieldValue::Integer(Some(7))],
                    ),
                    (
                        "perspective",
                        FieldType::Vector,
                        0,
                        0,
                        vec![FieldValue::Vector([None, None, None])],
                    ),
                ],
            ),
            node(4, "BODY", vec![]),
        ]
    }

    #[test]
    fn maps_typed_chains_and_retains_uncomposed_transform_values() -> Result<(), ParseError> {
        let result = map_assembly(
            &fixture(3),
            &SchemaProviderResolution::CallerSupplied,
            "custom",
        )?;
        assert_eq!(result.assemblies[0].instances, vec![2]);
        assert_eq!(result.instances[0].part, 4);
        assert_eq!(result.instances[0].transform, Some(3));
        assert_eq!(
            result.transforms[0].rotation,
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]
        );
        assert_eq!(result.transforms[0].translation, [10.0, 20.0, 30.0]);
        assert_eq!(result.transforms[0].scale, 2.0);
        assert_eq!(result.transforms[0].perspective, [None, None, None]);
        assert_eq!(result.transforms[0].precision, None);
        Ok(())
    }

    #[test]
    fn rejects_duplicate_transmit_indices_without_mutating_input() {
        let mut nodes = fixture(3);
        nodes.push(node(3, "BODY", vec![]));
        let original = nodes.clone();
        let error = map_assembly(&nodes, &SchemaProviderResolution::CallerSupplied, "custom")
            .expect_err("duplicate transmit indexes are ambiguous");
        assert_eq!(error.kind(), ErrorKind::DuplicateNodeIndex);
        assert_eq!(nodes, original);
    }

    #[test]
    fn maps_null_transform_and_missing_precision_as_effective_null() -> Result<(), ParseError> {
        let mut nodes = fixture(0);
        attach_precision(&mut nodes, 98, 229);
        let result = map_assembly(&nodes, &SchemaProviderResolution::CallerSupplied, "custom")?;
        assert_eq!(result.instances[0].transform, None);
        assert_eq!(result.transforms[0].precision, None);
        assert_eq!(raw_pointer(&nodes[1], "transform_ref"), 0);
        assert_eq!(raw_pointer(&nodes[2], "precision"), 98);

        let result = map_assembly(
            &fixture(0),
            &SchemaProviderResolution::CallerSupplied,
            "custom",
        )?;
        assert_eq!(result.instances[0].transform, None);

        let mut null_precision = fixture(3);
        attach_precision(&mut null_precision, 0, 229);
        let result = map_assembly(
            &null_precision,
            &SchemaProviderResolution::CallerSupplied,
            "custom",
        )?;
        assert_eq!(result.transforms[0].precision, None);
        Ok(())
    }

    #[test]
    fn rejects_missing_assembly_chain_links_wrong_type_or_cycle() {
        for (node, field) in [
            (1, "instances"),
            (1, "sub_instances"),
            (2, "next_in_assembly"),
            (2, "previous_in_assembly"),
            (2, "next_of_part"),
            (2, "previous_of_part"),
        ] {
            let mut nodes = fixture(3);
            // Part-chain links are traversed only when the instance references an assembly.
            if field.ends_with("of_part") {
                let mut referenced_assembly = nodes[0].clone();
                referenced_assembly.index = 5;
                nodes.push(referenced_assembly);
                set_pointer(&mut nodes, 2, "part_ref", 5);
                set_pointer(&mut nodes, 5, "instances", 2);
                set_pointer(&mut nodes, 5, "sub_instances", 0);
            }
            set_pointer(&mut nodes, node, field, 99);
            assert_eq!(
                map_assembly(&nodes, &SchemaProviderResolution::CallerSupplied, "custom")
                    .err()
                    .map(|error| error.kind()),
                Some(ErrorKind::InvalidBrepReference),
                "missing {field} must be rejected"
            );
        }

        let mut wrong_type = fixture(3);
        set_pointer(&mut wrong_type, 1, "instances", 4);
        assert!(
            map_assembly(
                &wrong_type,
                &SchemaProviderResolution::CallerSupplied,
                "custom"
            )
            .is_err()
        );

        assert!(
            map_assembly(
                &fixture(4),
                &SchemaProviderResolution::CallerSupplied,
                "custom"
            )
            .is_err()
        );

        let mut cycle = fixture(3);
        set_pointer(&mut cycle, 2, "next_in_assembly", 2);
        assert!(map_assembly(&cycle, &SchemaProviderResolution::CallerSupplied, "custom").is_err());
    }

    #[test]
    fn rejects_previous_instance_links_with_wrong_pointer_classes() -> Result<(), std::io::Error> {
        for field_name in ["previous_in_assembly", "previous_of_part"] {
            let mut nodes = fixture(3);
            let instance = nodes
                .iter_mut()
                .find(|node| node.index == 2)
                .ok_or_else(|| std::io::Error::other("instance node is missing"))?;
            std::sync::Arc::make_mut(&mut instance.definition)
                .fields
                .iter_mut()
                .find(|field| field.name == field_name)
                .ok_or_else(|| std::io::Error::other("instance field is missing"))?
                .pointer_class = 12;
            std::sync::Arc::make_mut(
                &mut instance
                    .fields
                    .iter_mut()
                    .find(|field| field.definition.name == field_name)
                    .ok_or_else(|| std::io::Error::other("instance field value is missing"))?
                    .definition,
            )
            .pointer_class = 12;

            assert!(
                map_assembly(&nodes, &SchemaProviderResolution::CallerSupplied, "custom").is_err(),
                "incorrect pointer class is rejected for {field_name}"
            );
        }
        Ok(())
    }

    #[test]
    fn maps_referenced_precision_corrections_without_replacing_raw_transform()
    -> Result<(), ParseError> {
        let mut nodes = fixture(3);
        attach_precision(&mut nodes, 5, 229);
        nodes.push(precision_node(5));
        let result = map_assembly(&nodes, &SchemaProviderResolution::CallerSupplied, "custom")?;
        let transform = &result.transforms[0];
        assert_eq!(transform.rotation[0], 1.0);
        assert_eq!(transform.translation, [10.0, 20.0, 30.0]);
        assert_eq!(
            transform.precision,
            Some(TransformPrecision {
                index: 5,
                rotation_correction: [0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08, 0.09,],
                translation_correction: [0.1, 0.2, 0.3],
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_precision_links_to_wrong_node_types_and_wrong_classes() {
        let mut wrong_target = fixture(3);
        attach_precision(&mut wrong_target, 4, 229);
        assert!(
            map_assembly(
                &wrong_target,
                &SchemaProviderResolution::CallerSupplied,
                "custom",
            )
            .is_err()
        );

        let mut wrong_class = fixture(3);
        attach_precision(&mut wrong_class, 5, 100);
        wrong_class.push(precision_node(5));
        assert!(
            map_assembly(
                &wrong_class,
                &SchemaProviderResolution::CallerSupplied,
                "custom",
            )
            .is_err()
        );
    }

    #[test]
    fn places_points_leaf_to_root_through_noncommuting_transforms() {
        let quarter_turn = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let structure = AssemblyStructure {
            assemblies: vec![
                AssemblyNode {
                    index: 1,
                    kind: 1,
                    size_precision: 0.0,
                    linear_precision: 0.0,
                    instances: vec![11],
                    referencing_instances: vec![],
                    byte_range: 0..0,
                },
                AssemblyNode {
                    index: 2,
                    kind: 1,
                    size_precision: 0.0,
                    linear_precision: 0.0,
                    instances: vec![10],
                    referencing_instances: vec![11],
                    byte_range: 0..0,
                },
            ],
            instances: vec![
                InstanceNode {
                    index: 10,
                    kind: 1,
                    part: 99,
                    part_is_assembly: false,
                    transform: Some(100),
                    assembly: 2,
                    byte_range: 0..0,
                },
                InstanceNode {
                    index: 11,
                    kind: 1,
                    part: 2,
                    part_is_assembly: true,
                    transform: Some(101),
                    assembly: 1,
                    byte_range: 0..0,
                },
            ],
            transforms: vec![
                placement_transform(100, quarter_turn, [1.0, 0.0, 0.0]),
                placement_transform(101, quarter_turn, [0.0, 2.0, 0.0]),
            ],
        };

        assert_eq!(
            structure.place_point([1.0, 0.0, 0.0], &[10, 11]),
            Ok([-1.0, 3.0, 0.0])
        );
        assert_ne!(
            structure.place_point([1.0, 0.0, 0.0], &[11, 10]),
            Ok([-1.0, 3.0, 0.0])
        );
    }

    #[test]
    fn applies_precision_once_and_handles_missing_transform_as_identity() {
        let mut transform =
            placement_transform(3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], [0.0; 3]);
        transform.precision = Some(TransformPrecision {
            index: 4,
            rotation_correction: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            translation_correction: [0.0, 1.0, 0.0],
        });
        assert_eq!(
            transform.transform_point([0.0, 1.0, 0.0]),
            Ok([1.0, 2.0, 0.0])
        );
        assert_eq!(
            transform.transform_point([0.0, 1.0, 0.0]),
            Ok([1.0, 2.0, 0.0])
        );
        assert_eq!(transform.rotation[1], 0.0);
        assert_eq!(transform.translation, [0.0; 3]);

        let structure = AssemblyStructure {
            assemblies: vec![AssemblyNode {
                index: 1,
                kind: 1,
                size_precision: 0.0,
                linear_precision: 0.0,
                instances: vec![2],
                referencing_instances: vec![],
                byte_range: 0..0,
            }],
            instances: vec![InstanceNode {
                index: 2,
                kind: 1,
                part: 3,
                part_is_assembly: false,
                transform: None,
                assembly: 1,
                byte_range: 0..0,
            }],
            transforms: vec![],
        };
        assert_eq!(
            structure.place_point([3.0, 4.0, 5.0], &[2]),
            Ok([3.0, 4.0, 5.0])
        );
        assert_eq!(
            structure.place_point([3.0, 4.0, 5.0], &[]),
            Ok([3.0, 4.0, 5.0])
        );
        assert!(structure.place_point([3.0, 4.0, 5.0], &[99]).is_err());
    }

    #[test]
    fn places_triangle_through_nested_reflection_without_changing_vertex_order()
    -> Result<(), ParseError> {
        let leaf_reflection = placement_transform(
            100,
            [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        );
        let root_translation = placement_transform(101, identity_matrix(), [0.0, 2.0, 0.0]);
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![20], vec![]),
                plain_assembly(2, vec![10], vec![20]),
            ],
            instances: vec![
                InstanceNode {
                    transform: Some(101),
                    ..plain_instance(20, 2, true, 1)
                },
                InstanceNode {
                    transform: Some(100),
                    ..plain_instance(10, 3, false, 2)
                },
            ],
            transforms: vec![leaf_reflection, root_translation],
        };
        let input = [[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]];
        let placed = structure.place_triangles(&input, &[10, 20])?;
        assert_eq!(input[0][0], [1.0, 0.0, 0.0]);
        assert_eq!(
            placed,
            vec![[[0.0, 2.0, 0.0], [1.0, 3.0, 0.0], [1.0, 2.0, 1.0]]]
        );
        Ok(())
    }

    #[test]
    fn tessellates_body_instance_by_source_index_and_rejects_invalid_paths() {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![20], vec![]),
                plain_assembly(2, vec![10], vec![20]),
            ],
            instances: vec![
                plain_instance(20, 2, true, 1),
                plain_instance(10, 50, false, 2),
            ],
            transforms: vec![],
        };
        let brep = body_only_brep(&[50]);
        assert_eq!(
            structure.tessellate_body_instance(&brep, &[10, 20], 0.1),
            Ok(vec![])
        );

        assert!(structure.tessellate_body_instance(&brep, &[], 0.1).is_err());
        assert!(
            structure
                .tessellate_body_instance(&brep, &[20, 10], 0.1)
                .is_err()
        );
        assert!(
            structure
                .tessellate_body_instance(&body_only_brep(&[51]), &[10, 20], 0.1)
                .is_err()
        );
        assert!(
            structure
                .tessellate_body_instance(&body_only_brep(&[50, 50]), &[10, 20], 0.1)
                .is_err()
        );
    }

    #[test]
    fn rejects_nonfinite_triangle_vertices_and_invalid_instance_paths() {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![20], vec![]),
                plain_assembly(2, vec![10], vec![20]),
            ],
            instances: vec![
                plain_instance(20, 2, true, 1),
                plain_instance(10, 3, false, 2),
            ],
            transforms: vec![],
        };
        let triangle = [[[0.0, 0.0, 0.0]; 3]];
        assert!(structure.place_triangles(&triangle, &[10, 20]).is_ok());
        assert!(structure.place_triangles(&triangle, &[20, 10]).is_err());
        let nonfinite = [[[f64::INFINITY, 0.0, 0.0]; 3]];
        assert!(structure.place_triangles(&nonfinite, &[10, 20]).is_err());

        let mut bad_transform = placement_transform(100, identity_matrix(), [0.0; 3]);
        bad_transform.perspective[0] = Some(1.0);
        let with_bad_transform = AssemblyStructure {
            transforms: vec![bad_transform],
            instances: vec![InstanceNode {
                transform: Some(100),
                ..plain_instance(10, 3, false, 2)
            }],
            ..structure.clone()
        };
        assert!(
            with_bad_transform
                .place_triangles(&triangle, &[10])
                .is_err()
        );

        let mut overflow_transform = placement_transform(101, identity_matrix(), [0.0; 3]);
        overflow_transform.scale = f64::MAX;
        let with_overflow = AssemblyStructure {
            transforms: vec![overflow_transform],
            instances: vec![InstanceNode {
                transform: Some(101),
                ..plain_instance(10, 3, false, 2)
            }],
            ..structure
        };
        let large = [[[2.0, 0.0, 0.0]; 3]];
        assert!(with_overflow.place_triangles(&large, &[10]).is_err());
    }

    #[test]
    fn rejects_perspective_nonfinite_points_and_broken_links() {
        let mut perspective =
            placement_transform(3, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], [0.0; 3]);
        perspective.perspective[0] = Some(0.0);
        assert_eq!(perspective.transform_point([0.0; 3]), Ok([0.0; 3]));
        perspective.perspective[0] = Some(1.0);
        assert!(perspective.transform_point([0.0; 3]).is_err());
        perspective.perspective[0] = Some(f64::INFINITY);
        assert!(perspective.transform_point([0.0; 3]).is_err());
        assert!(
            placement_transform(3, [1.0; 9], [0.0; 3])
                .transform_point([f64::INFINITY, 0.0, 0.0])
                .is_err()
        );

        let mut structure = AssemblyStructure {
            assemblies: vec![AssemblyNode {
                index: 1,
                kind: 1,
                size_precision: 0.0,
                linear_precision: 0.0,
                instances: vec![2],
                referencing_instances: vec![],
                byte_range: 0..0,
            }],
            instances: vec![InstanceNode {
                index: 2,
                kind: 1,
                part: 3,
                part_is_assembly: false,
                transform: Some(99),
                assembly: 1,
                byte_range: 0..0,
            }],
            transforms: vec![],
        };
        assert!(structure.place_point([0.0; 3], &[2]).is_err());
        structure.instances[0].transform = None;
        assert!(structure.place_point([0.0; 3], &[2, 2]).is_err());
    }

    #[test]
    fn accepts_only_reviewed_exact_assembly_profiles() -> Result<(), Box<dyn std::error::Error>> {
        for (profile_id, revision, key, digest) in [
            (
                "solidworks-3101290-31100-13006-r2",
                2,
                "SCH_3101290_31100_13006",
                "bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b",
            ),
            (
                "solidworks-sch37102-13006-r3",
                3,
                "SCH_3701229_37102_13006",
                "7300096c034365663a13f40c96a919d19b73d25b4c957f9dbb0e472c0a381730",
            ),
            (
                "icad-1901315-19008-13006-r2",
                2,
                "SCH_1901315_19008_13006",
                "e9d776fdc6cd29bdc9b3bbbc05e0b75c10e287d515a5ac7c6cad01547639ded8",
            ),
            (
                "icad-2401000-20000-r3",
                3,
                "SCH_2401000_20000",
                "fbdd2b2c31177b9fba0104c4a7d9ead562c38202aff91f2759f30101557fa53f",
            ),
            (
                "parasolid-2400164-20000-13006-r2",
                2,
                "SCH_2400164_20000_13006",
                "ef75cfbf0f21455156dde0b6d346e3be92dd114f9eb2500f4a2e7f7dd9de39ae",
            ),
            (
                "nx-2600169-25001-13006-r2",
                2,
                "SCH_2600169_25001_13006",
                "45a2e1d9ca4e48831c5e4c757978f0cde09e3d67c46d88392055f52ea031eea0",
            ),
            (
                "parasolid-2600217-25001-13006-r2",
                2,
                "SCH_2600217_25001_13006",
                "99becb31c455144e4bb98e25fa97232518754b4ea9b6115c3533f554ad8ce18b",
            ),
            (
                "nx-3001232-30100-13006-r2",
                2,
                "SCH_3001232_30100_13006",
                "a4d96295b72774062ffc6a7fb124664e1254f1df584748bbc5960897ec9b576b",
            ),
            (
                "nx-3400176-34001-13006-r2",
                2,
                "SCH_3400176_34001_13006",
                "254c0340fcef549d67badc03c25653f518e06d7657b892308a48f1f4b3aabf05",
            ),
        ] {
            let provider = SchemaProviderResolution::Builtin {
                profile_id: profile_id.into(),
                profile_revision: revision,
                schema_key: key.into(),
                coverage: crate::BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256: digest.into(),
            };
            let registry = crate::BuiltinProfileRegistry::compiled()?;
            let profile = registry
                .profile_for_key(&crate::SchemaKey::parse(key)?)
                .ok_or_else(|| std::io::Error::other("registered profile is missing"))?;
            let mut nodes = fixture(3);
            for node in nodes.iter_mut().filter(|node| node.node_type != 12) {
                let definition = profile
                    .definition(node.node_type)
                    .ok_or_else(|| std::io::Error::other("reviewed schema definition is missing"))?
                    .clone();
                node.definition = definition.clone().into();
                node.first_schema = Some(crate::SchemaResolution {
                    definition,
                    raw_schema: vec![],
                    byte_range: 0..0,
                    edits: vec![],
                });
            }
            let mapped = map_assembly(&nodes, &provider, key)?;
            assert_eq!(mapped.instances[0].part, 4);
            assert!(map_assembly(&nodes, &provider, "nearby-schema-key").is_err());
        }

        let registry = crate::BuiltinProfileRegistry::compiled()?;
        let exact_key = crate::SchemaKey::parse("SCH_2600169_25001_13006")?;
        let exact = registry
            .profile_for_key(&exact_key)
            .ok_or_else(|| std::io::Error::other("exact family member is missing"))?;
        let mut nodes = fixture(3);
        for node in nodes.iter_mut().filter(|node| node.node_type != 12) {
            let definition = exact
                .definition(node.node_type)
                .ok_or_else(|| std::io::Error::other("reviewed schema definition is missing"))?
                .clone();
            node.definition = definition.clone().into();
            node.first_schema = Some(crate::SchemaResolution {
                definition,
                raw_schema: vec![],
                byte_range: 0..0,
                edits: vec![],
            });
        }
        let inferred_key = crate::SchemaKey::parse("SCH_2600190_25001_13006")?;
        let family = registry
            .family_for_key(&inferred_key)
            .ok_or_else(|| std::io::Error::other("derived family profile is missing"))?;
        let provider = SchemaProviderResolution::Builtin {
            profile_id: family.metadata().profile_id.clone(),
            profile_revision: family.metadata().revision,
            schema_key: inferred_key.raw().into(),
            coverage: family.metadata().coverage,
            profile_sha256: family.metadata().profile_sha256.clone(),
        };
        let mapped = map_assembly(&nodes, &provider, inferred_key.raw())?;
        assert_eq!(mapped.instances[0].part, 4);
        Ok(())
    }

    #[test]
    fn rejects_missing_transform_zero_scale_and_unresolved_placements() -> Result<(), ParseError> {
        assert_eq!(
            map_assembly(
                &fixture(99),
                &SchemaProviderResolution::CallerSupplied,
                "custom"
            )
            .err()
            .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepReference)
        );
        let mut mapped = map_assembly(
            &fixture(0),
            &SchemaProviderResolution::CallerSupplied,
            "custom",
        )?;
        assert_eq!(mapped.instances[0].transform, None);
        assert_eq!(
            mapped.place_point([1.0, 2.0, 3.0], &[2]),
            Ok([1.0, 2.0, 3.0])
        );
        mapped.instances[0].transform = Some(99);
        assert!(mapped.place_point([1.0, 2.0, 3.0], &[2]).is_err());

        let mut invalid = fixture(3);
        invalid[2].fields[2].values[0] = FieldValue::Double(Some(0.0));
        assert_eq!(
            map_assembly(
                &invalid,
                &SchemaProviderResolution::CallerSupplied,
                "custom"
            )
            .err()
            .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
        Ok(())
    }

    #[test]
    fn enumerates_nested_leaf_paths_in_source_order_and_leaf_to_root_order()
    -> Result<(), ParseError> {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![10, 12], vec![]),
                plain_assembly(2, vec![11], vec![10]),
            ],
            instances: vec![
                plain_instance(10, 2, true, 1),
                plain_instance(11, 90, false, 2),
                plain_instance(12, 91, false, 1),
            ],
            transforms: vec![],
        };

        let paths = structure.leaf_body_instance_paths(1, 3)?;
        assert_eq!(paths, vec![vec![11, 10], vec![12]]);
        assert_eq!(
            structure.place_point([1.0, 2.0, 3.0], &paths[0]),
            Ok([1.0, 2.0, 3.0])
        );
        Ok(())
    }

    #[test]
    fn tessellates_assembly_leaf_paths_in_order_and_fails_on_invalid_body() -> Result<(), ParseError>
    {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![10, 12], vec![]),
                plain_assembly(2, vec![11], vec![10]),
            ],
            instances: vec![
                InstanceNode {
                    transform: Some(100),
                    ..plain_instance(10, 2, true, 1)
                },
                InstanceNode {
                    transform: Some(101),
                    ..plain_instance(11, 90, false, 2)
                },
                InstanceNode {
                    transform: Some(102),
                    ..plain_instance(12, 91, false, 1)
                },
            ],
            transforms: vec![
                placement_transform(100, identity_matrix(), [0.0, 2.0, 0.0]),
                placement_transform(101, identity_matrix(), [1.0, 0.0, 0.0]),
                placement_transform(102, identity_matrix(), [0.0, 0.0, 3.0]),
            ],
        };
        let mut brep = body_only_brep(&[90, 91]);
        for (index, body) in brep.bodies.iter_mut().enumerate() {
            let region = 100 + index as u32;
            let shell = 200 + index as u32;
            let face = 300 + index as u32;
            let surface = 400 + index as u32;
            let source = body.source.clone();
            let x = index as f64 * 10.0;
            body.regions = vec![region];
            brep.regions.push(Region {
                id: region,
                kind: RegionKind::Solid,
                body: body.id,
                shells: vec![shell, shell + 100],
                source: source.clone(),
            });
            brep.shells.push(Shell {
                id: shell,
                region,
                back_faces: vec![face],
                front_faces: vec![],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source.clone(),
            });
            brep.shells.push(Shell {
                id: shell + 100,
                region,
                back_faces: vec![],
                front_faces: vec![face],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source.clone(),
            });
            brep.faces.push(Face {
                id: face,
                back_shell: shell,
                front_shell: shell + 100,
                loops: vec![],
                surface: Some(surface),
                sense: Sense::Positive,
                attributes: vec![],
                source: source.clone(),
            });
            brep.surfaces.push(SurfaceGeometry {
                id: surface,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Mesh {
                    source_mesh_index: surface,
                    embedded: Some(RawMeshData {
                        mesh_index: surface,
                        transform_index: None,
                        surface_sense: Sense::Positive,
                        psm_mesh_index: surface,
                        precision: 0,
                        normal_type: 0,
                        positions: RawVectorPool {
                            encoding: 1,
                            vector_count: 3,
                            capacity: 3,
                            shift: 0,
                            values: vec![x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x, 1.0, 0.0],
                        },
                        normals: None,
                        position_indices: RawIntegerPool {
                            integer_count: 3,
                            capacity: 3,
                            shift: 0,
                            values: vec![0, 1, 2],
                        },
                        normal_indices: None,
                    }),
                    transform_index: None,
                    transform: None,
                    unembedded_source_fields: None,
                },
                source,
            });
        }

        assert_eq!(
            structure.leaf_body_instance_paths(1, 3)?,
            vec![vec![11, 10], vec![12]]
        );
        assert_eq!(
            structure.tessellate_assembly_bodies(&brep, 1, 3, 0.1),
            Ok(vec![
                [[1.0, 2.0, 0.0], [2.0, 2.0, 0.0], [1.0, 3.0, 0.0]],
                [[10.0, 0.0, 3.0], [11.0, 0.0, 3.0], [10.0, 1.0, 3.0]],
            ])
        );
        assert!(
            structure
                .tessellate_assembly_bodies(&body_only_brep(&[90]), 1, 3, 0.1)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn tessellates_resolved_compound_occurrence_and_places_child_triangles() {
        let structure = AssemblyStructure {
            assemblies: vec![plain_assembly(1, vec![10], vec![])],
            instances: vec![InstanceNode {
                transform: Some(100),
                ..plain_instance(10, 90, false, 1)
            }],
            transforms: vec![placement_transform(100, identity_matrix(), [5.0, 0.0, 0.0])],
        };
        let mut model = body_only_brep(&[90]);
        model.bodies[0].regions = vec![1];
        let mut parent = model.bodies[0].clone();
        parent.id = 2;
        parent.source.node_index = 91;
        parent.regions.clear();
        model.bodies.push(parent);
        model.regions.push(Region {
            id: 1,
            kind: RegionKind::Solid,
            body: 1,
            shells: vec![2, 3],
            source: model.bodies[0].source.clone(),
        });
        model.shells.extend([
            Shell {
                id: 2,
                region: 1,
                back_faces: vec![4],
                front_faces: vec![],
                wire_edges: vec![],
                isolated_vertex: None,
                source: model.bodies[0].source.clone(),
            },
            Shell {
                id: 3,
                region: 1,
                back_faces: vec![],
                front_faces: vec![4],
                wire_edges: vec![],
                isolated_vertex: None,
                source: model.bodies[0].source.clone(),
            },
        ]);
        model.faces.push(Face {
            id: 4,
            back_shell: 2,
            front_shell: 3,
            loops: vec![],
            surface: Some(5),
            sense: Sense::Positive,
            attributes: vec![],
            source: model.bodies[0].source.clone(),
        });
        model.surfaces.push(SurfaceGeometry {
            id: 5,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Mesh {
                source_mesh_index: 5,
                embedded: Some(RawMeshData {
                    mesh_index: 5,
                    transform_index: None,
                    surface_sense: Sense::Positive,
                    psm_mesh_index: 5,
                    precision: 0,
                    normal_type: 0,
                    positions: RawVectorPool {
                        encoding: 1,
                        vector_count: 3,
                        capacity: 3,
                        shift: 0,
                        values: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    },
                    normals: None,
                    position_indices: RawIntegerPool {
                        integer_count: 3,
                        capacity: 3,
                        shift: 0,
                        values: vec![0, 1, 2],
                    },
                    normal_indices: None,
                }),
                transform_index: None,
                transform: None,
                unembedded_source_fields: None,
            },
            source: model.bodies[0].source.clone(),
        });
        let resolved = BrepModelWithResolvedBodyHierarchy {
            model,
            body_hierarchies: vec![],
            body_children: vec![MappedBodyChildren {
                parent_brep_body_id: 2,
                parent_source_node_index: 91,
                child_brep_body_ids: vec![1],
            }],
        };

        assert_eq!(
            structure.tessellate_resolved_assembly_bodies(&resolved, 1, 1, 0.1),
            Ok(vec![[[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0]]])
        );
    }

    #[test]
    fn enumerates_shared_assembly_dag_once_per_instance_path() -> Result<(), ParseError> {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![10, 20], vec![]),
                plain_assembly(2, vec![30], vec![10, 20]),
            ],
            instances: vec![
                plain_instance(10, 2, true, 1),
                plain_instance(20, 2, true, 1),
                plain_instance(30, 90, false, 2),
            ],
            transforms: vec![],
        };

        assert_eq!(
            structure.leaf_body_instance_paths(1, 5)?,
            vec![vec![30, 10], vec![30, 20]]
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_duplicate_and_cyclic_assembly_links() {
        let missing = AssemblyStructure {
            assemblies: vec![plain_assembly(1, vec![99], vec![])],
            instances: vec![],
            transforms: vec![],
        };
        assert!(missing.leaf_body_instance_paths(1, 10).is_err());

        let duplicate = AssemblyStructure {
            assemblies: vec![plain_assembly(1, vec![10, 10], vec![])],
            instances: vec![plain_instance(10, 90, false, 1)],
            transforms: vec![],
        };
        assert!(duplicate.leaf_body_instance_paths(1, 10).is_err());

        let cycle = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![10], vec![20]),
                plain_assembly(2, vec![20], vec![10]),
            ],
            instances: vec![
                plain_instance(10, 2, true, 1),
                plain_instance(20, 1, true, 2),
            ],
            transforms: vec![],
        };
        assert!(cycle.leaf_body_instance_paths(1, 10).is_err());
    }

    #[test]
    fn rejects_ambiguous_assembly_and_instance_indices_before_traversal() {
        let duplicate_assembly = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![], vec![]),
                plain_assembly(1, vec![], vec![]),
            ],
            instances: vec![],
            transforms: vec![],
        };
        assert!(duplicate_assembly.leaf_body_instance_paths(1, 1).is_err());

        let instance = plain_instance(10, 90, false, 1);
        let duplicate_instance = AssemblyStructure {
            assemblies: vec![plain_assembly(1, vec![10], vec![])],
            instances: vec![instance.clone(), instance],
            transforms: vec![],
        };
        assert!(duplicate_instance.leaf_body_instance_paths(1, 1).is_err());
    }

    #[test]
    fn enforces_explicit_cap_on_dag_expansion() -> Result<(), ParseError> {
        let structure = AssemblyStructure {
            assemblies: vec![
                plain_assembly(1, vec![10], vec![]),
                plain_assembly(2, vec![20], vec![10]),
            ],
            instances: vec![
                plain_instance(10, 2, true, 1),
                plain_instance(20, 90, false, 2),
            ],
            transforms: vec![],
        };

        assert!(structure.leaf_body_instance_paths(1, 1).is_err());
        assert_eq!(
            structure.leaf_body_instance_paths(1, 2)?,
            vec![vec![20, 10]]
        );
        Ok(())
    }

    #[test]
    fn enumerates_a_deep_chain_without_recursive_call_stack() -> Result<(), ParseError> {
        const DEPTH: usize = 1_000;
        const INSTANCE_BASE: u32 = 20_000;
        let assemblies = (1..=DEPTH as u32)
            .map(|index| {
                let instance_index = INSTANCE_BASE + index;
                let referencing = if index == 1 {
                    vec![]
                } else {
                    vec![INSTANCE_BASE + index - 1]
                };
                plain_assembly(index, vec![instance_index], referencing)
            })
            .collect::<Vec<_>>();
        let instances = (1..=DEPTH as u32)
            .map(|index| {
                let final_leaf = index == DEPTH as u32;
                let part = if final_leaf { 90 } else { index + 1 };
                plain_instance(INSTANCE_BASE + index, part, !final_leaf, index)
            })
            .collect::<Vec<_>>();
        let structure = AssemblyStructure {
            assemblies,
            instances,
            transforms: vec![],
        };

        let paths = structure.leaf_body_instance_paths(1, DEPTH)?;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), DEPTH);
        assert_eq!(paths[0][0], INSTANCE_BASE + DEPTH as u32);
        assert_eq!(paths[0][DEPTH - 1], INSTANCE_BASE + 1);
        Ok(())
    }
}
