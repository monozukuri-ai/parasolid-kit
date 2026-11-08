//! Opt-in raw decoding of embedded PSM mesh pools for reviewed schema keys.

use std::collections::HashMap;

use super::assembly::{TransformNode, TransformPrecision};
use super::model::Sense;
use crate::schema::profiles::topsolid::TopSolidV6PsolidCatalogProvider;
use crate::{FieldType, FieldValue, RawNode, SchemaKey, SchemaProvider, TypeDefinition};

const MAX_POOL_ITEMS: usize = 12_000_000;
const MAX_FACETS: usize = MAX_POOL_ITEMS / 3;
const MAX_INDEXED_NODES: usize = 10_000_000;
const SCHEMA_KEYS: [&str; 5] = [
    "SCH_2701192_26105_13006",
    "SCH_3300259_32001_13006",
    "SCH_3300261_32001_13006",
    "SCH_3700192_36001_13006",
    "SCH_3600188_36001_13006",
];

/// Validated schema definitions and transmit-index view shared by mesh decoders.
pub(super) struct MeshDecodeContext<'a> {
    definitions: HashMap<u16, TypeDefinition>,
    by_index: HashMap<u32, &'a RawNode>,
    mesh_type: u16,
    owner_class: Option<u16>,
}

impl<'a> MeshDecodeContext<'a> {
    pub(super) fn new(
        schema_key: &str,
        nodes: &'a [RawNode],
        mesh_index: u32,
    ) -> Result<Self, MeshDataError> {
        if !SCHEMA_KEYS.contains(&schema_key) {
            return Err(MeshDataError::UnsupportedSchema(schema_key.to_owned()));
        }
        let key = SchemaKey::parse(schema_key)
            .map_err(|_| MeshDataError::UnsupportedSchema(schema_key.to_owned()))?;
        let catalog = TopSolidV6PsolidCatalogProvider::new().map_err(|_| {
            invalid_at(
                mesh_index,
                "schema",
                "reviewed schema catalog could not be initialized",
            )
        })?;
        let profile = catalog
            .provider_for_key(&key)
            .ok_or_else(|| MeshDataError::UnsupportedSchema(schema_key.to_owned()))?;
        let schema = if profile.contains_schema(key.effective()) {
            key.effective()
        } else {
            key.provider_schema()
        };
        let mut definitions = HashMap::new();
        for node_type in [100, 118, 189, 190, 191, 192, 193, 201, 229] {
            if let Some(definition) = profile.type_definition(schema, node_type) {
                definitions.insert(node_type, definition.clone());
            }
        }
        let mesh_type = [118, 201]
            .into_iter()
            .find(|node_type| {
                definitions
                    .get(node_type)
                    .is_some_and(|definition| definition.name == "MESH")
            })
            .ok_or_else(|| {
                invalid_at(
                    mesh_index,
                    "schema",
                    "exact profile has no reviewed MESH definition",
                )
            })?;
        if nodes.len() > MAX_INDEXED_NODES {
            return Err(invalid_at(mesh_index, "nodes", "node index limit exceeded"));
        }
        let mut by_index = HashMap::with_capacity(nodes.len());
        for node in nodes {
            if by_index.insert(node.index, node).is_some() {
                return Err(invalid(node, "node_index", "node index is ambiguous"));
            }
        }
        let owner_class = definitions
            .get(&189)
            .and_then(|definition| definition.fields.iter().find(|field| field.name == "owner"))
            .filter(|field| field.field_type == FieldType::PointerIndex)
            .map(|field| field.pointer_class)
            .filter(|class| *class != 0);
        Ok(Self {
            definitions,
            by_index,
            mesh_type,
            owner_class,
        })
    }

    pub(super) fn decode_embedded_mesh(
        &self,
        mesh_index: u32,
    ) -> Result<Option<RawMeshData>, MeshDataError> {
        decode_embedded_mesh_with(self, mesh_index)
    }

    pub(super) fn decode_mesh_linked_transform(
        &self,
        mesh_index: u32,
    ) -> Result<Option<TransformNode>, MeshDataError> {
        decode_mesh_linked_transform_with(self, mesh_index)
    }
}

/// Raw vector comb data in its transmitted encoding, order, and allocated capacity.
#[derive(Debug, Clone, PartialEq)]
pub struct RawVectorPool {
    pub encoding: u8,
    pub vector_count: usize,
    pub capacity: usize,
    pub shift: usize,
    /// Finite scalar values, including unused capacity, in tooth order.
    pub values: Vec<f64>,
}

/// Raw integer comb data. Values are retained without zero/one-based conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIntegerPool {
    pub integer_count: usize,
    pub capacity: usize,
    pub shift: usize,
    /// Signed source values, including unused capacity.
    pub values: Vec<i32>,
}

/// Decoded embedded PSM mesh pools without topology or coordinate interpretation.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMeshData {
    pub mesh_index: u32,
    /// Source MESH transform link. A null link is retained as `None`.
    pub transform_index: Option<u32>,
    /// Source MESH surface sense, including `?` as `Sense::Unknown`.
    pub surface_sense: Sense,
    pub psm_mesh_index: u32,
    pub precision: u8,
    pub normal_type: u8,
    pub positions: RawVectorPool,
    pub normals: Option<RawVectorPool>,
    pub position_indices: RawIntegerPool,
    pub normal_indices: Option<RawIntegerPool>,
}

/// Explicit convention selected by the caller when interpreting triangle indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshIndexOrigin {
    /// Integer `0` selects the first position.
    ZeroBased,
    /// Integer `1` selects the first position.
    OneBased,
}

impl RawMeshData {
    /// Turn raw position indices into triangle coordinates when exactly one origin fits.
    ///
    /// The raw pools remain unchanged. Ambiguous or out-of-range indices are rejected.
    ///
    /// # Errors
    /// Returns an error when the index origin is ambiguous or the mesh data is malformed.
    pub fn triangles_with_inferred_index_origin(
        &self,
    ) -> Result<Vec<[[f64; 3]; 3]>, MeshDataError> {
        self.triangles_with_index_origin(self.inferred_index_origin()?)
    }

    fn inferred_index_origin(&self) -> Result<MeshIndexOrigin, MeshDataError> {
        let indices = &self.position_indices;
        if indices.integer_count == 0
            || indices.integer_count % 3 != 0
            || indices.integer_count > indices.capacity
            || indices.capacity != indices.values.len()
            || indices.integer_count / 3 > MAX_FACETS
        {
            return Err(invalid_at(
                self.psm_mesh_index,
                "position_indices",
                "triangle index count or capacity is invalid",
            ));
        }
        let active_indices = &indices.values[..indices.integer_count];
        let zero_based_fits = active_indices.iter().all(|index| {
            usize::try_from(*index).is_ok_and(|index| index < self.positions.vector_count)
        });
        let one_based_fits = active_indices.iter().all(|index| {
            usize::try_from(*index)
                .is_ok_and(|index| index > 0 && index <= self.positions.vector_count)
        });
        let origin = match (zero_based_fits, one_based_fits) {
            (true, false) => MeshIndexOrigin::ZeroBased,
            (false, true) => MeshIndexOrigin::OneBased,
            (true, true) => {
                return Err(invalid_at(
                    self.psm_mesh_index,
                    "position_indices",
                    "index origin is ambiguous",
                ));
            }
            (false, false) => {
                return Err(invalid_at(
                    self.psm_mesh_index,
                    "position_indices",
                    "index is outside zero-based and one-based ranges",
                ));
            }
        };
        Ok(origin)
    }

    /// Turn raw position indices into triangle coordinates under the caller's chosen origin.
    ///
    /// The raw pools remain unchanged. This method does not infer an origin; when both
    /// conventions fit the values, the caller's selection determines the triangles.
    ///
    /// # Errors
    /// Returns an error when the pools are malformed or an index is invalid under `origin`.
    pub fn triangles_with_index_origin(
        &self,
        origin: MeshIndexOrigin,
    ) -> Result<Vec<[[f64; 3]; 3]>, MeshDataError> {
        let source = self.psm_mesh_index;
        if self.positions.encoding != 1 {
            return Err(invalid_at(
                source,
                "position_pool",
                "triangle coordinates require simple Cartesian vector encoding",
            ));
        }
        if self.positions.vector_count == 0
            || self.positions.vector_count > self.positions.capacity
            || self.positions.capacity > MAX_POOL_ITEMS / 3
            || self.positions.capacity.checked_mul(3) != Some(self.positions.values.len())
            || self.positions.values.iter().any(|value| !value.is_finite())
        {
            return Err(invalid_at(
                source,
                "position_pool",
                "position count, capacity, or coordinates are invalid",
            ));
        }
        let indices = &self.position_indices;
        if indices.integer_count == 0
            || indices.integer_count % 3 != 0
            || indices.integer_count > indices.capacity
            || indices.capacity != indices.values.len()
            || indices.integer_count / 3 > MAX_FACETS
        {
            return Err(invalid_at(
                source,
                "position_indices",
                "triangle index count or capacity is invalid",
            ));
        }
        let mut triangles = Vec::with_capacity(indices.integer_count / 3);
        let mut triangle = [[0.0; 3]; 3];
        for (corner, raw_index) in indices.values[..indices.integer_count].iter().enumerate() {
            let index = match origin {
                MeshIndexOrigin::ZeroBased => usize::try_from(*raw_index).ok(),
                MeshIndexOrigin::OneBased => raw_index
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok()),
            }
            .filter(|index| *index < self.positions.vector_count)
            .ok_or_else(|| {
                invalid_at(
                    source,
                    "position_indices",
                    "index is outside the caller-selected origin range",
                )
            })?;
            let start = index
                .checked_mul(3)
                .ok_or_else(|| invalid_at(source, "position_pool", "position offset overflow"))?;
            triangle[corner % 3].copy_from_slice(&self.positions.values[start..start + 3]);
            if corner % 3 == 2 {
                triangles.push(triangle);
            }
        }
        Ok(triangles)
    }

    /// Return type-3 raw spherical-polar normal pairs in position-corner order.
    ///
    /// The XT reference does not specify the angle convention or normal transforms, so this
    /// preserves the two stored values without converting or orienting them.
    ///
    /// # Errors
    /// Returns an error when normal data is missing, malformed, or incompatible with `origin`.
    pub fn per_corner_normal_parameters_with_index_origin(
        &self,
        origin: MeshIndexOrigin,
    ) -> Result<Vec<[[f64; 2]; 3]>, MeshDataError> {
        if self.normal_type != 3 {
            return Err(invalid_at(
                self.psm_mesh_index,
                "normal_type",
                "per-corner normal parameters require per-facet normal storage",
            ));
        }
        let normals = self.normals.as_ref().ok_or_else(|| {
            invalid_at(
                self.psm_mesh_index,
                "normal_pool",
                "per-facet normal storage requires a normal pool",
            )
        })?;
        let indices = self.normal_indices.as_ref().ok_or_else(|| {
            invalid_at(
                self.psm_mesh_index,
                "normal_indices",
                "per-facet normal storage requires normal indices",
            )
        })?;
        let positions = &self.positions;
        if positions.encoding != 1
            || positions.vector_count == 0
            || positions.vector_count > positions.capacity
            || positions.capacity > MAX_POOL_ITEMS / 3
            || positions.capacity.checked_mul(3) != Some(positions.values.len())
            || positions.values.iter().any(|value| !value.is_finite())
        {
            return Err(invalid_at(
                self.psm_mesh_index,
                "position_pool",
                "position pool must contain finite Cartesian triples",
            ));
        }
        if normals.encoding != 2
            || normals.vector_count == 0
            || normals.vector_count > normals.capacity
            || normals.capacity > MAX_POOL_ITEMS / 2
            || normals.capacity.checked_mul(2) != Some(normals.values.len())
            || normals.values.iter().any(|value| !value.is_finite())
        {
            return Err(invalid_at(
                self.psm_mesh_index,
                "normal_pool",
                "normal pool must contain finite two-component spherical-polar values",
            ));
        }
        let position_indices = &self.position_indices;
        if position_indices.integer_count == 0
            || position_indices.integer_count % 3 != 0
            || position_indices.integer_count > position_indices.capacity
            || position_indices.capacity > MAX_POOL_ITEMS
            || position_indices.capacity != position_indices.values.len()
            || position_indices.integer_count / 3 > MAX_FACETS
            || indices.integer_count == 0
            || indices.integer_count % 3 != 0
            || indices.integer_count != position_indices.integer_count
            || indices.integer_count > indices.capacity
            || indices.capacity > MAX_POOL_ITEMS
            || indices.capacity != indices.values.len()
            || indices.integer_count / 3 > MAX_FACETS
        {
            return Err(invalid_at(
                self.psm_mesh_index,
                "normal_indices",
                "normal index count or capacity must match the 3N position corners",
            ));
        }
        per_corner_normal_parameters_with_origin(
            self.psm_mesh_index,
            origin,
            positions,
            normals,
            position_indices,
            indices,
        )
    }

    /// Place raw mesh triangles using an explicitly supplied index origin and source transform.
    ///
    /// A null source transform link requires `transform == None` and leaves coordinates in
    /// the raw mesh coordinate space; no unit conversion is applied. A non-null link requires
    /// the caller to provide the already decoded transform with the same transmit index.
    /// The face and surface senses determine winding using the XT rule that orientation is
    /// positive when both senses have the same sign. Unknown senses are rejected.
    ///
    /// # Errors
    /// Returns an error for a missing or mismatched transform, unknown sense, or invalid mesh.
    pub fn placed_triangles(
        &self,
        origin: MeshIndexOrigin,
        transform: Option<&TransformNode>,
        face_sense: Sense,
    ) -> Result<Vec<[[f64; 3]; 3]>, MeshDataError> {
        let transform_index = self.transform_index;
        match (transform_index, transform) {
            (None, None) => {}
            (Some(expected), Some(transform)) if transform.index == expected => {}
            (Some(_), None) => {
                return Err(invalid_at(
                    self.mesh_index,
                    "transform",
                    "source transform link is non-null but no matching transform was supplied",
                ));
            }
            (None, Some(_)) => {
                return Err(invalid_at(
                    self.mesh_index,
                    "transform",
                    "a transform was supplied for a null source link",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(invalid_at(
                    self.mesh_index,
                    "transform",
                    "supplied transform index does not match the source link",
                ));
            }
        }
        if self.surface_sense == Sense::Unknown || face_sense == Sense::Unknown {
            return Err(invalid_at(
                self.mesh_index,
                "sense",
                "unknown face or surface sense cannot determine triangle winding",
            ));
        }

        let mut triangles = self.triangles_with_index_origin(origin)?;
        if let Some(transform) = transform {
            for triangle in &mut triangles {
                for point in triangle {
                    *point = transform.transform_point(*point).map_err(|_| {
                        invalid_at(
                            transform.index,
                            "transform",
                            "transform cannot place mesh point",
                        )
                    })?;
                }
            }
        }
        if (self.surface_sense == Sense::Positive) != (face_sense == Sense::Positive) {
            for triangle in &mut triangles {
                triangle.swap(1, 2);
            }
        }
        Ok(triangles)
    }

    /// Place raw mesh triangles when exactly one position-index origin fits.
    ///
    /// The raw pools remain unchanged. Ambiguous or out-of-range indices are rejected.
    ///
    /// # Errors
    /// Returns an error when the index origin is ambiguous or the mesh data is malformed.
    pub fn placed_triangles_with_inferred_index_origin(
        &self,
        transform: Option<&TransformNode>,
        face_sense: Sense,
    ) -> Result<Vec<[[f64; 3]; 3]>, MeshDataError> {
        self.placed_triangles(self.inferred_index_origin()?, transform, face_sense)
    }
}

fn per_corner_normal_parameters_with_origin(
    source: u32,
    origin: MeshIndexOrigin,
    positions: &RawVectorPool,
    normals: &RawVectorPool,
    position_indices: &RawIntegerPool,
    normal_indices: &RawIntegerPool,
) -> Result<Vec<[[f64; 2]; 3]>, MeshDataError> {
    let mut triangles = Vec::with_capacity(normal_indices.integer_count / 3);
    let mut triangle = [[0.0; 2]; 3];
    for (corner, (raw_position_index, raw_normal_index)) in position_indices.values
        [..position_indices.integer_count]
        .iter()
        .zip(&normal_indices.values[..normal_indices.integer_count])
        .enumerate()
    {
        let _position_index = match origin {
            MeshIndexOrigin::ZeroBased => usize::try_from(*raw_position_index).ok(),
            MeshIndexOrigin::OneBased => raw_position_index
                .checked_sub(1)
                .and_then(|index| usize::try_from(index).ok()),
        }
        .filter(|index| *index < positions.vector_count)
        .ok_or_else(|| {
            invalid_at(
                source,
                "position_indices",
                "index is outside the caller-selected origin range",
            )
        })?;
        let index = match origin {
            MeshIndexOrigin::ZeroBased => usize::try_from(*raw_normal_index).ok(),
            MeshIndexOrigin::OneBased => raw_normal_index
                .checked_sub(1)
                .and_then(|index| usize::try_from(index).ok()),
        }
        .filter(|index| *index < normals.vector_count)
        .ok_or_else(|| {
            invalid_at(
                source,
                "normal_indices",
                "index is outside the caller-selected origin range",
            )
        })?;
        let start = index
            .checked_mul(2)
            .ok_or_else(|| invalid_at(source, "normal_pool", "offset overflow"))?;
        triangle[corner % 3].copy_from_slice(&normals.values[start..start + 2]);
        if corner % 3 == 2 {
            triangles.push(triangle);
        }
    }
    Ok(triangles)
}

/// An unsupported profile or malformed embedded mesh field/link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshDataError {
    UnsupportedSchema(String),
    Invalid {
        node: u32,
        field: &'static str,
        reason: &'static str,
    },
}

/// Decode an embedded `MESH.psm_imesh` for one exact reviewed `TopSolid` schema key.
///
/// `Ok(None)` means the mesh node has a null embedded link. The XT reference allows
/// such mesh data to live in an associated mesh file, so this does not classify why.
///
/// # Errors
/// Returns an error when the schema key or mesh fields and links are unsupported or malformed.
pub fn decode_embedded_mesh(
    schema_key: &str,
    nodes: &[RawNode],
    mesh_index: u32,
) -> Result<Option<RawMeshData>, MeshDataError> {
    MeshDecodeContext::new(schema_key, nodes, mesh_index)?.decode_embedded_mesh(mesh_index)
}

#[allow(
    clippy::too_many_lines,
    reason = "This decoder keeps the ordered field validation and mesh reconstruction steps together."
)]
fn decode_embedded_mesh_with(
    context: &MeshDecodeContext<'_>,
    mesh_index: u32,
) -> Result<Option<RawMeshData>, MeshDataError> {
    let definitions = &context.definitions;
    let by_index = &context.by_index;
    let mesh_type = context.mesh_type;
    let owner_class = context.owner_class.ok_or_else(|| {
        invalid_at(
            mesh_index,
            "schema",
            "exact profile has no reviewed PSM_MESH owner link",
        )
    })?;
    let mesh = target(
        by_index,
        definitions,
        mesh_index,
        mesh_type,
        "MESH",
        mesh_index,
        "mesh",
    )?;
    if mesh
        .definition
        .fields
        .iter()
        .any(|definition| definition.name == "rcv_key")
    {
        let rcv_key_field = field(mesh, "rcv_key", FieldType::PointerIndex)?;
        validate_exact_field(mesh, rcv_key_field, "rcv_key")?;
        if rcv_key_field.definition.pointer_class != 1041
            || rcv_key_field.definition.element_count != 0
        {
            return Err(invalid(
                mesh,
                "rcv_key",
                "receiver key pointer schema type or count is wrong",
            ));
        }
        let rcv_key = match rcv_key_field.values.as_slice() {
            [FieldValue::PointerIndex(index)] => *index,
            _ => {
                return Err(invalid(
                    mesh,
                    "rcv_key",
                    "receiver key must contain one pointer value",
                ));
            }
        };
        // XT treats a pointer to no transmitted node as null; retain the source index untouched.
        if rcv_key != 0 && by_index.contains_key(&rcv_key) {
            return Err(invalid(
                mesh,
                "rcv_key",
                "unused receiver key resolves to a transmitted node",
            ));
        }
    }
    let transform_field = field(mesh, "transform", FieldType::PointerIndex)?;
    validate_exact_field(mesh, transform_field, "transform")?;
    if transform_field.definition.pointer_class != 100
        || transform_field.definition.element_count != 0
    {
        return Err(invalid(
            mesh,
            "transform",
            "transform pointer schema type or count is wrong",
        ));
    }
    let transform_index = match transform_field.values.as_slice() {
        [FieldValue::PointerIndex(0)] => None,
        [FieldValue::PointerIndex(index)] => Some(*index),
        _ => {
            return Err(invalid(
                mesh,
                "transform",
                "transform must contain one pointer value",
            ));
        }
    };
    let surface_field = field(mesh, "sense", FieldType::Character)?;
    validate_exact_field(mesh, surface_field, "sense")?;
    if surface_field.definition.element_count != 0 {
        return Err(invalid(mesh, "sense", "surface sense is not a scalar"));
    }
    let surface_sense = match surface_field.values.as_slice() {
        [FieldValue::Character(b'+')] => Sense::Positive,
        [FieldValue::Character(b'-')] => Sense::Negative,
        [FieldValue::Character(b'?')] => Sense::Unknown,
        [FieldValue::Character(_)] => {
            return Err(invalid(mesh, "sense", "sense must be '+', '-', or '?'"));
        }
        _ => {
            return Err(invalid(
                mesh,
                "sense",
                "sense must contain one character value",
            ));
        }
    };
    if let Some(transform_index) = transform_index {
        target(
            by_index,
            definitions,
            mesh_index,
            100,
            "TRANSFORM",
            transform_index,
            "transform",
        )?;
    }
    let Some(psm_index) = optional_pointer(mesh, "psm_imesh", 189)? else {
        return Ok(None);
    };
    let psm = target(
        by_index,
        definitions,
        mesh_index,
        189,
        "PSM_MESH",
        psm_index,
        "psm_imesh",
    )?;
    if pointer(psm, "owner", owner_class)? != 0 {
        return Err(invalid(psm, "owner", "reserved owner pointer must be null"));
    }
    let precision = unsigned(psm, "precision")?;
    if precision != 1 {
        return Err(invalid(
            psm,
            "precision",
            "only double precision is supported",
        ));
    }

    let positions = vector_pool(by_index, definitions, psm, "position_pool")?;
    let position_indices = integer_pool(by_index, definitions, psm, "position_indices")?;
    if position_indices.integer_count == 0 || position_indices.integer_count % 3 != 0 {
        return Err(invalid(
            psm,
            "position_indices",
            "index count must be a nonzero multiple of three",
        ));
    }
    if position_indices.integer_count / 3 > MAX_FACETS {
        return Err(invalid(psm, "position_indices", "facet limit exceeded"));
    }
    if positions.vector_count == 0
        || position_indices.values[..position_indices.integer_count]
            .iter()
            .any(|index| {
                usize::try_from(*index).map_or(true, |index| index > positions.vector_count)
            })
    {
        return Err(invalid(
            psm,
            "position_indices",
            "index is outside both possible pool-origin ranges",
        ));
    }

    let normal_type = unsigned(psm, "normal_type")?;
    let (normals, normal_indices) = match normal_type {
        1 => {
            if optional_pointer(psm, "normal_pool", 193)?.is_some()
                || optional_pointer(psm, "normal_indices", 191)?.is_some()
            {
                return Err(invalid(
                    psm,
                    "normal_type",
                    "normal-none mesh must have null normal links",
                ));
            }
            (None, None)
        }
        2 => {
            return Err(invalid(
                psm,
                "normal_type",
                "per-vertex normal storage is not implemented",
            ));
        }
        3 => {
            let normals = vector_pool(by_index, definitions, psm, "normal_pool")?;
            let indices = integer_pool(by_index, definitions, psm, "normal_indices")?;
            if indices.integer_count != position_indices.integer_count
                || normals.vector_count == 0
                || indices.values[..indices.integer_count].iter().any(|index| {
                    usize::try_from(*index).map_or(true, |index| index > normals.vector_count)
                })
            {
                return Err(invalid(
                    psm,
                    "normal_indices",
                    "normal indices must parallel facets and fit either possible pool-origin range",
                ));
            }
            (Some(normals), Some(indices))
        }
        _ => return Err(invalid(psm, "normal_type", "unknown normal storage type")),
    };
    Ok(Some(RawMeshData {
        mesh_index,
        transform_index,
        surface_sense,
        psm_mesh_index: psm_index,
        precision,
        normal_type,
        positions,
        normals,
        position_indices,
        normal_indices,
    }))
}

/// Decode the exact-profile TRANSFORM linked from a named MESH node.
///
/// This is an opt-in companion to [`decode_embedded_mesh`]. Raw `TopSolid` field names are
/// mapped only from the selected provider's definitions. A profile without a `precision`
/// field yields `precision: None`; when that field exists, its null/non-null pointer state is
/// honored and a non-null link must resolve to the exact reviewed `TRANSFORM_PRECISION` type.
///
/// # Errors
/// Returns an error when the schema key or transform fields and links are unsupported or malformed.
pub fn decode_mesh_linked_transform(
    schema_key: &str,
    nodes: &[RawNode],
    mesh_index: u32,
) -> Result<Option<TransformNode>, MeshDataError> {
    MeshDecodeContext::new(schema_key, nodes, mesh_index)?.decode_mesh_linked_transform(mesh_index)
}

fn decode_mesh_linked_transform_with(
    context: &MeshDecodeContext<'_>,
    mesh_index: u32,
) -> Result<Option<TransformNode>, MeshDataError> {
    let definitions = &context.definitions;
    let by_index = &context.by_index;
    let mesh_type = context.mesh_type;
    let mesh = target(
        by_index,
        definitions,
        mesh_index,
        mesh_type,
        "MESH",
        mesh_index,
        "mesh",
    )?;
    let mesh_transform = field(mesh, "transform", FieldType::PointerIndex)?;
    validate_exact_field(mesh, mesh_transform, "transform")?;
    if mesh_transform.definition.pointer_class != 100
        || mesh_transform.definition.element_count != 0
    {
        return Err(invalid(
            mesh,
            "transform",
            "transform pointer schema type or count is wrong",
        ));
    }
    let transform_index = match mesh_transform.values.as_slice() {
        [FieldValue::PointerIndex(0)] => return Ok(None),
        [FieldValue::PointerIndex(index)] => *index,
        _ => {
            return Err(invalid(
                mesh,
                "transform",
                "transform must contain one pointer value",
            ));
        }
    };
    let transform = target(
        by_index,
        definitions,
        mesh_index,
        100,
        "TRANSFORM",
        transform_index,
        "transform",
    )?;

    let rotation = exact_double_array::<9>(transform, "rotation_matrix")?;
    let translation = exact_vector(transform, "translation_vector")?;
    let scale = exact_double_scalar(transform, "scale")?;
    if scale == 0.0 {
        return Err(invalid(transform, "scale", "transform scale is zero"));
    }
    let flags = exact_integer_scalar(transform, "flag")?;
    if flags < 0 {
        return Err(invalid(transform, "flag", "transform flags are negative"));
    }
    let perspective = exact_optional_vector(transform, "perspective_vector")?;
    let precision = decode_transform_precision(definitions, by_index, transform)?;

    Ok(Some(TransformNode {
        index: transform.index,
        rotation,
        translation,
        scale,
        flags,
        perspective,
        precision,
        byte_range: transform.byte_range.clone(),
    }))
}

fn decode_transform_precision(
    definitions: &HashMap<u16, TypeDefinition>,
    by_index: &HashMap<u32, &RawNode>,
    transform: &RawNode,
) -> Result<Option<TransformPrecision>, MeshDataError> {
    if !transform
        .definition
        .fields
        .iter()
        .any(|definition| definition.name == "precision")
    {
        return Ok(None);
    }
    let link = field(transform, "precision", FieldType::PointerIndex)?;
    validate_exact_field(transform, link, "precision")?;
    if link.definition.pointer_class != 229 || link.definition.element_count != 0 {
        return Err(invalid(
            transform,
            "precision",
            "precision pointer schema type or count is wrong",
        ));
    }
    match link.values.as_slice() {
        [FieldValue::PointerIndex(0)] => Ok(None),
        [FieldValue::PointerIndex(index)] => {
            let precision_node = target(
                by_index,
                definitions,
                transform.index,
                229,
                "TRANSFORM_PRECISION",
                *index,
                "precision",
            )?;
            Ok(Some(TransformPrecision {
                index: precision_node.index,
                rotation_correction: exact_double_array::<9>(precision_node, "rotation_matrix")?,
                translation_correction: exact_vector(precision_node, "translation_vector")?,
            }))
        }
        _ => Err(invalid(
            transform,
            "precision",
            "precision must contain one pointer value",
        )),
    }
}

/// Decode and place one embedded PSM mesh using an explicit index origin and face sense.
///
/// The same `nodes` and exact `schema_key` are used to decode the mesh and its linked
/// transform. `Ok(None)` preserves the source state where `MESH.psm_imesh` is null; XT allows
/// mesh data to be held outside the document, but this function does not resolve that data.
/// A null `MESH.transform` leaves coordinates raw, with no unit conversion. The caller must
/// select the position-index origin and provide the face sense; unknown sense is rejected.
///
/// # Errors
/// Returns an error when the schema, mesh, transform, index origin, or face sense is invalid.
pub fn decode_mesh_triangles(
    schema_key: &str,
    nodes: &[RawNode],
    mesh_index: u32,
    origin: MeshIndexOrigin,
    face_sense: Sense,
) -> Result<Option<Vec<[[f64; 3]; 3]>>, MeshDataError> {
    let context = MeshDecodeContext::new(schema_key, nodes, mesh_index)?;
    let Some(mesh) = context.decode_embedded_mesh(mesh_index)? else {
        return Ok(None);
    };
    let transform = context.decode_mesh_linked_transform(mesh_index)?;
    mesh.placed_triangles(origin, transform.as_ref(), face_sense)
        .map(Some)
}

fn exact_double_array<const N: usize>(
    node: &RawNode,
    name: &'static str,
) -> Result<[f64; N], MeshDataError> {
    let raw_field = field(node, name, FieldType::Double)?;
    validate_exact_field(node, raw_field, name)?;
    if u32::try_from(N).ok() != Some(raw_field.definition.element_count)
        || raw_field.values.len() != N
    {
        return Err(invalid(
            node,
            name,
            "double array count does not match the exact provider definition",
        ));
    }
    let mut values = [0.0; N];
    for (output, value) in values.iter_mut().zip(&raw_field.values) {
        match value {
            FieldValue::Double(Some(value)) if value.is_finite() => *output = *value,
            _ => {
                return Err(invalid(
                    node,
                    name,
                    "double array contains null, non-finite, or wrongly typed data",
                ));
            }
        }
    }
    Ok(values)
}

fn exact_double_scalar(node: &RawNode, name: &'static str) -> Result<f64, MeshDataError> {
    let raw_field = field(node, name, FieldType::Double)?;
    validate_exact_field(node, raw_field, name)?;
    if raw_field.definition.element_count != 0 {
        return Err(invalid(node, name, "double field is not scalar"));
    }
    match raw_field.values.as_slice() {
        [FieldValue::Double(Some(value))] if value.is_finite() => Ok(*value),
        _ => Err(invalid(
            node,
            name,
            "double scalar is null, non-finite, or wrongly typed",
        )),
    }
}

fn exact_integer_scalar(node: &RawNode, name: &'static str) -> Result<i32, MeshDataError> {
    let raw_field = field(node, name, FieldType::Integer)?;
    validate_exact_field(node, raw_field, name)?;
    if raw_field.definition.element_count != 0 {
        return Err(invalid(node, name, "integer field is not scalar"));
    }
    match raw_field.values.as_slice() {
        [FieldValue::Integer(Some(value))] => Ok(*value),
        _ => Err(invalid(
            node,
            name,
            "integer scalar is unset or wrongly typed",
        )),
    }
}

fn exact_vector(node: &RawNode, name: &'static str) -> Result<[f64; 3], MeshDataError> {
    let raw_field = field(node, name, FieldType::Vector)?;
    validate_exact_field(node, raw_field, name)?;
    if raw_field.definition.element_count != 0 {
        return Err(invalid(node, name, "vector field is not scalar"));
    }
    match raw_field.values.as_slice() {
        [FieldValue::Vector([Some(x), Some(y), Some(z)])]
            if x.is_finite() && y.is_finite() && z.is_finite() =>
        {
            Ok([*x, *y, *z])
        }
        _ => Err(invalid(
            node,
            name,
            "vector is incomplete, non-finite, or wrongly typed",
        )),
    }
}

fn exact_optional_vector(
    node: &RawNode,
    name: &'static str,
) -> Result<[Option<f64>; 3], MeshDataError> {
    let raw_field = field(node, name, FieldType::Vector)?;
    validate_exact_field(node, raw_field, name)?;
    if raw_field.definition.element_count != 0 {
        return Err(invalid(node, name, "vector field is not scalar"));
    }
    match raw_field.values.as_slice() {
        [FieldValue::Vector(values)] if values.iter().flatten().all(|value| value.is_finite()) => {
            Ok(*values)
        }
        _ => Err(invalid(
            node,
            name,
            "optional vector is non-finite or wrongly typed",
        )),
    }
}

fn vector_pool(
    nodes: &HashMap<u32, &RawNode>,
    definitions: &HashMap<u16, TypeDefinition>,
    source: &RawNode,
    field_name: &'static str,
) -> Result<RawVectorPool, MeshDataError> {
    let comb = target(
        nodes,
        definitions,
        source.index,
        193,
        "VECTOR_COMB",
        pointer(source, field_name, 193)?,
        field_name,
    )?;
    let vector_count = count(comb, "n_vectors")?;
    let capacity = count(comb, "n_max_vectors")?;
    let encoding = unsigned(comb, "encoding")?;
    let components = match encoding {
        1 => 3,
        2 => 2,
        _ => return Err(invalid(comb, "encoding", "unknown vector encoding")),
    };
    let shift = count(comb, "shift")?;
    let tooth_capacity = u32::try_from(shift)
        .ok()
        .and_then(|shift| 1usize.checked_shl(shift))
        .ok_or_else(|| invalid(comb, "shift", "tooth capacity shift is out of range"))?;
    if vector_count > capacity
        || capacity > MAX_POOL_ITEMS / components
        || tooth_capacity > MAX_POOL_ITEMS / components
    {
        return Err(invalid(
            comb,
            "n_vectors",
            "vector count or tooth capacity exceeds limits",
        ));
    }
    if capacity != 0 && capacity % tooth_capacity != 0 {
        return Err(invalid(
            comb,
            "n_max_vectors",
            "capacity is not a whole number of teeth",
        ));
    }
    let teeth = pointer_array(comb, "teeth", 192)?;
    let mut values = Vec::with_capacity(capacity.saturating_mul(components));
    for tooth_index in teeth {
        let tooth = target(
            nodes,
            definitions,
            comb.index,
            192,
            "VECTOR_TOOTH",
            tooth_index,
            "teeth",
        )?;
        let tooth_values = double_values(tooth, "values")?;
        if tooth_values.len() != tooth_capacity.saturating_mul(components) {
            return Err(invalid(
                tooth,
                "values",
                "vector tooth length does not match 2^shift and encoding",
            ));
        }
        for value in tooth_values {
            match value {
                FieldValue::Double(Some(value)) if value.is_finite() => values.push(*value),
                FieldValue::Double(_) => {
                    return Err(invalid(
                        tooth,
                        "values",
                        "vector value is unset or non-finite",
                    ));
                }
                _ => return Err(invalid(tooth, "values", "vector value has the wrong type")),
            }
            if values.len() > capacity.saturating_mul(components) {
                return Err(invalid(
                    comb,
                    "teeth",
                    "tooth data exceeds declared capacity",
                ));
            }
        }
    }
    if values.len() != capacity.saturating_mul(components) {
        return Err(invalid(
            comb,
            "teeth",
            "tooth data does not match declared capacity",
        ));
    }
    Ok(RawVectorPool {
        encoding,
        vector_count,
        capacity,
        shift,
        values,
    })
}

fn integer_pool(
    nodes: &HashMap<u32, &RawNode>,
    definitions: &HashMap<u16, TypeDefinition>,
    source: &RawNode,
    field_name: &'static str,
) -> Result<RawIntegerPool, MeshDataError> {
    let comb = target(
        nodes,
        definitions,
        source.index,
        191,
        "INTEGER_COMB",
        pointer(source, field_name, 191)?,
        field_name,
    )?;
    let integer_count = count(comb, "n_integers")?;
    let capacity = count(comb, "n_max_integers")?;
    if unsigned(comb, "encoding")? != 1 || count(comb, "n_bits_per_integer")? != 32 {
        return Err(invalid(
            comb,
            "encoding",
            "only encoding 1 with 32-bit integers is supported",
        ));
    }
    let shift = count(comb, "shift")?;
    let tooth_capacity = u32::try_from(shift)
        .ok()
        .and_then(|shift| 1usize.checked_shl(shift))
        .ok_or_else(|| invalid(comb, "shift", "tooth capacity shift is out of range"))?;
    if integer_count > capacity || capacity > MAX_POOL_ITEMS || tooth_capacity > MAX_POOL_ITEMS {
        return Err(invalid(
            comb,
            "n_integers",
            "integer count or tooth capacity exceeds limits",
        ));
    }
    if capacity != 0 && capacity % tooth_capacity != 0 {
        return Err(invalid(
            comb,
            "n_max_integers",
            "capacity is not a whole number of teeth",
        ));
    }
    let teeth = pointer_array(comb, "teeth", 190)?;
    let mut values = Vec::with_capacity(capacity);
    for tooth_index in teeth {
        let tooth = target(
            nodes,
            definitions,
            comb.index,
            190,
            "INTEGER_TOOTH",
            tooth_index,
            "teeth",
        )?;
        let tooth_values = integer_values(tooth, "values")?;
        if tooth_values.len() != tooth_capacity {
            return Err(invalid(
                tooth,
                "values",
                "integer tooth length does not match 2^shift",
            ));
        }
        for value in tooth_values {
            match value {
                FieldValue::Integer(Some(value)) if *value >= 0 => values.push(*value),
                FieldValue::Integer(Some(_)) => {
                    return Err(invalid(tooth, "values", "index must be nonnegative"));
                }
                FieldValue::Integer(_) => return Err(invalid(tooth, "values", "index is unset")),
                _ => return Err(invalid(tooth, "values", "index has the wrong type")),
            }
            if values.len() > capacity {
                return Err(invalid(
                    comb,
                    "teeth",
                    "tooth data exceeds declared capacity",
                ));
            }
        }
    }
    if values.len() != capacity {
        return Err(invalid(
            comb,
            "teeth",
            "tooth data does not match declared capacity",
        ));
    }
    Ok(RawIntegerPool {
        integer_count,
        capacity,
        shift,
        values,
    })
}

fn target<'a>(
    nodes: &'a HashMap<u32, &'a RawNode>,
    definitions: &HashMap<u16, TypeDefinition>,
    source_index: u32,
    node_type: u16,
    name: &'static str,
    index: u32,
    field_name: &'static str,
) -> Result<&'a RawNode, MeshDataError> {
    let source = nodes
        .get(&source_index)
        .copied()
        .ok_or_else(|| invalid_at(source_index, field_name, "source node is missing"))?;
    let node = nodes
        .get(&index)
        .copied()
        .ok_or_else(|| invalid(source, field_name, "pointer target is missing"))?;
    let expected = definitions.get(&node_type).ok_or_else(|| {
        invalid(
            source,
            field_name,
            "type is absent from the reviewed schema",
        )
    })?;
    if node.node_type != node_type
        || expected.node_type != node_type
        || node.definition.name != name
        || node.definition.name != expected.name
        || node.definition.fields != expected.fields
        || node.definition.variable != expected.variable
    {
        return Err(invalid(
            source,
            field_name,
            "pointer target does not match the exact reviewed schema definition",
        ));
    }
    Ok(node)
}

fn field<'a>(
    node: &'a RawNode,
    name: &'static str,
    ty: FieldType,
) -> Result<&'a crate::RawField, MeshDataError> {
    let mut matches = node
        .fields
        .iter()
        .filter(|field| field.definition.name == name);
    let field = matches
        .next()
        .ok_or_else(|| invalid(node, name, "required field is missing"))?;
    if matches.next().is_some()
        || field.definition.field_type != ty
        || !field.definition.transmitted
    {
        return Err(invalid(
            node,
            name,
            "field is ambiguous, non-transmitted, or has the wrong type",
        ));
    }
    Ok(field)
}

fn validate_exact_field(
    node: &RawNode,
    raw_field: &crate::RawField,
    name: &'static str,
) -> Result<(), MeshDataError> {
    match node
        .definition
        .fields
        .iter()
        .find(|definition| definition.name == name)
    {
        Some(expected) if expected == raw_field.definition.as_ref() => Ok(()),
        _ => Err(invalid(
            node,
            name,
            "field does not match the exact reviewed provider definition",
        )),
    }
}

fn double_values<'a>(
    node: &'a RawNode,
    name: &'static str,
) -> Result<&'a [FieldValue], MeshDataError> {
    array_values(node, name, FieldType::Double)
}

fn integer_values<'a>(
    node: &'a RawNode,
    name: &'static str,
) -> Result<&'a [FieldValue], MeshDataError> {
    array_values(node, name, FieldType::Integer)
}

fn array_values<'a>(
    node: &'a RawNode,
    name: &'static str,
    ty: FieldType,
) -> Result<&'a [FieldValue], MeshDataError> {
    let field = field(node, name, ty)?;
    if field.definition.element_count != 1 {
        return Err(invalid(
            node,
            name,
            "field is not variable-length in the schema",
        ));
    }
    Ok(&field.values)
}

fn pointer(node: &RawNode, name: &'static str, target_type: u16) -> Result<u32, MeshDataError> {
    let field = field(node, name, FieldType::PointerIndex)?;
    if field.definition.pointer_class != target_type || field.definition.element_count != 0 {
        return Err(invalid(node, name, "pointer schema type or count is wrong"));
    }
    match field.values.as_slice() {
        [FieldValue::PointerIndex(index)] => Ok(*index),
        _ => Err(invalid(node, name, "pointer must contain one value")),
    }
}

fn optional_pointer(
    node: &RawNode,
    name: &'static str,
    target_type: u16,
) -> Result<Option<u32>, MeshDataError> {
    match pointer(node, name, target_type)? {
        0 => Ok(None),
        index => Ok(Some(index)),
    }
}

fn pointer_array(
    node: &RawNode,
    name: &'static str,
    target_type: u16,
) -> Result<Vec<u32>, MeshDataError> {
    let field = field(node, name, FieldType::PointerIndex)?;
    if field.definition.pointer_class != target_type || field.definition.element_count != 1 {
        return Err(invalid(
            node,
            name,
            "pointer-array schema type or count is wrong",
        ));
    }
    field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::PointerIndex(index) if *index != 0 => Ok(*index),
            _ => Err(invalid(
                node,
                name,
                "tooth link is null or has the wrong type",
            )),
        })
        .collect()
}

fn unsigned(node: &RawNode, name: &'static str) -> Result<u8, MeshDataError> {
    let field = field(node, name, FieldType::UnsignedByte)?;
    if field.definition.element_count != 0 {
        return Err(invalid(node, name, "byte field is not scalar"));
    }
    match field.values.as_slice() {
        [FieldValue::UnsignedByte(value)] => Ok(*value),
        _ => Err(invalid(node, name, "byte field must contain one value")),
    }
}

fn count(node: &RawNode, name: &'static str) -> Result<usize, MeshDataError> {
    let field = field(node, name, FieldType::Integer)?;
    if field.definition.element_count != 0 {
        return Err(invalid(node, name, "count field is not scalar"));
    }
    match field.values.as_slice() {
        [FieldValue::Integer(Some(value))] if *value >= 0 => usize::try_from(*value)
            .map_err(|_| invalid(node, name, "count exceeds addressable size")),
        _ => Err(invalid(node, name, "count must be one nonnegative integer")),
    }
}

fn invalid(node: &RawNode, field: &'static str, reason: &'static str) -> MeshDataError {
    invalid_at(node.index, field, reason)
}

fn invalid_at(node: u32, field: &'static str, reason: &'static str) -> MeshDataError {
    MeshDataError::Invalid {
        node,
        field,
        reason,
    }
}

#[cfg(test)]
#[allow(
    clippy::map_unwrap_or,
    clippy::format_push_string,
    reason = "Fixture indices and values are bounded and kept explicit for regression checks."
)]
#[allow(
    clippy::float_cmp,
    clippy::panic,
    clippy::too_many_lines,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::{DocumentLimits, FieldDefinition, InMemorySchemaProvider, RawField, parse_xt};

    type MeshDataTestResult = Result<(), MeshDataError>;

    #[test]
    fn rejects_unreviewed_schema_before_reading_nodes() {
        assert_eq!(
            decode_embedded_mesh("SCH_3700193_36001_13006", &[], 1),
            Err(MeshDataError::UnsupportedSchema(
                "SCH_3700193_36001_13006".into()
            ))
        );
    }

    fn field(
        name: &str,
        field_type: FieldType,
        pointer_class: u16,
        element_count: u32,
        values: Vec<FieldValue>,
    ) -> RawField {
        RawField {
            definition: FieldDefinition {
                name: name.to_owned(),
                field_type,
                pointer_class,
                element_count,
                transmitted: true,
            }
            .into(),
            values,
            byte_range: 0..0,
        }
    }

    fn ptr(name: &str, class: u16, value: u32) -> RawField {
        field(
            name,
            FieldType::PointerIndex,
            class,
            0,
            vec![FieldValue::PointerIndex(value)],
        )
    }

    fn int(name: &str, value: i32) -> RawField {
        field(
            name,
            FieldType::Integer,
            0,
            0,
            vec![FieldValue::Integer(Some(value))],
        )
    }

    fn character(name: &str, value: u8) -> RawField {
        field(
            name,
            FieldType::Character,
            0,
            0,
            vec![FieldValue::Character(value)],
        )
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "test schema lookup must find the reviewed field defined by its fixture"
    )]
    fn exact_field(
        schema_key: &str,
        node_type: u16,
        name: &str,
        values: Vec<FieldValue>,
    ) -> RawField {
        let definition = reviewed_definition(schema_key, node_type)
            .unwrap()
            .fields
            .into_iter()
            .find(|definition| definition.name == name)
            .unwrap();
        RawField {
            definition: definition.into(),
            values,
            byte_range: 0..0,
        }
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "transform fixture fields are guaranteed by the reviewed schema"
    )]
    fn transform_fixture_node(schema_key: &str, index: u32, precision_index: u32) -> RawNode {
        let mut fields = vec![
            exact_field(
                schema_key,
                100,
                "rotation_matrix",
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                    .into_iter()
                    .map(|value| FieldValue::Double(Some(value)))
                    .collect(),
            ),
            exact_field(
                schema_key,
                100,
                "translation_vector",
                vec![FieldValue::Vector([Some(4.0), Some(5.0), Some(6.0)])],
            ),
            exact_field(
                schema_key,
                100,
                "scale",
                vec![FieldValue::Double(Some(2.0))],
            ),
            exact_field(schema_key, 100, "flag", vec![FieldValue::Integer(Some(3))]),
            exact_field(
                schema_key,
                100,
                "perspective_vector",
                vec![FieldValue::Vector([None, None, None])],
            ),
        ];
        if reviewed_definition(schema_key, 100)
            .unwrap()
            .fields
            .iter()
            .any(|definition| definition.name == "precision")
        {
            fields.push(exact_field(
                schema_key,
                100,
                "precision",
                vec![FieldValue::PointerIndex(precision_index)],
            ));
        }
        node(schema_key, index, 100, fields)
    }

    fn precision_fixture_node(schema_key: &str, index: u32) -> RawNode {
        node(
            schema_key,
            index,
            229,
            vec![
                exact_field(
                    schema_key,
                    229,
                    "rotation_matrix",
                    [0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                ),
                exact_field(
                    schema_key,
                    229,
                    "translation_vector",
                    vec![FieldValue::Vector([Some(0.5), Some(0.0), Some(0.0)])],
                ),
            ],
        )
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the test fixture references only schema keys and node types defined in its local registry"
    )]
    fn reviewed_definition(schema_key: &str, node_type: u16) -> Option<TypeDefinition> {
        let key = SchemaKey::parse(schema_key).ok()?;
        let catalog = TopSolidV6PsolidCatalogProvider::new().unwrap();
        let profile = catalog.provider_for_key(&key).unwrap();
        let schema = if profile.contains_schema(key.effective()) {
            key.effective()
        } else {
            key.provider_schema()
        };
        profile.type_definition(schema, node_type).cloned()
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "fixture node construction uses definitions guaranteed by the preceding schema lookup"
    )]
    fn node(schema_key: &str, index: u32, node_type: u16, fields: Vec<RawField>) -> RawNode {
        RawNode {
            node_type,
            index,
            variable_length: None,
            definition: reviewed_definition(schema_key, node_type).unwrap().into(),
            first_schema: None,
            fields,
            byte_range: 0..0,
            user_fields: vec![],
        }
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the reviewed mesh fixture requires its known node definitions and fields"
    )]
    fn fixture(schema_key: &str) -> Vec<RawNode> {
        let mesh_type = [118, 201]
            .into_iter()
            .find(|node_type| {
                reviewed_definition(schema_key, *node_type)
                    .is_some_and(|definition| definition.name == "MESH")
            })
            .unwrap();
        let owner_class = reviewed_definition(schema_key, 189)
            .and_then(|definition| {
                definition
                    .fields
                    .into_iter()
                    .find(|field| field.name == "owner")
                    .map(|field| field.pointer_class)
            })
            .unwrap();
        let mut mesh_fields = vec![
            ptr("transform", 100, 0),
            character("sense", b'+'),
            ptr("psm_imesh", 189, 2),
        ];
        if reviewed_definition(schema_key, mesh_type)
            .unwrap()
            .fields
            .iter()
            .any(|field| field.name == "rcv_key")
        {
            mesh_fields.push(ptr("rcv_key", 1041, 0));
        }
        vec![
            node(schema_key, 1, mesh_type, mesh_fields),
            node(
                schema_key,
                2,
                189,
                vec![
                    field(
                        "precision",
                        FieldType::UnsignedByte,
                        0,
                        0,
                        vec![FieldValue::UnsignedByte(1)],
                    ),
                    ptr("owner", owner_class, 0),
                    ptr("position_pool", 193, 3),
                    ptr("normal_pool", 193, 0),
                    ptr("position_indices", 191, 5),
                    field(
                        "normal_type",
                        FieldType::UnsignedByte,
                        0,
                        0,
                        vec![FieldValue::UnsignedByte(1)],
                    ),
                    ptr("normal_indices", 191, 0),
                ],
            ),
            node(
                schema_key,
                3,
                193,
                vec![
                    field(
                        "encoding",
                        FieldType::UnsignedByte,
                        0,
                        0,
                        vec![FieldValue::UnsignedByte(1)],
                    ),
                    int("n_vectors", 2),
                    int("n_max_vectors", 2),
                    int("shift", 1),
                    field(
                        "teeth",
                        FieldType::PointerIndex,
                        192,
                        1,
                        vec![FieldValue::PointerIndex(4)],
                    ),
                ],
            ),
            node(
                schema_key,
                4,
                192,
                vec![field(
                    "values",
                    FieldType::Double,
                    0,
                    1,
                    [2.0, 0.0, 0.0, 0.0, 2.0, 0.0]
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                )],
            ),
            node(
                schema_key,
                5,
                191,
                vec![
                    field(
                        "encoding",
                        FieldType::UnsignedByte,
                        0,
                        0,
                        vec![FieldValue::UnsignedByte(1)],
                    ),
                    int("n_integers", 3),
                    int("n_max_integers", 4),
                    int("n_bits_per_integer", 32),
                    int("shift", 2),
                    field(
                        "teeth",
                        FieldType::PointerIndex,
                        190,
                        1,
                        vec![FieldValue::PointerIndex(6)],
                    ),
                ],
            ),
            node(
                schema_key,
                6,
                190,
                vec![field(
                    "values",
                    FieldType::Integer,
                    0,
                    1,
                    [0, 1, 1, 99]
                        .into_iter()
                        .map(|value| FieldValue::Integer(Some(value)))
                        .collect(),
                )],
            ),
        ]
    }

    #[test]
    fn decodes_embedded_pool_values_and_preserves_capacity_and_raw_indices() -> MeshDataTestResult {
        let result = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)?.ok_or(
            MeshDataError::Invalid {
                node: 1,
                field: "psm_imesh",
                reason: "fixture should contain embedded mesh data",
            },
        )?;
        assert_eq!(result.positions.encoding, 1);
        assert_eq!(result.positions.values, [2.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
        assert_eq!(result.position_indices.values, [0, 1, 1, 99]);
        assert_eq!(result.position_indices.integer_count, 3);
        assert_eq!(result.position_indices.capacity, 4);
        Ok(())
    }

    #[test]
    fn shared_context_decodes_mesh_and_transform_from_one_validated_index() -> MeshDataTestResult {
        let nodes = fixture(SCHEMA_KEYS[0]);
        let context = MeshDecodeContext::new(SCHEMA_KEYS[0], &nodes, 1)?;
        assert!(context.decode_embedded_mesh(1)?.is_some());
        assert!(context.decode_mesh_linked_transform(1)?.is_none());

        let mut duplicate = nodes.clone();
        duplicate.push(nodes[0].clone());
        assert!(matches!(
            MeshDecodeContext::new(SCHEMA_KEYS[0], &duplicate, 1),
            Err(MeshDataError::Invalid {
                field: "node_index",
                reason: "node index is ambiguous",
                ..
            })
        ));
        Ok(())
    }

    fn text_value(value: &FieldValue) -> String {
        match value {
            FieldValue::UnsignedByte(value) => value.to_string(),
            FieldValue::Character(value) => char::from(*value).to_string(),
            FieldValue::Logical(value) => if *value { "T" } else { "F" }.to_owned(),
            FieldValue::ShortInteger(value) => {
                value.map_or_else(|| "?".to_owned(), |v| v.to_string())
            }
            FieldValue::UnicodeCharacter(value) => value.to_string(),
            FieldValue::Integer(value) => value.map_or_else(|| "?".to_owned(), |v| v.to_string()),
            FieldValue::PointerIndex(value) => value.to_string(),
            FieldValue::Tag(value) => value.map_or_else(|| "?".to_owned(), |v| v.to_string()),
            FieldValue::Double(value) => value.map_or_else(|| "?".to_owned(), |v| v.to_string()),
            FieldValue::Interval(values) => composite_text(*values),
            FieldValue::Vector(values) | FieldValue::IntersectionPoint(values) => {
                composite_text(*values)
            }
            FieldValue::Box3(values) => composite_text(*values),
        }
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the formatter fixture supplies exactly the declared array length"
    )]
    fn composite_text<const N: usize>(values: [Option<f64>; N]) -> String {
        if values.iter().all(Option::is_none) {
            "?".to_owned()
        } else {
            values
                .into_iter()
                .map(|value| value.expect("synthetic composite is fully set").to_string())
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "test XT nodes use valid fixture definitions and encodable field values"
    )]
    fn encoded_xt_node(
        schema_key: &str,
        node_type: u16,
        index: u32,
        overrides: &[(&str, Vec<FieldValue>)],
    ) -> String {
        let definition = reviewed_definition(schema_key, node_type).unwrap();
        let variable_length = definition.variable.then(|| {
            let field = definition.fields.last().expect("variable field exists");
            overrides
                .iter()
                .find(|(name, _)| *name == field.name.as_str())
                .map_or(0, |(_, values)| values.len())
        });
        let mut encoded = format!("{} {}", node_type, embedded_full_schema(&definition));
        if let Some(count) = variable_length {
            // The final transmitted flag is a raw logical character; the parser reads
            // variable_length immediately after it, without skipping a separator.
            encoded.push_str(&count.to_string());
            encoded.push(' ');
        } else if definition
            .fields
            .last()
            .is_some_and(|field| field.pointer_class != 0)
        {
            // A final pointer-class field ends in an integer token, which consumes this
            // separator before the node index. A final scalar type code is fixed-width.
            encoded.push(' ');
        }
        encoded.push_str(&index.to_string());
        encoded.push(' ');
        for field in &definition.fields {
            if !field.transmitted {
                continue;
            }
            let count = match field.element_count {
                0 => 1,
                1 => variable_length.unwrap_or(0),
                count => count as usize,
            };
            let values = overrides
                .iter()
                .find(|(name, _)| *name == field.name.as_str())
                .map(|(_, values)| values.clone())
                .unwrap_or_else(|| {
                    (0..count)
                        .map(|_| match field.field_type {
                            FieldType::UnsignedByte => FieldValue::UnsignedByte(0),
                            FieldType::Character => FieldValue::Character(b'+'),
                            FieldType::Logical => FieldValue::Logical(false),
                            FieldType::ShortInteger => FieldValue::ShortInteger(Some(0)),
                            FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(0),
                            FieldType::Integer => FieldValue::Integer(Some(0)),
                            FieldType::PointerIndex => FieldValue::PointerIndex(0),
                            FieldType::OpaquePointer => panic!("unsupported synthetic field"),
                            FieldType::Tag => FieldValue::Tag(Some(0)),
                            FieldType::Double => FieldValue::Double(Some(0.0)),
                            FieldType::Interval => FieldValue::Interval([None; 2]),
                            FieldType::Vector => FieldValue::Vector([None; 3]),
                            FieldType::Box3 => FieldValue::Box3([None; 6]),
                            FieldType::IntersectionPoint => {
                                FieldValue::IntersectionPoint([None; 3])
                            }
                        })
                        .collect()
                });
            assert_eq!(
                values.len(),
                count,
                "field {} has exact schema count",
                field.name
            );
            for value in &values {
                let token = text_value(value);
                encoded.push_str(&token);
                if !matches!(value, FieldValue::Character(_) | FieldValue::Logical(_))
                    && token != "?"
                {
                    encoded.push(' ');
                }
            }
        }
        encoded
    }

    fn embedded_full_schema(definition: &TypeDefinition) -> String {
        let mut schema = format!(
            "{} {} {}{} {}",
            definition.fields.len(),
            definition.name.len(),
            definition.name,
            definition.description.len(),
            definition.description
        );
        for (index, field) in definition.fields.iter().enumerate() {
            schema.push_str(&format!(
                "{} {}{} {}",
                field.name.len(),
                field.name,
                field.pointer_class,
                field.element_count
            ));
            if field.pointer_class == 0 {
                schema.push_str(&format!(
                    " {} {}",
                    field.field_type.code().len(),
                    field.field_type.code()
                ));
            }
            if field.element_count == 1 {
                if field.pointer_class != 0 {
                    schema.push(' ');
                }
                schema.push_str(if field.transmitted { "T" } else { "F" });
            } else if field.pointer_class != 0 && index + 1 < definition.fields.len() {
                schema.push(' ');
            }
        }
        schema
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the reviewed XT fixture must parse and decode before its invariants are asserted"
    )]
    #[test]
    fn parses_exact_profile_xt_mesh_and_requires_caller_selected_index_origin() {
        let schema_key = SCHEMA_KEYS[3];
        let mesh_type = [118, 201]
            .into_iter()
            .find(|node_type| {
                reviewed_definition(schema_key, *node_type)
                    .is_some_and(|definition| definition.name == "MESH")
            })
            .unwrap();

        let modeller = ": TRANSMIT FILE created by modeller version 3700192";
        let mut encoded = format!(
            "T{} {}{} {}300 0 ",
            modeller.len(),
            modeller,
            schema_key.len(),
            schema_key
        );
        // This synthetic caller catalog explicitly declares all six node types absent from
        // its base. Their complete embedded definitions below are copied from the exact
        // reviewed 36001 catalog; this does not assert official 13006 base membership.
        let mut synthetic_provider = InMemorySchemaProvider::new();
        synthetic_provider.add_schema("13006");
        let node_specs = [
            (
                mesh_type,
                1,
                vec![
                    ("sense", vec![FieldValue::Character(b'+')]),
                    ("rcv_key", vec![FieldValue::PointerIndex(0)]),
                    ("psm_imesh", vec![FieldValue::PointerIndex(2)]),
                ],
            ),
            (
                189,
                2,
                vec![
                    ("precision", vec![FieldValue::UnsignedByte(1)]),
                    ("position_pool", vec![FieldValue::PointerIndex(3)]),
                    ("normal_pool", vec![FieldValue::PointerIndex(0)]),
                    ("position_indices", vec![FieldValue::PointerIndex(5)]),
                    ("normal_type", vec![FieldValue::UnsignedByte(1)]),
                    ("normal_indices", vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            (
                193,
                3,
                vec![
                    ("encoding", vec![FieldValue::UnsignedByte(1)]),
                    ("n_vectors", vec![FieldValue::Integer(Some(4))]),
                    ("n_max_vectors", vec![FieldValue::Integer(Some(4))]),
                    ("shift", vec![FieldValue::Integer(Some(2))]),
                    ("teeth", vec![FieldValue::PointerIndex(4)]),
                ],
            ),
            (
                192,
                4,
                vec![(
                    "values",
                    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                )],
            ),
            (
                191,
                5,
                vec![
                    ("encoding", vec![FieldValue::UnsignedByte(1)]),
                    ("n_integers", vec![FieldValue::Integer(Some(3))]),
                    ("n_max_integers", vec![FieldValue::Integer(Some(4))]),
                    ("n_bits_per_integer", vec![FieldValue::Integer(Some(32))]),
                    ("shift", vec![FieldValue::Integer(Some(2))]),
                    ("teeth", vec![FieldValue::PointerIndex(6)]),
                ],
            ),
            (
                190,
                6,
                vec![(
                    "values",
                    [1, 2, 3, 99]
                        .into_iter()
                        .map(|value| FieldValue::Integer(Some(value)))
                        .collect(),
                )],
            ),
        ];
        for (node_type, index, fields) in node_specs {
            encoded.push_str(&encoded_xt_node(schema_key, node_type, index, &fields));
        }
        encoded.push_str("1 0 ");

        let document = parse_xt(
            encoded.as_bytes(),
            &synthetic_provider,
            DocumentLimits::default(),
        )
        .unwrap();
        let zero_based = decode_mesh_triangles(
            schema_key,
            &document.nodes,
            1,
            MeshIndexOrigin::ZeroBased,
            Sense::Positive,
        )
        .unwrap()
        .unwrap();
        let one_based = decode_mesh_triangles(
            schema_key,
            &document.nodes,
            1,
            MeshIndexOrigin::OneBased,
            Sense::Positive,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            zero_based,
            vec![[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]]
        );
        assert_eq!(
            one_based,
            vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]
        );
    }

    #[test]
    fn decodes_reviewed_mesh_node_201_for_32001_and_36001_profiles() -> MeshDataTestResult {
        for schema_key in [SCHEMA_KEYS[1], SCHEMA_KEYS[3]] {
            let mesh_type = [118, 201]
                .into_iter()
                .find(|node_type| {
                    reviewed_definition(schema_key, *node_type)
                        .is_some_and(|definition| definition.name == "MESH")
                })
                .ok_or(MeshDataError::Invalid {
                    node: 1,
                    field: "MESH",
                    reason: "reviewed profile should define MESH",
                })?;
            assert_eq!(mesh_type, 201);
            let result = decode_embedded_mesh(schema_key, &fixture(schema_key), 1)?.ok_or(
                MeshDataError::Invalid {
                    node: 1,
                    field: "psm_imesh",
                    reason: "fixture should contain embedded mesh data",
                },
            )?;
            assert_eq!(result.mesh_index, 1);
            assert_eq!(result.position_indices.values[..3], [0, 1, 1]);
        }
        Ok(())
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "fixture placement uses a known mesh, transform, and valid triangle indices"
    )]
    #[test]
    fn placement_requires_matching_link_and_applies_precision_and_sense() {
        let mut nodes = fixture(SCHEMA_KEYS[3]);
        nodes[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(7)];
        nodes.push(node(SCHEMA_KEYS[3], 7, 100, vec![]));
        let mut mesh = decode_embedded_mesh(SCHEMA_KEYS[3], &nodes, 1)
            .unwrap()
            .unwrap();
        mesh.position_indices.values[..3].copy_from_slice(&[0, 1, 0]);
        assert_eq!(mesh.transform_index, Some(7));
        assert_eq!(mesh.surface_sense, Sense::Positive);

        let transform = TransformNode {
            index: 7,
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            translation: [0.0; 3],
            scale: 1.0,
            flags: 0,
            perspective: [None; 3],
            precision: Some(super::super::assembly::TransformPrecision {
                index: 8,
                rotation_correction: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                translation_correction: [0.0; 3],
            }),
            byte_range: 0..0,
        };
        assert!(matches!(
            mesh.placed_triangles(MeshIndexOrigin::ZeroBased, None, Sense::Positive),
            Err(MeshDataError::Invalid {
                field: "transform",
                ..
            })
        ));
        let placed = mesh
            .placed_triangles(
                MeshIndexOrigin::ZeroBased,
                Some(&transform),
                Sense::Negative,
            )
            .unwrap();
        assert_eq!(placed[0][0], [3.0, 0.0, 0.0]);
        assert_eq!(placed[0][1], [3.0, 0.0, 0.0]);
        assert_eq!(placed[0][2], [0.0, 2.0, 0.0]);
        assert!(matches!(
            mesh.placed_triangles(
                MeshIndexOrigin::ZeroBased,
                Some(&TransformNode {
                    index: 9,
                    ..transform.clone()
                }),
                Sense::Positive
            ),
            Err(MeshDataError::Invalid {
                field: "transform",
                ..
            })
        ));
        assert!(matches!(
            mesh.placed_triangles(MeshIndexOrigin::ZeroBased, Some(&transform), Sense::Unknown),
            Err(MeshDataError::Invalid { field: "sense", .. })
        ));
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the exact-profile fixture must contain its linked transform and precision fields"
    )]
    #[test]
    fn decodes_linked_transform_and_precision_from_exact_profile() {
        let schema_key = SCHEMA_KEYS[3];
        let mut nodes = fixture(schema_key);
        nodes[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(7)];
        nodes.push(transform_fixture_node(schema_key, 7, 8));
        nodes.push(precision_fixture_node(schema_key, 8));

        let transform = decode_mesh_linked_transform(schema_key, &nodes, 1)
            .unwrap()
            .unwrap();
        assert_eq!(transform.index, 7);
        assert_eq!(transform.rotation[0], 1.0);
        assert_eq!(transform.translation, [4.0, 5.0, 6.0]);
        assert_eq!(transform.scale, 2.0);
        assert_eq!(transform.flags, 3);
        assert_eq!(transform.perspective, [None; 3]);
        let precision = transform.precision.as_ref().unwrap();
        assert_eq!(precision.index, 8);
        assert_eq!(precision.rotation_correction[0], 0.25);
        assert_eq!(precision.translation_correction, [0.5, 0.0, 0.0]);
        assert_eq!(
            transform.transform_point([1.0, 0.0, 0.0]).unwrap(),
            [11.5, 10.0, 12.0]
        );
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the fixture contains a valid transform while invalid target cases are asserted separately"
    )]
    #[test]
    fn transform_decoder_preserves_profile_without_precision_link_and_rejects_bad_targets() {
        let schema_key = SCHEMA_KEYS[1];
        let mut nodes = fixture(schema_key);
        nodes[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(7)];
        nodes.push(transform_fixture_node(schema_key, 7, 0));
        let transform = decode_mesh_linked_transform(schema_key, &nodes, 1)
            .unwrap()
            .unwrap();
        assert_eq!(transform.precision, None);
        assert_eq!(
            transform.transform_point([1.0, 0.0, 0.0]).unwrap(),
            [10.0, 10.0, 12.0]
        );

        let mut bad = nodes.clone();
        bad[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(99)];
        assert!(decode_mesh_linked_transform(schema_key, &bad, 1).is_err());

        let mut missing_precision = fixture(SCHEMA_KEYS[3]);
        missing_precision[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(7)];
        let mut transform_node = transform_fixture_node(SCHEMA_KEYS[3], 7, 8);
        transform_node
            .fields
            .retain(|field| field.definition.name != "precision");
        missing_precision.push(transform_node);
        missing_precision.push(precision_fixture_node(SCHEMA_KEYS[3], 8));
        assert!(decode_mesh_linked_transform(SCHEMA_KEYS[3], &missing_precision, 1).is_err());
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the one-step fixture must contain the requested mesh and linked transform"
    )]
    #[test]
    fn one_step_mesh_decode_uses_explicit_origin_face_sense_and_linked_transform() {
        let schema_key = SCHEMA_KEYS[3];
        let mut nodes = fixture(schema_key);
        nodes[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(7)];
        nodes[5].fields[0].values = [0, 1, 0, 99]
            .into_iter()
            .map(|value| FieldValue::Integer(Some(value)))
            .collect();
        nodes.push(transform_fixture_node(schema_key, 7, 8));
        nodes.push(precision_fixture_node(schema_key, 8));

        let triangles = decode_mesh_triangles(
            schema_key,
            &nodes,
            1,
            MeshIndexOrigin::ZeroBased,
            Sense::Negative,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            triangles[0],
            [[14.0, 10.0, 12.0], [14.0, 10.0, 12.0], [9.0, 14.0, 12.0]]
        );
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the fixture supplies the fields needed to construct each explicit rejection case"
    )]
    #[test]
    fn one_step_mesh_decode_keeps_absent_data_and_rejects_bad_links_or_senses() {
        let schema_key = SCHEMA_KEYS[0];
        let mut no_embedded_data = fixture(schema_key);
        no_embedded_data[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "psm_imesh")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(0)];
        assert_eq!(
            decode_mesh_triangles(
                schema_key,
                &no_embedded_data,
                1,
                MeshIndexOrigin::ZeroBased,
                Sense::Positive
            ),
            Ok(None)
        );

        let mut bad_transform_link = fixture(schema_key);
        bad_transform_link[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "transform")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(99)];
        assert!(
            decode_mesh_triangles(
                schema_key,
                &bad_transform_link,
                1,
                MeshIndexOrigin::ZeroBased,
                Sense::Positive
            )
            .is_err()
        );

        let mut unknown_sense = fixture(schema_key);
        unknown_sense[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "sense")
            .unwrap()
            .values = vec![FieldValue::Character(b'?')];
        assert!(matches!(
            decode_mesh_triangles(
                schema_key,
                &unknown_sense,
                1,
                MeshIndexOrigin::ZeroBased,
                Sense::Positive
            ),
            Err(MeshDataError::Invalid { field: "sense", .. })
        ));
        assert!(matches!(
            decode_mesh_triangles(
                schema_key,
                &fixture(schema_key),
                1,
                MeshIndexOrigin::ZeroBased,
                Sense::Unknown
            ),
            Err(MeshDataError::Invalid { field: "sense", .. })
        ));
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the reviewed mesh fixture contains valid raw coordinates and indices"
    )]
    #[test]
    fn null_transform_keeps_raw_coordinates_and_unknown_surface_sense_fails() {
        let mut mesh = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        assert_eq!(mesh.transform_index, None);
        let raw = mesh
            .placed_triangles(MeshIndexOrigin::ZeroBased, None, Sense::Positive)
            .unwrap();
        assert_eq!(raw[0], [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 2.0, 0.0]]);
        mesh.surface_sense = Sense::Unknown;
        assert!(matches!(
            mesh.placed_triangles(MeshIndexOrigin::ZeroBased, None, Sense::Positive),
            Err(MeshDataError::Invalid { field: "sense", .. })
        ));
    }

    #[test]
    fn rejects_base_only_profile_without_reviewed_mesh_definitions() {
        assert!(matches!(
            decode_embedded_mesh(SCHEMA_KEYS[4], &[], 1),
            Err(MeshDataError::Invalid {
                field: "schema",
                reason: "exact profile has no reviewed MESH definition",
                ..
            })
        ));
    }

    #[test]
    fn rejects_wrong_link_type_and_missing_target() {
        let mut wrong_type = fixture(SCHEMA_KEYS[0]);
        wrong_type[5].node_type = 192;
        assert!(decode_embedded_mesh(SCHEMA_KEYS[0], &wrong_type, 1).is_err());

        let mut missing = fixture(SCHEMA_KEYS[0]);
        missing.pop();
        assert!(decode_embedded_mesh(SCHEMA_KEYS[0], &missing, 1).is_err());
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the receiver-key fixture has the reviewed field required for the targeted mutations"
    )]
    #[test]
    fn rcv_key_accepts_null_effective_indices_and_rejects_resolved_targets() {
        let schema_key = SCHEMA_KEYS[3];
        let explicit_null = fixture(schema_key);
        assert!(decode_embedded_mesh(schema_key, &explicit_null, 1).is_ok());

        let mut missing_target = fixture(schema_key);
        let rcv_key = missing_target[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "rcv_key")
            .unwrap();
        rcv_key.values = vec![FieldValue::PointerIndex(u32::MAX)];
        assert!(decode_embedded_mesh(schema_key, &missing_target, 1).is_ok());
        assert_eq!(
            missing_target[0]
                .fields
                .iter()
                .find(|field| field.definition.name == "rcv_key")
                .unwrap()
                .values,
            [FieldValue::PointerIndex(u32::MAX)]
        );

        let mut resolved_target = fixture(schema_key);
        resolved_target[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "rcv_key")
            .unwrap()
            .values = vec![FieldValue::PointerIndex(2)];
        assert!(matches!(
            decode_embedded_mesh(schema_key, &resolved_target, 1),
            Err(MeshDataError::Invalid {
                field: "rcv_key",
                reason: "unused receiver key resolves to a transmitted node",
                ..
            })
        ));
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the receiver-key fixture contains the field changed to exercise invalid definitions"
    )]
    #[test]
    fn rcv_key_rejects_wrong_type_and_ambiguous_fields() {
        let schema_key = SCHEMA_KEYS[3];
        let mut wrong_type = fixture(schema_key);
        let rcv_key = wrong_type[0]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "rcv_key")
            .unwrap();
        std::sync::Arc::make_mut(&mut rcv_key.definition).field_type = FieldType::Integer;
        rcv_key.values = vec![FieldValue::Integer(Some(0))];
        assert!(matches!(
            decode_embedded_mesh(schema_key, &wrong_type, 1),
            Err(MeshDataError::Invalid {
                field: "rcv_key",
                ..
            })
        ));

        let mut ambiguous = fixture(schema_key);
        ambiguous[0].fields.push(ptr("rcv_key", 1041, 0));
        assert!(matches!(
            decode_embedded_mesh(schema_key, &ambiguous, 1),
            Err(MeshDataError::Invalid {
                field: "rcv_key",
                ..
            })
        ));
    }

    #[test]
    fn rejects_negative_raw_index() {
        let mut nodes = fixture(SCHEMA_KEYS[0]);
        nodes[5].fields[0].values[0] = FieldValue::Integer(Some(-1));
        assert!(decode_embedded_mesh(SCHEMA_KEYS[0], &nodes, 1).is_err());
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the reviewed mesh fixture is valid for both explicit index-origin assertions"
    )]
    #[test]
    fn triangle_conversion_uses_the_explicit_index_origin() {
        let raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        let zero_based = raw
            .triangles_with_index_origin(MeshIndexOrigin::ZeroBased)
            .unwrap();
        assert_eq!(
            zero_based[0],
            [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 2.0, 0.0]]
        );

        let mut one_based_raw = raw;
        one_based_raw.position_indices.values[..3].copy_from_slice(&[1, 2, 2]);
        let one_based = one_based_raw
            .triangles_with_index_origin(MeshIndexOrigin::OneBased)
            .unwrap();
        assert_eq!(one_based, zero_based);
        assert_eq!(one_based_raw.position_indices.values[..3], [1, 2, 2]);
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the fixture indices are deliberately valid under both selected origins"
    )]
    #[test]
    fn triangle_conversion_does_not_resolve_values_valid_under_both_origins() {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.positions.vector_count = 4;
        raw.positions.capacity = 4;
        raw.positions.values.extend([1.0, 1.0, 1.0, 3.0, 3.0, 3.0]);
        raw.position_indices.integer_count = 3;
        raw.position_indices.capacity = 3;
        raw.position_indices.values = vec![1, 2, 3];

        let zero_based = raw
            .triangles_with_index_origin(MeshIndexOrigin::ZeroBased)
            .unwrap();
        let one_based = raw
            .triangles_with_index_origin(MeshIndexOrigin::OneBased)
            .unwrap();
        assert_ne!(zero_based, one_based);
        assert_eq!(raw.position_indices.values, [1, 2, 3]);
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the initial fixture is valid before each explicit invalid-index mutation"
    )]
    #[test]
    fn triangle_conversion_rejects_indices_outside_the_selected_origin() {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.position_indices.values[0] = 2;
        assert!(
            raw.triangles_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        raw.position_indices.values[0] = 0;
        assert!(
            raw.triangles_with_index_origin(MeshIndexOrigin::OneBased)
                .is_err()
        );
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the fixture mesh is valid before its deliberate index-origin mutations"
    )]
    #[test]
    fn triangle_conversion_infers_only_a_unique_index_origin() {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.positions.vector_count = 4;
        raw.positions.capacity = 4;
        raw.positions.values.extend([3.0, 3.0, 3.0, 4.0, 4.0, 4.0]);
        raw.position_indices.integer_count = 3;
        raw.position_indices.capacity = 3;

        raw.position_indices.values = vec![0, 1, 1];
        assert_eq!(
            raw.triangles_with_inferred_index_origin().unwrap()[0],
            [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 2.0, 0.0]]
        );
        raw.position_indices.values = vec![1, 4, 4];
        assert_eq!(
            raw.triangles_with_inferred_index_origin().unwrap()[0],
            [[2.0, 0.0, 0.0], [4.0, 4.0, 4.0], [4.0, 4.0, 4.0]]
        );

        raw.position_indices.values = vec![1, 2, 3];
        assert!(matches!(
            raw.triangles_with_inferred_index_origin(),
            Err(MeshDataError::Invalid {
                field: "position_indices",
                reason: "index origin is ambiguous",
                ..
            })
        ));
        raw.position_indices.values = vec![0, 5, 5];
        assert!(matches!(
            raw.triangles_with_inferred_index_origin(),
            Err(MeshDataError::Invalid {
                field: "position_indices",
                reason: "index is outside zero-based and one-based ranges",
                ..
            })
        ));
        assert_eq!(raw.position_indices.values, [0, 5, 5]);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the valid fixture supports both index-origin conversions before ambiguity is tested"
    )]
    fn placed_triangle_conversion_infers_only_a_unique_index_origin() {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.positions.vector_count = 4;
        raw.positions.capacity = 4;
        raw.positions.values.extend([3.0, 3.0, 3.0, 4.0, 4.0, 4.0]);
        raw.position_indices.integer_count = 3;
        raw.position_indices.capacity = 3;

        raw.position_indices.values = vec![0, 1, 1];
        let zero_based = raw.position_indices.values.clone();
        assert_eq!(
            raw.placed_triangles_with_inferred_index_origin(None, Sense::Positive)
                .unwrap()[0],
            [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 2.0, 0.0]]
        );
        assert_eq!(raw.position_indices.values, zero_based);

        raw.position_indices.values = vec![1, 4, 4];
        let one_based = raw.position_indices.values.clone();
        assert_eq!(
            raw.placed_triangles_with_inferred_index_origin(None, Sense::Positive)
                .unwrap()[0],
            [[2.0, 0.0, 0.0], [4.0, 4.0, 4.0], [4.0, 4.0, 4.0]]
        );
        assert_eq!(raw.position_indices.values, one_based);

        raw.position_indices.values = vec![1, 2, 3];
        let ambiguous = raw.position_indices.values.clone();
        assert!(matches!(
            raw.placed_triangles_with_inferred_index_origin(None, Sense::Positive),
            Err(MeshDataError::Invalid {
                field: "position_indices",
                reason: "index origin is ambiguous",
                ..
            })
        ));
        assert_eq!(raw.position_indices.values, ambiguous);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the fixture mesh is valid before encoding and non-finite position failures are introduced"
    )]
    fn triangle_conversion_requires_finite_cartesian_positions() {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.positions.encoding = 2;
        assert!(
            raw.triangles_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        raw.positions.encoding = 1;
        raw.positions.values[0] = f64::INFINITY;
        assert!(
            raw.triangles_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the normal fixture requires the reviewed embedded mesh data"
    )]
    fn mesh_with_raw_per_corner_normals() -> RawMeshData {
        let mut raw = decode_embedded_mesh(SCHEMA_KEYS[0], &fixture(SCHEMA_KEYS[0]), 1)
            .unwrap()
            .unwrap();
        raw.positions.vector_count = 4;
        raw.positions.capacity = 4;
        raw.positions.values.extend([0.0, 0.0, 1.0, 0.0, 0.0, 2.0]);
        raw.position_indices.values[..3].copy_from_slice(&[1, 2, 3]);
        raw.normal_type = 3;
        raw.normals = Some(RawVectorPool {
            encoding: 2,
            vector_count: 4,
            capacity: 4,
            shift: 2,
            values: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
        });
        raw.normal_indices = Some(RawIntegerPool {
            integer_count: 3,
            capacity: 4,
            shift: 2,
            values: vec![1, 2, 3, 99],
        });
        raw
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "the normal fixture supplies valid linked pools for both explicit index origins"
    )]
    fn per_corner_normal_parameters_follow_the_explicit_index_origin() {
        let raw = mesh_with_raw_per_corner_normals();
        let raw_normals = raw.normals.clone();
        let raw_indices = raw.normal_indices.clone();

        assert_eq!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .unwrap(),
            vec![[[0.3, 0.4], [0.5, 0.6], [0.7, 0.8]]]
        );
        assert_eq!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::OneBased)
                .unwrap(),
            vec![[[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]]
        );
        assert_eq!(raw.normals, raw_normals);
        assert_eq!(raw.normal_indices, raw_indices);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "each malformed-normal case starts from the same valid linked-pool fixture"
    )]
    fn per_corner_normal_parameters_reject_missing_links_and_malformed_indices() {
        let mut raw = mesh_with_raw_per_corner_normals();
        raw.normals = None;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.normal_indices = None;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.normal_indices.as_mut().unwrap().values[0] = 4;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.normal_indices.as_mut().unwrap().integer_count = 2;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.normals.as_mut().unwrap().encoding = 1;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.position_indices.values[0] = 4;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.position_indices.values[0] = 0;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::OneBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.positions.capacity = 3;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );

        let mut raw = mesh_with_raw_per_corner_normals();
        raw.position_indices.capacity = 3;
        assert!(
            raw.per_corner_normal_parameters_with_index_origin(MeshIndexOrigin::ZeroBased)
                .is_err()
        );
    }
}
