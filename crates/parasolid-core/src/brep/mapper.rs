//! Strict conversion from schema-framed raw nodes to the native B-Rep model.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use crate::{
    ErrorDetails, ErrorKind, FieldValue, ParseError, RawField, RawNode, XbDocument, XtDocument,
};

use super::metrics::derive_metrics;
use super::model::{
    Assembly, AssemblyKind, Attribute, AttributeField, AttributeOwner, BlendType, Body, BodyKind,
    BrepDiagnostic, BrepId, BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind,
    Edge, Face, HalfEdge, Instance, InstancePart, IntersectionChartParameters, IntersectionData,
    IntersectionLimitKind, IntersectionUvCoordinates, IntersectionUvType, Loop, NurbsCurve,
    NurbsSurface, PointGeometry, Region, RegionKind, Sense, Shell, SourceNodeRef, SurfaceGeometry,
    SurfaceKind, SweptSurfaceData, TopologyValidation, Transform, Vector3, Vertex,
};
use super::profile_roles::RoleAccess;

/// Default modeller linear precision, in metres: the bound within which a trim
/// point with unset parameters must lie on its basis line.
const LINE_TRIM_TOLERANCE: f64 = 1.0e-8;
const TRIM_PARAMETER_TOLERANCE: f64 = 1e-6;

fn nurbs_parameter_range(
    knots: &[f64],
    multiplicities: &[u16],
    control_count: u32,
    degree: u16,
) -> Option<(f64, f64)> {
    let knot_at = |index: u64| {
        let mut end = 0_u64;
        knots
            .iter()
            .zip(multiplicities)
            .find_map(|(&knot, &count)| {
                end += u64::from(count);
                (index < end).then_some(knot)
            })
    };
    Some((
        knot_at(u64::from(degree))?,
        knot_at(u64::from(control_count))?,
    ))
}

/// Map a complete `X_B` raw document into the Parasolid-native B-Rep model.
///
/// # Errors
///
/// Returns a strict semantic error for missing bodies, malformed required fields,
/// unresolved typed references, inconsistent NURBS arrays, or invalid topology rings.
pub fn map_xb_brep(document: &XbDocument) -> Result<BrepModel, ParseError> {
    map_xb_brep_with_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_B` document while bounding retained non-fatal diagnostics.
///
/// # Errors
///
/// Returns a strict semantic error, an invalid-limit error for zero, or a
/// limit error when mapping would retain more diagnostics than requested.
pub fn map_xb_brep_with_diagnostic_limit(
    document: &XbDocument,
    max_diagnostics: usize,
) -> Result<BrepModel, ParseError> {
    map_xb_brep_with_options(document, max_diagnostics, false)
}

/// Map an `X_B` document, retaining geometry when optional semantic metadata decoding fails.
///
/// If any attribute is malformed, all semantic attributes are omitted. If any assembly or
/// instance is malformed, both collections are omitted. Each failure adds a diagnostic;
/// geometry and topology errors remain strict.
///
/// # Errors
/// Returns a schema/provider resolution error, a strict geometry/topology error (including
/// missing bodies, malformed required fields, unresolved required references, inconsistent
/// NURBS arrays, or invalid topology rings), or a limit error if diagnostics exceed the
/// unbounded entry point's capacity.
pub fn map_xb_brep_with_metadata_recovery(document: &XbDocument) -> Result<BrepModel, ParseError> {
    map_xb_brep_with_metadata_recovery_and_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_B` document with metadata recovery and a bound on retained diagnostics.
///
/// # Errors
/// Returns an invalid-limit error for zero, a limit error when geometry and metadata
/// diagnostics exceed the bound, or a strict geometry/topology error.
pub fn map_xb_brep_with_metadata_recovery_and_diagnostic_limit(
    document: &XbDocument,
    max_diagnostics: usize,
) -> Result<BrepModel, ParseError> {
    map_xb_brep_with_options(document, max_diagnostics, true)
}

fn map_xb_brep_with_options(
    document: &XbDocument,
    max_diagnostics: usize,
    recover_metadata: bool,
) -> Result<BrepModel, ParseError> {
    validate_diagnostic_limit(max_diagnostics)?;
    let mapper = Mapper::new(
        BrepSourceFormat::Binary,
        document.schema_key.raw(),
        &document.nodes,
        RoleAccess::select(
            &document.schema_provider,
            document.schema_key.raw(),
            &document.schemas,
        )?,
    )
    .with_max_diagnostics(max_diagnostics);
    if recover_metadata {
        mapper.map_with_metadata_recovery()
    } else {
        mapper.map()
    }
}

/// Map a complete `X_T` raw document into the Parasolid-native B-Rep model.
///
/// # Errors
///
/// Returns a strict semantic error for missing bodies, malformed required fields,
/// unresolved typed references, inconsistent NURBS arrays, or invalid topology rings.
pub fn map_xt_brep(document: &XtDocument) -> Result<BrepModel, ParseError> {
    map_xt_brep_with_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_T` document while bounding retained non-fatal diagnostics.
///
/// # Errors
///
/// Returns a strict semantic error, an invalid-limit error for zero, or a
/// limit error when mapping would retain more diagnostics than requested.
pub fn map_xt_brep_with_diagnostic_limit(
    document: &XtDocument,
    max_diagnostics: usize,
) -> Result<BrepModel, ParseError> {
    map_xt_brep_with_options(document, max_diagnostics, false)
}

/// Map an `X_T` document, retaining geometry when optional semantic metadata decoding fails.
///
/// If any attribute is malformed, all semantic attributes are omitted. If any assembly or
/// instance is malformed, both collections are omitted. Each failure adds a diagnostic;
/// geometry and topology errors remain strict.
///
/// # Errors
/// Returns a schema/provider resolution error, a strict geometry/topology error (including
/// missing bodies, malformed required fields, unresolved required references, inconsistent
/// NURBS arrays, or invalid topology rings), or a limit error if diagnostics exceed the
/// unbounded entry point's capacity.
pub fn map_xt_brep_with_metadata_recovery(document: &XtDocument) -> Result<BrepModel, ParseError> {
    map_xt_brep_with_metadata_recovery_and_diagnostic_limit(document, usize::MAX)
}

/// Map an `X_T` document with metadata recovery and a bound on retained diagnostics.
///
/// # Errors
/// Returns an invalid-limit error for zero, a limit error when geometry and metadata
/// diagnostics exceed the bound, or a strict geometry/topology error.
pub fn map_xt_brep_with_metadata_recovery_and_diagnostic_limit(
    document: &XtDocument,
    max_diagnostics: usize,
) -> Result<BrepModel, ParseError> {
    map_xt_brep_with_options(document, max_diagnostics, true)
}

fn map_xt_brep_with_options(
    document: &XtDocument,
    max_diagnostics: usize,
    recover_metadata: bool,
) -> Result<BrepModel, ParseError> {
    validate_diagnostic_limit(max_diagnostics)?;
    let mapper = Mapper::new(
        BrepSourceFormat::Text,
        document.schema_key.raw(),
        &document.nodes,
        RoleAccess::select(
            &document.schema_provider,
            document.schema_key.raw(),
            &document.schemas,
        )?,
    )
    .with_max_diagnostics(max_diagnostics);
    if recover_metadata {
        mapper.map_with_metadata_recovery()
    } else {
        mapper.map()
    }
}

/// Decode the optional UV data linked by one `X_T` `INTERSECTION` node.
///
/// `intersection_index` is the node's transmit index in the already parsed document.
/// The curve's chart evaluator parameters remain available through `CurveKind::Intersection`.
///
/// # Errors
/// Returns an error for a missing/non-intersection node, malformed linked records,
/// unresolved or wrongly typed links, or an invalid value count for a known UV mode.
pub fn map_xt_intersection_data(
    document: &XtDocument,
    intersection_index: u32,
) -> Result<Option<IntersectionData>, ParseError> {
    let roles = RoleAccess::select(
        &document.schema_provider,
        document.schema_key.raw(),
        &document.schemas,
    )?;
    let mapper = Mapper::new(
        BrepSourceFormat::Text,
        document.schema_key.raw(),
        &document.nodes,
        roles,
    );
    let intersection = mapper
        .nodes
        .get(&intersection_index)
        .copied()
        .ok_or_else(|| {
            ParseError::new(
                ErrorKind::InvalidBrepField,
                0,
                format!("X_T node {intersection_index} does not exist"),
                ErrorDetails::NodeIndex {
                    node_index: intersection_index,
                },
            )
        })?;
    if mapper.roles.type_name(intersection) != "INTERSECTION" {
        return Err(mapper.invalid_field(
            intersection,
            "node_type",
            "requested node is not an INTERSECTION",
        ));
    }
    mapper.map_intersection_uv_data_for(intersection)
}

/// Decode the raw nullable fields from one `X_T` `SWEPT_SURF` node.
///
/// `surface_index` is the node's transmit index in the parsed document. The
/// returned section is its raw pointer target, not a resolved B-Rep ID.
///
/// # Errors
/// Returns an error for a missing/non-swept node or malformed `section`, `sweep`,
/// or `scale` fields.
pub fn map_xt_swept_data(
    document: &XtDocument,
    surface_index: u32,
) -> Result<SweptSurfaceData, ParseError> {
    let roles = RoleAccess::select(
        &document.schema_provider,
        document.schema_key.raw(),
        &document.schemas,
    )?;
    let mapper = Mapper::new(
        BrepSourceFormat::Text,
        document.schema_key.raw(),
        &document.nodes,
        roles,
    );
    let surface = mapper.nodes.get(&surface_index).copied().ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("X_T node {surface_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: surface_index,
            },
        )
    })?;
    if mapper.roles.type_name(surface) != "SWEPT_SURF" {
        return Err(mapper.invalid_field(
            surface,
            "node_type",
            "requested node is not a SWEPT_SURF",
        ));
    }
    let section = match mapper.value(surface, "section")? {
        FieldValue::PointerIndex(0) => None,
        FieldValue::PointerIndex(index) => Some(*index),
        _ => return Err(mapper.invalid_field(surface, "section", "field is not a pointer")),
    };
    let sweep = match mapper.value(surface, "sweep")? {
        FieldValue::Vector(vector) => *vector,
        _ => return Err(mapper.invalid_field(surface, "sweep", "field is not a vector")),
    };
    let scale = match mapper.value(surface, "scale")? {
        FieldValue::Double(scale) => *scale,
        _ => return Err(mapper.invalid_field(surface, "scale", "field is not a double")),
    };
    Ok(SweptSurfaceData {
        source: mapper.source(surface),
        section,
        sweep,
        scale,
    })
}

fn validate_diagnostic_limit(max_diagnostics: usize) -> Result<(), ParseError> {
    if max_diagnostics == 0 {
        return Err(ParseError::invalid_limit("max_diagnostics", 0));
    }
    Ok(())
}

#[derive(Default)]
struct IdMaps {
    assemblies: BTreeMap<u32, BrepId>,
    instances: BTreeMap<u32, BrepId>,
    bodies: BTreeMap<u32, BrepId>,
    regions: BTreeMap<u32, BrepId>,
    shells: BTreeMap<u32, BrepId>,
    faces: BTreeMap<u32, BrepId>,
    loops: BTreeMap<u32, BrepId>,
    half_edges: BTreeMap<u32, BrepId>,
    edges: BTreeMap<u32, BrepId>,
    vertices: BTreeMap<u32, BrepId>,
    points: BTreeMap<u32, BrepId>,
    curves: BTreeMap<u32, BrepId>,
    surfaces: BTreeMap<u32, BrepId>,
    attributes: BTreeMap<u32, BrepId>,
}

impl IdMaps {
    fn new(nodes: &[RawNode], roles: RoleAccess) -> Self {
        Self {
            assemblies: assign_ids(nodes, |node| roles.type_name(node) == "ASSEMBLY"),
            instances: assign_ids(nodes, |node| roles.type_name(node) == "INSTANCE"),
            bodies: assign_ids(nodes, |node| roles.type_name(node) == "BODY"),
            regions: assign_ids(nodes, |node| roles.type_name(node) == "REGION"),
            shells: assign_ids(nodes, |node| roles.type_name(node) == "SHELL"),
            faces: assign_ids(nodes, |node| roles.type_name(node) == "FACE"),
            loops: assign_ids(nodes, |node| roles.type_name(node) == "LOOP"),
            half_edges: assign_ids(nodes, |node| roles.type_name(node) == "HALFEDGE"),
            edges: assign_ids(nodes, |node| roles.type_name(node) == "EDGE"),
            vertices: assign_ids(nodes, |node| roles.type_name(node) == "VERTEX"),
            points: assign_ids(nodes, |node| roles.type_name(node) == "POINT"),
            curves: assign_ids(nodes, |node| roles.is_curve(node)),
            surfaces: assign_ids(nodes, |node| roles.is_surface(node)),
            attributes: assign_ids(nodes, |node| roles.type_name(node) == "ATTRIBUTE"),
        }
    }
}

fn assign_ids(nodes: &[RawNode], predicate: impl Fn(&RawNode) -> bool) -> BTreeMap<u32, BrepId> {
    nodes
        .iter()
        .filter(|node| predicate(node))
        .enumerate()
        .map(|(id, node)| (node.index, u32::try_from(id).unwrap_or(u32::MAX)))
        .collect()
}

struct Mapper<'a> {
    source_format: BrepSourceFormat,
    schema_key: &'a str,
    raw_nodes: &'a [RawNode],
    nodes: BTreeMap<u32, &'a RawNode>,
    duplicate_nodes: BTreeMap<u32, &'a RawNode>,
    mesh_context: Option<super::mesh_data::MeshDecodeContext<'a>>,
    ids: IdMaps,
    roles: RoleAccess,
    diagnostics: RefCell<Vec<BrepDiagnostic>>,
    max_diagnostics: usize,
}

impl<'a> Mapper<'a> {
    fn new(
        source_format: BrepSourceFormat,
        schema_key: &'a str,
        nodes: &'a [RawNode],
        roles: RoleAccess,
    ) -> Self {
        let mut indexed_nodes = BTreeMap::new();
        let mut duplicate_nodes = BTreeMap::new();
        for node in nodes {
            if indexed_nodes.insert(node.index, node).is_some() {
                duplicate_nodes.entry(node.index).or_insert(node);
            }
        }
        Self {
            source_format,
            schema_key,
            raw_nodes: nodes,
            nodes: indexed_nodes,
            duplicate_nodes,
            mesh_context: None,
            ids: IdMaps::new(nodes, roles),
            roles,
            diagnostics: RefCell::new(Vec::new()),
            max_diagnostics: usize::MAX,
        }
    }

    const fn with_max_diagnostics(mut self, max_diagnostics: usize) -> Self {
        self.max_diagnostics = max_diagnostics;
        self
    }

    fn map(self) -> Result<BrepModel, ParseError> {
        self.map_with_policy(false)
    }

    fn map_with_metadata_recovery(self) -> Result<BrepModel, ParseError> {
        self.map_with_policy(true)
    }

    #[allow(clippy::too_many_lines)]
    fn map_with_policy(mut self, recover_metadata: bool) -> Result<BrepModel, ParseError> {
        if self.ids.bodies.is_empty() {
            return Err(ParseError::new(
                ErrorKind::MissingBrepBody,
                0,
                "raw document contains no BODY node",
                ErrorDetails::None,
            ));
        }

        let mut assemblies = Vec::with_capacity(self.ids.assemblies.len());
        let mut instances = Vec::with_capacity(self.ids.instances.len());
        let mut assembly_error = None;
        for index in self.ordered_indices(&self.ids.assemblies) {
            match self.map_assembly(index) {
                Ok(assembly) => assemblies.push(assembly),
                Err(error) if recover_metadata => {
                    assemblies.clear();
                    assembly_error = Some((index, error));
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if assembly_error.is_none() {
            for index in self.ordered_indices(&self.ids.instances) {
                match self.map_instance(index) {
                    Ok(instance) => instances.push(instance),
                    Err(error) if recover_metadata => {
                        assemblies.clear();
                        instances.clear();
                        assembly_error = Some((index, error));
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let assembly_metadata_discarded = assembly_error.is_some();
        let mut bodies = self
            .ordered_indices(&self.ids.bodies)
            .into_iter()
            .map(|index| self.map_body(index))
            .collect::<Result<Vec<_>, _>>()?;
        let regions = self
            .ordered_indices(&self.ids.regions)
            .into_iter()
            .map(|index| self.map_region(index))
            .collect::<Result<Vec<_>, _>>()?;
        let shells = self
            .ordered_indices(&self.ids.shells)
            .into_iter()
            .map(|index| self.map_shell(index))
            .collect::<Result<Vec<_>, _>>()?;
        let mut faces = self
            .ordered_indices(&self.ids.faces)
            .into_iter()
            .map(|index| self.map_face(index))
            .collect::<Result<Vec<_>, _>>()?;
        let loops = self
            .ordered_indices(&self.ids.loops)
            .into_iter()
            .map(|index| self.map_loop(index))
            .collect::<Result<Vec<_>, _>>()?;
        let half_edges = self
            .ordered_indices(&self.ids.half_edges)
            .into_iter()
            .map(|index| self.map_half_edge(index))
            .collect::<Result<Vec<_>, _>>()?;
        let edges = self
            .ordered_indices(&self.ids.edges)
            .into_iter()
            .map(|index| self.map_edge(index))
            .collect::<Result<Vec<_>, _>>()?;
        let vertices = self
            .ordered_indices(&self.ids.vertices)
            .into_iter()
            .map(|index| self.map_vertex(index))
            .collect::<Result<Vec<_>, _>>()?;
        let points = self
            .ordered_indices(&self.ids.points)
            .into_iter()
            .map(|index| self.map_point(index))
            .collect::<Result<Vec<_>, _>>()?;

        let mut curves = Vec::with_capacity(self.ids.curves.len());
        for index in self.ordered_indices(&self.ids.curves) {
            curves.push(self.map_curve(index)?);
        }
        let mut surfaces = Vec::with_capacity(self.ids.surfaces.len());
        for index in self.ordered_indices(&self.ids.surfaces) {
            surfaces.push(self.map_surface(index)?);
        }
        let mut attributes = Vec::with_capacity(self.ids.attributes.len());
        let mut attribute_error = None;
        for index in self.ordered_indices(&self.ids.attributes) {
            let mapped = if assembly_metadata_discarded {
                self.map_attribute_with_metadata_policy(index, true)
            } else {
                self.map_attribute(index)
            };
            match mapped {
                Ok(attribute) => attributes.push(attribute),
                Err(error) if recover_metadata => {
                    attributes.clear();
                    attribute_error = Some((index, error));
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        let topology = self.validate_topology(
            &bodies,
            &regions,
            &shells,
            &faces,
            &loops,
            &half_edges,
            &edges,
            &vertices,
        )?;
        let geometry_complete = topology.valid && self.diagnostics.get_mut().is_empty();

        if let Some((source_index, error)) = assembly_error {
            let Some(source_node) = self.nodes.get(&source_index).copied() else {
                return Err(error);
            };
            let diagnostic = BrepDiagnostic {
                code: "metadata.assembly_decode_failed",
                message: error.message().to_owned(),
                role: "assembly",
                source: self.source(source_node),
            };
            push_diagnostic(
                self.diagnostics.get_mut(),
                self.max_diagnostics,
                error.offset(),
                diagnostic,
            )?;
        }
        if let Some((source_index, error)) = attribute_error {
            let Some(source_node) = self.nodes.get(&source_index).copied() else {
                return Err(error);
            };
            let diagnostic = BrepDiagnostic {
                code: "metadata.attribute_decode_failed",
                message: error.message().to_owned(),
                role: "attribute",
                source: self.source(source_node),
            };
            push_diagnostic(
                self.diagnostics.get_mut(),
                self.max_diagnostics,
                error.offset(),
                diagnostic,
            )?;
        }
        for attribute in &attributes {
            match attribute.owner {
                AttributeOwner::Body(id) => {
                    if let Some(body) = bodies.iter_mut().find(|body| body.id == id) {
                        body.attributes.push(attribute.id);
                    }
                }
                AttributeOwner::Face(id) => {
                    if let Some(face) = faces.iter_mut().find(|face| face.id == id) {
                        face.attributes.push(attribute.id);
                    }
                }
                _ => {}
            }
        }

        let mut model = BrepModel {
            source_format: self.source_format,
            schema_key: self.schema_key.to_owned(),
            complete: false,
            assemblies,
            instances,
            bodies,
            regions,
            shells,
            faces,
            loops,
            half_edges,
            edges,
            vertices,
            points,
            curves,
            surfaces,
            attributes,
            topology,
            metrics: BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            diagnostics: self.diagnostics.into_inner(),
        };
        model.metrics = derive_metrics(&model);
        model.complete = geometry_complete;
        Ok(model)
    }

    #[allow(clippy::unused_self)]
    fn ordered_indices(&self, ids: &BTreeMap<u32, BrepId>) -> Vec<u32> {
        let mut values = ids
            .iter()
            .map(|(index, id)| (*id, *index))
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.into_iter().map(|(_, index)| index).collect()
    }

    fn map_body(&self, index: u32) -> Result<Body, ParseError> {
        let node = self.node(index)?;
        let kind = match self.byte(node, "body_type")? {
            1 => BodyKind::Solid,
            2 => BodyKind::Wire,
            3 => BodyKind::Sheet,
            6 => BodyKind::General,
            value => {
                return Err(self.invalid_field(
                    node,
                    "body_type",
                    format!("unknown body type {value}"),
                ));
            }
        };
        Ok(Body {
            id: self.id(&self.ids.bodies, node, "BODY")?,
            kind,
            size_resolution: self.double(node, "res_size")?,
            linear_resolution: self.double(node, "res_linear")?,
            regions: self.null_chain(node, "region", "REGION", "next", &self.ids.regions)?,
            edges: self.null_chain(node, "edge", "EDGE", "next", &self.ids.edges)?,
            vertices: self.null_chain(node, "vertex", "VERTEX", "next", &self.ids.vertices)?,
            attributes: Vec::new(),
            source: self.source(node),
        })
    }

    fn map_assembly(&self, index: u32) -> Result<Assembly, ParseError> {
        let node = self.node(index)?;
        let kind = match self.byte(node, "type")? {
            1 => AssemblyKind::Collective,
            2 => AssemblyKind::Conjunctive,
            3 => AssemblyKind::Disjunctive,
            value => {
                return Err(self.invalid_field(
                    node,
                    "type",
                    format!("unknown assembly type {value}"),
                ));
            }
        };
        Ok(Assembly {
            id: self.id(&self.ids.assemblies, node, "ASSEMBLY")?,
            kind,
            size_resolution: self.double(node, "res_size")?,
            linear_resolution: self.double(node, "res_linear")?,
            instances: self.null_chain(
                node,
                "sub_instance",
                "INSTANCE",
                "next_in_part",
                &self.ids.instances,
            )?,
            referencing_instances: self.null_chain(
                node,
                "ref_instance",
                "INSTANCE",
                "next_of_part",
                &self.ids.instances,
            )?,
            source: self.source(node),
        })
    }

    fn map_instance(&self, index: u32) -> Result<Instance, ParseError> {
        let node = self.node(index)?;
        let negative = match self.byte(node, "type")? {
            1 => false,
            2 => true,
            value => {
                return Err(self.invalid_field(
                    node,
                    "type",
                    format!("unknown instance type {value}"),
                ));
            }
        };
        let part_index = self.required_pointer(node, "part")?;
        let part_node = self.nodes.get(&part_index).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                "part",
                part_index,
                "PART",
                "required B-Rep pointer is unresolved",
            )
        })?;
        let part = match self.roles.type_name(part_node) {
            "BODY" => InstancePart::Body(self.id_from_target(
                node,
                "part",
                part_index,
                "BODY",
                &self.ids.bodies,
            )?),
            "ASSEMBLY" => InstancePart::Assembly(self.id_from_target(
                node,
                "part",
                part_index,
                "ASSEMBLY",
                &self.ids.assemblies,
            )?),
            _ => {
                return Err(self.invalid_reference(
                    node,
                    "part",
                    part_index,
                    "PART",
                    format!(
                        "pointer targets {} rather than a BODY or ASSEMBLY",
                        part_node.definition.name
                    ),
                ));
            }
        };
        let transform = match self.pointer(node, "transform")? {
            0 => None,
            target => Some(self.map_transform(node, target)?),
        };
        Ok(Instance {
            id: self.id(&self.ids.instances, node, "INSTANCE")?,
            negative,
            part,
            transform,
            assembly: self.required_id(node, "assembly", "ASSEMBLY", &self.ids.assemblies)?,
            source: self.source(node),
        })
    }

    fn map_transform(&self, owner: &RawNode, target: u32) -> Result<Transform, ParseError> {
        let node = self.nodes.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(
                owner,
                "transform",
                target,
                "TRANSFORM",
                "required B-Rep pointer is unresolved",
            )
        })?;
        if self.roles.type_name(node) != "TRANSFORM" {
            return Err(self.invalid_reference(
                owner,
                "transform",
                target,
                "TRANSFORM",
                format!(
                    "pointer targets {} rather than TRANSFORM",
                    node.definition.name
                ),
            ));
        }
        let values = self.double_array(node, "rotation_matrix")?;
        self.validate_count(node, "rotation_matrix", 9, values.len())?;
        let scale = self.double(node, "scale")?;
        if scale == 0.0 {
            return Err(self.invalid_geometry(node, "scale", "transform scale is zero"));
        }
        Ok(Transform {
            rotation: [
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
                [values[6], values[7], values[8]],
            ],
            translation: self.vector(node, "translation_vector")?,
            scale,
            flags: self.nonnegative_integer(node, "flag")?,
            perspective: self
                .optional_vector(node, "perspective_vector")?
                .unwrap_or(Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
            source: self.source(node),
        })
    }

    fn map_region(&self, index: u32) -> Result<Region, ParseError> {
        let node = self.node(index)?;
        let kind = match self.character(node, "type")? {
            b'S' => RegionKind::Solid,
            b'V' => RegionKind::Void,
            value => {
                return Err(self.invalid_field(
                    node,
                    "type",
                    format!("unknown region type 0x{value:02x}"),
                ));
            }
        };
        Ok(Region {
            id: self.id(&self.ids.regions, node, "REGION")?,
            kind,
            body: self.required_id(node, "body", "BODY", &self.ids.bodies)?,
            shells: self.null_chain(node, "shell", "SHELL", "next", &self.ids.shells)?,
            source: self.source(node),
        })
    }

    fn map_shell(&self, index: u32) -> Result<Shell, ParseError> {
        let node = self.node(index)?;
        Ok(Shell {
            id: self.id(&self.ids.shells, node, "SHELL")?,
            region: self.required_id(node, "region", "REGION", &self.ids.regions)?,
            back_faces: self.null_chain(node, "face", "FACE", "next", &self.ids.faces)?,
            front_faces: self.null_chain(
                node,
                "front_face",
                "FACE",
                "next_front",
                &self.ids.faces,
            )?,
            wire_edges: self.null_chain(node, "edge", "EDGE", "next", &self.ids.edges)?,
            isolated_vertex: self.optional_id(node, "vertex", "VERTEX", &self.ids.vertices)?,
            source: self.source(node),
        })
    }

    fn map_face(&self, index: u32) -> Result<Face, ParseError> {
        let node = self.node(index)?;
        Ok(Face {
            id: self.id(&self.ids.faces, node, "FACE")?,
            back_shell: self.required_id(node, "shell", "SHELL", &self.ids.shells)?,
            front_shell: self.required_id(node, "front_shell", "SHELL", &self.ids.shells)?,
            loops: self.null_chain(node, "loop", "LOOP", "next", &self.ids.loops)?,
            surface: self.optional_id(node, "surface", "surface geometry", &self.ids.surfaces)?,
            sense: self.sense(node, "sense")?,
            attributes: Vec::new(),
            source: self.source(node),
        })
    }

    fn map_attribute(&self, index: u32) -> Result<Attribute, ParseError> {
        self.map_attribute_with_metadata_policy(index, false)
    }

    fn map_attribute_with_metadata_policy(
        &self,
        index: u32,
        assembly_metadata_discarded: bool,
    ) -> Result<Attribute, ParseError> {
        let node = self.node(index)?;
        let definition = self.required_typed_node(node, "definition", "ATTRIB_DEF")?;
        let identifier_node = self.required_typed_node(definition, "identifier", "ATT_DEF_ID")?;
        let identifier = self.character_text(identifier_node, "string")?;
        let kind_id = self.optional_integer(definition, "type_id")?;
        let owner_index = self.required_pointer(node, "owner")?;
        let owner_node = self.nodes.get(&owner_index).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                "owner",
                owner_index,
                "raw node",
                "attribute owner pointer is unresolved",
            )
        })?;
        let owner = if assembly_metadata_discarded
            && (self.ids.assemblies.contains_key(&owner_index)
                || self.ids.instances.contains_key(&owner_index))
        {
            AttributeOwner::Other(self.source(owner_node))
        } else {
            self.attribute_owner(owner_node)
        };
        // The definition types each field; an empty field is transmitted as
        // a null pointer (the reference's pointer fields always are).
        let declared_kinds: Vec<u8> = self
            .field(definition, "fields")?
            .values
            .iter()
            .filter_map(|value| match value {
                FieldValue::UnsignedByte(kind) => Some(*kind),
                _ => None,
            })
            .collect();
        let value_targets = self.pointer_array(node, "fields")?;
        if value_targets.len() > declared_kinds.len() {
            return Err(self.invalid_field(
                node,
                "fields",
                "attribute value count exceeds its definition",
            ));
        }
        let trailing_kinds = &declared_kinds[value_targets.len()..];
        if (!trailing_kinds.is_empty() && definition.user_fields.is_empty())
            || trailing_kinds.iter().any(|kind| *kind != 0)
        {
            return Err(self.invalid_field(
                definition,
                "fields",
                "attribute definition has nonzero or unexpected trailing value kinds",
            ));
        }
        let mut fields = Vec::new();
        for (position, target) in value_targets.into_iter().enumerate() {
            fields.push(self.map_attribute_field(node, target, position, &declared_kinds)?);
        }
        Ok(Attribute {
            id: self.id(&self.ids.attributes, node, "ATTRIBUTE")?,
            identifier,
            kind_id,
            owner,
            fields,
            source: self.source(node),
        })
    }

    fn map_attribute_field(
        &self,
        node: &RawNode,
        target: u32,
        position: usize,
        declared_kinds: &[u8],
    ) -> Result<AttributeField, ParseError> {
        if target == 0 {
            return Ok(match declared_kinds.get(position) {
                Some(1) => AttributeField::Integers(Vec::new()),
                Some(2) => AttributeField::Reals(Vec::new()),
                Some(3) => AttributeField::Text(String::new()),
                Some(4) => AttributeField::Points(Vec::new()),
                Some(5) => AttributeField::Vectors(Vec::new()),
                Some(6) => AttributeField::Directions(Vec::new()),
                Some(7) => AttributeField::Axes(Vec::new()),
                Some(8) => AttributeField::Tags(Vec::new()),
                Some(10) => AttributeField::Unicode(String::new()),
                other => AttributeField::Unsupported {
                    type_name: match other {
                        Some(kind) => format!("empty field of kind {kind}"),
                        None => "empty field of undeclared kind".to_owned(),
                    },
                    node_index: 0,
                },
            });
        }

        let value_node = self.nodes.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                "fields",
                target,
                "value node",
                "attribute field pointer is unresolved",
            )
        })?;
        Ok(match self.roles.type_name(value_node) {
            "INT_VALUES" => AttributeField::Integers(self.integer_array(value_node, "values")?),
            "REAL_VALUES" => {
                AttributeField::Reals(self.optional_double_array(value_node, "values")?)
            }
            "CHAR_VALUES" => AttributeField::Text(self.character_text(value_node, "values")?),
            "UNICODE_VALUES" => AttributeField::Unicode(self.unicode_text(value_node, "values")?),
            "POINT_VALUES" => {
                AttributeField::Points(self.optional_vector_array(value_node, "values")?)
            }
            "VECTOR_VALUES" => {
                AttributeField::Vectors(self.optional_vector_array(value_node, "values")?)
            }
            "DIRECTION_VALUES" => {
                AttributeField::Directions(self.optional_vector_array(value_node, "values")?)
            }
            "AXIS_VALUES" => {
                AttributeField::Axes(self.optional_vector_array(value_node, "values")?)
            }
            "TAG_VALUES" => AttributeField::Tags(self.tag_array(value_node, "values")?),
            _ => AttributeField::Unsupported {
                type_name: value_node.definition.name.clone(),
                node_index: target,
            },
        })
    }

    fn attribute_owner(&self, node: &RawNode) -> AttributeOwner {
        let index = &node.index;
        if let Some(id) = self.ids.bodies.get(index) {
            AttributeOwner::Body(*id)
        } else if let Some(id) = self.ids.faces.get(index) {
            AttributeOwner::Face(*id)
        } else if let Some(id) = self.ids.edges.get(index) {
            AttributeOwner::Edge(*id)
        } else if let Some(id) = self.ids.vertices.get(index) {
            AttributeOwner::Vertex(*id)
        } else if let Some(id) = self.ids.regions.get(index) {
            AttributeOwner::Region(*id)
        } else if let Some(id) = self.ids.shells.get(index) {
            AttributeOwner::Shell(*id)
        } else if let Some(id) = self.ids.loops.get(index) {
            AttributeOwner::Loop(*id)
        } else if let Some(id) = self.ids.assemblies.get(index) {
            AttributeOwner::Assembly(*id)
        } else if let Some(id) = self.ids.instances.get(index) {
            AttributeOwner::Instance(*id)
        } else if let Some(id) = self.ids.points.get(index) {
            AttributeOwner::Point(*id)
        } else if let Some(id) = self.ids.curves.get(index) {
            AttributeOwner::Curve(*id)
        } else if let Some(id) = self.ids.surfaces.get(index) {
            AttributeOwner::Surface(*id)
        } else {
            AttributeOwner::Other(self.source(node))
        }
    }

    fn map_loop(&self, index: u32) -> Result<Loop, ParseError> {
        let node = self.node(index)?;
        Ok(Loop {
            id: self.id(&self.ids.loops, node, "LOOP")?,
            face: self.required_id(node, "face", "FACE", &self.ids.faces)?,
            half_edges: self.required_ring(
                node,
                "halfedge",
                "HALFEDGE",
                "forward",
                &self.ids.half_edges,
            )?,
            source: self.source(node),
        })
    }

    fn map_half_edge(&self, index: u32) -> Result<HalfEdge, ParseError> {
        let node = self.node(index)?;
        let loop_id = self.optional_id(node, "loop", "LOOP", &self.ids.loops)?;
        Ok(HalfEdge {
            id: self.id(&self.ids.half_edges, node, "HALFEDGE")?,
            loop_id,
            forward: self.optional_id(node, "forward", "HALFEDGE", &self.ids.half_edges)?,
            backward: self.optional_id(node, "backward", "HALFEDGE", &self.ids.half_edges)?,
            vertex: self.optional_id(node, "vertex", "VERTEX", &self.ids.vertices)?,
            other: self.optional_id(node, "other", "HALFEDGE", &self.ids.half_edges)?,
            edge: self.optional_id(node, "edge", "EDGE", &self.ids.edges)?,
            curve: self.optional_id(node, "curve", "curve geometry", &self.ids.curves)?,
            sense: self.sense(node, "sense")?,
            dummy: loop_id.is_none(),
            source: self.source(node),
        })
    }

    fn map_edge(&self, index: u32) -> Result<Edge, ParseError> {
        let node = self.node(index)?;
        let half_edges =
            self.required_ring(node, "halfedge", "HALFEDGE", "other", &self.ids.half_edges)?;
        let first_index = self.required_pointer(node, "halfedge")?;
        let first = self.node(first_index)?;
        let other_index = self.required_pointer(first, "other")?;
        let other = self.node(other_index)?;
        let end_vertex = self.optional_id(first, "vertex", "VERTEX", &self.ids.vertices)?;
        let start_vertex = self.optional_id(other, "vertex", "VERTEX", &self.ids.vertices)?;
        if start_vertex.is_some() != end_vertex.is_some() {
            return Err(self.invalid_topology(
                node,
                "edge_endpoints",
                "edge must have either zero or two endpoint vertices",
            ));
        }
        Ok(Edge {
            id: self.id(&self.ids.edges, node, "EDGE")?,
            owner: self.required_source(node, "owner")?,
            half_edges,
            start_vertex,
            end_vertex,
            curve: self.optional_id(node, "curve", "curve geometry", &self.ids.curves)?,
            tolerance: self.optional_double(node, "tolerance")?,
            source: self.source(node),
        })
    }

    fn map_vertex(&self, index: u32) -> Result<Vertex, ParseError> {
        let node = self.node(index)?;
        Ok(Vertex {
            id: self.id(&self.ids.vertices, node, "VERTEX")?,
            point: self.required_id(node, "point", "POINT", &self.ids.points)?,
            tolerance: self.optional_double(node, "tolerance")?,
            owner: self.required_source(node, "owner")?,
            source: self.source(node),
        })
    }

    fn map_point(&self, index: u32) -> Result<PointGeometry, ParseError> {
        let node = self.node(index)?;
        Ok(PointGeometry {
            id: self.id(&self.ids.points, node, "POINT")?,
            position: self.vector(node, "pvec")?,
            owner: self.optional_source(node, "owner")?,
            source: self.source(node),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn map_curve(&mut self, index: u32) -> Result<CurveGeometry, ParseError> {
        let node = self.node(index)?;
        let kind = match self.roles.type_name(node) {
            "LINE" => CurveKind::Line {
                point: self.vector(node, "pvec")?,
                direction: self.vector(node, "direction")?,
            },
            "CIRCLE" => CurveKind::Circle {
                center: self.vector(node, "centre")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                radius: self.positive_double(node, "radius")?,
            },
            "ELLIPSE" => CurveKind::Ellipse {
                center: self.vector(node, "centre")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                major_radius: self.positive_double(node, "major_radius")?,
                minor_radius: self.positive_double(node, "minor_radius")?,
            },
            "PARABOLA" => CurveKind::Parabola {
                origin: self.vector(node, "origin")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                focal_length: self.positive_double(node, "focal_length")?,
            },
            "HYPERBOLA" => CurveKind::Hyperbola {
                origin: self.vector(node, "origin")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
                transverse_radius: self.positive_double(node, "transverse_radius")?,
                conjugate_radius: self.positive_double(node, "conjugate_radius")?,
            },
            "TRIMMED_CURVE" => self.trimmed_curve(node)?,
            "B_CURVE" => CurveKind::Nurbs(self.nurbs_curve(node)?),
            "POLYLINE" => CurveKind::Polyline(match self.roles {
                RoleAccess::Named => super::polyline_data::decode_named_polyline_indexed(
                    node.index,
                    &self.nodes,
                    &self.duplicate_nodes,
                )?,
                RoleAccess::TopsolidCatalog => {
                    super::polyline_data::decode_builtin_topsolid_polyline_indexed(
                        self.schema_key,
                        node.index,
                        &self.nodes,
                        &self.duplicate_nodes,
                    )?
                }
                _ => {
                    return Err(self.invalid_field(
                        node,
                        "schema_layout",
                        "POLYLINE is not reviewed for this built-in profile",
                    ));
                }
            }),
            "SP_CURVE" => self.surface_parametric_curve(node)?,
            "INTERSECTION" => self.intersection_curve(node)?,
            type_name => {
                let source = self.source(node);
                push_diagnostic(
                    &mut self.diagnostics.borrow_mut(),
                    self.max_diagnostics,
                    node.byte_range.start,
                    BrepDiagnostic {
                        code: "geometry.unsupported_curve",
                        message: format!(
                            "curve type {type_name} is retained without semantic decoding"
                        ),
                        role: "curve",
                        source,
                    },
                )?;
                CurveKind::Unsupported {
                    type_name: type_name.to_owned(),
                }
            }
        };
        Ok(CurveGeometry {
            id: self.id(&self.ids.curves, node, "curve geometry")?,
            sense: self.sense(node, "sense")?,
            owner: self.optional_source(node, "owner")?,
            kind,
            source: self.source(node),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn map_surface(&mut self, index: u32) -> Result<SurfaceGeometry, ParseError> {
        let node = self.node(index)?;
        let kind = match self.roles.type_name(node) {
            "PLANE" => SurfaceKind::Plane {
                point: self.vector(node, "pvec")?,
                normal: self.vector(node, "normal")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "CYLINDER" => SurfaceKind::Cylinder {
                point: self.vector(node, "pvec")?,
                axis: self.vector(node, "axis")?,
                radius: self.positive_double(node, "radius")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "CONE" => {
                let point = self.vector(node, "pvec")?;
                let axis = self.vector(node, "axis")?;
                let radius = self.nonnegative_double(node, "radius")?;
                let sin_half_angle = self.double(node, "sin_half_angle")?;
                let cos_half_angle = self.double(node, "cos_half_angle")?;
                let x_axis = self.vector(node, "x_axis")?;
                if self.source_format == BrepSourceFormat::Text
                    && ((self.schema_key == "SCH_3600188_36001_13006"
                        && matches!(self.roles, RoleAccess::Sch3600188))
                        || (self.schema_key == "SCH_3700192_36001_13006"
                            && matches!(self.roles, RoleAccess::TopsolidCatalog)))
                {
                    SurfaceKind::Cone {
                        point,
                        axis,
                        radius,
                        sin_half_angle,
                        cos_half_angle,
                        x_axis,
                    }
                } else {
                    SurfaceKind::XtCone {
                        point,
                        axis,
                        radius,
                        sin_half_angle,
                        cos_half_angle,
                        x_axis,
                    }
                }
            }
            "SPHERE" => SurfaceKind::Sphere {
                center: self.vector(node, "centre")?,
                radius: self.positive_double(node, "radius")?,
                axis: self.vector(node, "axis")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "TORUS" => SurfaceKind::Torus {
                center: self.vector(node, "centre")?,
                axis: self.vector(node, "axis")?,
                major_radius: self.double(node, "major_radius")?,
                minor_radius: self.positive_double(node, "minor_radius")?,
                x_axis: self.vector(node, "x_axis")?,
            },
            "BLENDED_EDGE" => self.blended_edge_surface(node)?,
            "BLEND_BOUND" => {
                let boundary_index = self.nonnegative_short(node, "boundary")?;
                if boundary_index > 1 {
                    return Err(self.invalid_geometry(
                        node,
                        "boundary",
                        "blend boundary index must be zero or one",
                    ));
                }
                SurfaceKind::BlendBoundary {
                    boundary_index: u8::try_from(boundary_index).unwrap_or(u8::MAX),
                    blend_surface: self.required_id(
                        node,
                        "blend",
                        "surface geometry",
                        &self.ids.surfaces,
                    )?,
                }
            }
            "SPUN_SURF" => SurfaceKind::Spun {
                profile: self.required_id(node, "profile", "curve geometry", &self.ids.curves)?,
                base: self.vector(node, "base")?,
                axis: self.vector(node, "axis")?,
                start: self.optional_vector(node, "start")?,
                end: self.optional_vector(node, "end")?,
                start_parameter: self.optional_double(node, "start_param")?,
                end_parameter: self.optional_double(node, "end_param")?,
                x_axis: self.optional_vector(node, "x_axis")?,
            },
            // The scale field is for internal use only and may be null (XT reference).
            "SWEPT_SURF" => SurfaceKind::Swept {
                section: self.required_id(node, "section", "curve geometry", &self.ids.curves)?,
                direction: self.vector(node, "sweep")?,
            },
            "OFFSET_SURF" => SurfaceKind::Offset {
                basis_surface: self.required_id(
                    node,
                    "surface",
                    "surface geometry",
                    &self.ids.surfaces,
                )?,
                offset: self.double(node, "offset")?,
            },
            "MESH" => self.mesh_surface(node)?,
            "B_SURFACE" => SurfaceKind::Nurbs(self.nurbs_surface(node)?),
            type_name => {
                let source = self.source(node);
                push_diagnostic(
                    &mut self.diagnostics.borrow_mut(),
                    self.max_diagnostics,
                    node.byte_range.start,
                    BrepDiagnostic {
                        code: "geometry.unsupported_surface",
                        message: format!(
                            "surface type {type_name} is retained without semantic decoding"
                        ),
                        role: "surface",
                        source,
                    },
                )?;
                SurfaceKind::Unsupported {
                    type_name: type_name.to_owned(),
                }
            }
        };
        Ok(SurfaceGeometry {
            id: self.id(&self.ids.surfaces, node, "surface geometry")?,
            sense: self.sense(node, "sense")?,
            owner: self.optional_source(node, "owner")?,
            kind,
            source: self.source(node),
        })
    }

    fn mesh_surface(&mut self, node: &RawNode) -> Result<SurfaceKind, ParseError> {
        if self.mesh_context.is_none() {
            let context = super::mesh_data::MeshDecodeContext::new(
                self.schema_key,
                self.raw_nodes,
                node.index,
            )
            .map_err(|error| {
                self.invalid_geometry(
                    node,
                    "psm_imesh",
                    format!("exact-profile MESH decode failed: {error:?}"),
                )
            })?;
            self.mesh_context = Some(context);
        }
        let context = self.mesh_context.as_ref().ok_or_else(|| {
            self.invalid_geometry(node, "psm_imesh", "mesh decode context was not initialized")
        })?;
        let embedded = context.decode_embedded_mesh(node.index).map_err(|error| {
            self.invalid_geometry(
                node,
                "psm_imesh",
                format!("exact-profile MESH decode failed: {error:?}"),
            )
        })?;
        let transform = context
            .decode_mesh_linked_transform(node.index)
            .map_err(|error| {
                self.invalid_geometry(
                    node,
                    "transform",
                    format!("exact-profile TRANSFORM decode failed: {error:?}"),
                )
            })?;
        let transform_index = match self.pointer(node, "transform")? {
            0 => None,
            index => Some(index),
        };
        if embedded.is_none() {
            let source = self.source(node);
            push_diagnostic(
                &mut self.diagnostics.borrow_mut(),
                self.max_diagnostics,
                node.byte_range.start,
                BrepDiagnostic {
                    code: "geometry.mesh_data_not_embedded",
                    message: "MESH has no embedded PSM_MESH; associated-file data, if any, was not loaded"
                        .to_owned(),
                    role: "surface",
                    source,
                },
            )?;
        }
        let unembedded_source_fields = embedded.is_none().then(|| node.fields.clone());
        Ok(SurfaceKind::Mesh {
            source_mesh_index: node.index,
            embedded,
            transform_index,
            transform,
            unembedded_source_fields,
        })
    }

    #[allow(clippy::float_cmp)] // A zero range is stored as exact zeros.
    fn blended_edge_surface(&self, node: &RawNode) -> Result<SurfaceKind, ParseError> {
        let blend_type = match self.character(node, "blend_type")? {
            b'R' => BlendType::RollingBall,
            b'E' => BlendType::CliffEdge,
            value => {
                return Err(self.invalid_geometry(
                    node,
                    "blend_type",
                    format!("blend type must be 'R' or 'E', observed 0x{value:02x}"),
                ));
            }
        };
        let supporting_surfaces =
            self.required_id_pair(node, "surface", "surface geometry", &self.ids.surfaces)?;
        let boundary_surfaces =
            self.optional_id_pair(node, "boundary", "surface geometry", &self.ids.surfaces)?;
        let ranges = self.double_pair(node, "range")?;
        // A zero-range blend, the construction surface a cliff-edge blend
        // rolls through and never a face's surface, stores null thumb weights
        // (an iCAD SX V8L4 production body, 2026). The unweighted rolling ball
        // has weight 1 on both sides; a blend with a range keeps its weights.
        let weights = self.optional_double_array(node, "thumb_weight")?;
        self.validate_count(node, "thumb_weight", 2, weights.len())?;
        let thumb_weights = match (weights[0], weights[1]) {
            (Some(first), Some(second)) => [first, second],
            _ if ranges == [0.0, 0.0] => [1.0, 1.0],
            _ => {
                return Err(self.invalid_geometry(
                    node,
                    "thumb_weight",
                    "thumb weights are null on a blend with a nonzero range",
                ));
            }
        };
        Ok(SurfaceKind::BlendedEdge {
            blend_type,
            supporting_surfaces,
            spine_curve: self.required_id(node, "spine", "curve geometry", &self.ids.curves)?,
            ranges,
            thumb_weights,
            boundary_surfaces,
            start: self.optional_source(node, "start")?,
            end: self.optional_source(node, "end")?,
        })
    }

    #[allow(clippy::float_cmp)] // Equal parameters are forbidden, however short a valid trim is.
    fn trimmed_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let basis_curve =
            self.required_id(node, "basis_curve", "curve geometry", &self.ids.curves)?;
        let basis_index = self.required_pointer(node, "basis_curve")?;
        if basis_index == node.index {
            return Err(self.invalid_geometry(
                node,
                "basis_curve",
                "trimmed curve references itself",
            ));
        }
        if self.sense(node, "sense")? != Sense::Positive {
            return Err(self.invalid_geometry(
                node,
                "sense",
                "trimmed curve sense must be positive",
            ));
        }
        let start_point = self.vector(node, "point_1")?;
        let end_point = self.vector(node, "point_2")?;
        let basis = self.node(basis_index)?;
        let stored = [
            self.optional_double(node, "parm_1")?,
            self.optional_double(node, "parm_2")?,
        ];
        let [mut start_parameter, mut end_parameter] =
            self.resolve_trim_parameters(node, basis, &start_point, &end_point, stored)?;
        let basis_sense = self.sense(basis, "sense")?;
        if self.roles.type_name(basis) == "POLYLINE" && basis_sense == Sense::Unknown {
            return Err(self.invalid_geometry(
                basis,
                "sense",
                "trimmed polyline requires a known curve sense",
            ));
        }
        if start_parameter == end_parameter
            || (basis_sense == Sense::Positive && end_parameter < start_parameter)
            || (basis_sense == Sense::Negative && end_parameter > start_parameter)
        {
            return Err(self.invalid_geometry(
                node,
                "parm_2",
                "trim parameters must be distinct and follow the basis curve sense",
            ));
        }
        if self.roles.type_name(basis) == "B_CURVE" {
            let nurbs = self.nurbs_curve(basis)?;
            if !nurbs.periodic {
                let (range_start, range_end) = nurbs_parameter_range(
                    &nurbs.knots,
                    &nurbs.knot_multiplicities,
                    nurbs.control_vertex_count,
                    nurbs.degree,
                )
                .ok_or_else(|| {
                    self.invalid_geometry(
                        basis,
                        "knots",
                        "NURBS active parameter range is unavailable",
                    )
                })?;
                if start_parameter < range_start - TRIM_PARAMETER_TOLERANCE
                    || start_parameter > range_end + TRIM_PARAMETER_TOLERANCE
                    || end_parameter < range_start - TRIM_PARAMETER_TOLERANCE
                    || end_parameter > range_end + TRIM_PARAMETER_TOLERANCE
                {
                    let field = if start_parameter < range_start - TRIM_PARAMETER_TOLERANCE
                        || start_parameter > range_end + TRIM_PARAMETER_TOLERANCE
                    {
                        "parm_1"
                    } else {
                        "parm_2"
                    };
                    return Err(self.invalid_geometry(
                        node,
                        field,
                        "trim parameters must lie in the non-periodic basis curve parameter range",
                    ));
                }
                start_parameter = start_parameter.clamp(range_start, range_end);
                end_parameter = end_parameter.clamp(range_start, range_end);
            }
        }
        Ok(CurveKind::Trimmed {
            basis_curve,
            start_point,
            end_point,
            start_parameter,
            end_parameter,
        })
    }

    fn resolve_trim_parameters(
        &self,
        node: &RawNode,
        basis: &RawNode,
        start_point: &Vector3,
        end_point: &Vector3,
        stored: [Option<f64>; 2],
    ) -> Result<[f64; 2], ParseError> {
        Ok(match stored {
            [Some(start), Some(end)] => [start, end],
            // Both parameters can be unset on a line. Its published
            // parameterisation R(t) = P + tD, with D a unit vector, gives them
            // from the stored end points, which must lie on that line.
            [None, None] if self.roles.type_name(basis) == "LINE" => {
                let origin = self.vector(basis, "pvec")?.to_array();
                let direction = self.vector(basis, "direction")?.to_array();
                let unit = direction.iter().map(|d| d * d).sum::<f64>().sqrt();
                if (unit - 1.0).abs() > LINE_TRIM_TOLERANCE {
                    return Err(self.invalid_geometry(
                        basis,
                        "direction",
                        "basis line direction is not a unit vector",
                    ));
                }
                let mut parameters = [0.0; 2];
                for (parameter, (point, field)) in parameters
                    .iter_mut()
                    .zip([(start_point, "point_1"), (end_point, "point_2")])
                {
                    let point = point.to_array();
                    let along = (0..3)
                        .map(|i| (point[i] - origin[i]) * direction[i])
                        .sum::<f64>();
                    let off = (0..3)
                        .map(|i| (origin[i] + along * direction[i] - point[i]).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    if off.is_nan() || off > LINE_TRIM_TOLERANCE {
                        return Err(self.invalid_geometry(
                            node,
                            field,
                            "trim point is not on its basis line",
                        ));
                    }
                    *parameter = along;
                }
                parameters
            }
            _ => {
                let field = if stored[0].is_none() {
                    "parm_1"
                } else {
                    "parm_2"
                };
                return Err(self.invalid_geometry(
                    node,
                    field,
                    "required double is null or non-finite",
                ));
            }
        })
    }

    fn intersection_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let chart = self.required_typed_node(node, "chart", "CHART")?;
        let chart_parameters = IntersectionChartParameters {
            base_parameter: self.optional_double(chart, "base_parameter")?,
            base_scale: self.optional_double(chart, "base_scale")?,
            chordal_error: self.optional_double(chart, "chordal_error")?,
            angular_error: self.optional_double(chart, "angular_error")?,
        };
        let count = self.nonnegative_integer(chart, "chart_count")?;
        let chart_points =
            self.intersection_points(chart, usize::try_from(count).unwrap_or(usize::MAX))?;
        let (start_points, start_limit_kind, start_term_use) =
            self.intersection_limit(node, "start")?;
        let (end_points, end_limit_kind, end_term_use) = self.intersection_limit(node, "end")?;
        let surface_indices = self.pointer_array(node, "surface")?;
        if surface_indices.len() != 2 {
            return Err(self.invalid_field(
                node,
                "surface",
                "intersection curve requires exactly two surfaces",
            ));
        }
        let surfaces = [
            self.id_from_target(
                node,
                "surface",
                surface_indices[0],
                "surface geometry",
                &self.ids.surfaces,
            )?,
            self.id_from_target(
                node,
                "surface",
                surface_indices[1],
                "surface geometry",
                &self.ids.surfaces,
            )?,
        ];
        let intersection_data = self.optional_source_if_field(node, "intersection_data")?;
        if intersection_data.is_some() {
            self.required_typed_node(node, "intersection_data", "INTERSECTION_DATA")?;
        }
        Ok(CurveKind::Intersection {
            surfaces,
            chart: self.required_source(node, "chart")?,
            chart_parameters,
            chart_points,
            start: self.required_source(node, "start")?,
            start_limit_kind: Some(start_limit_kind),
            start_term_use,
            start_points,
            end: self.required_source(node, "end")?,
            end_limit_kind: Some(end_limit_kind),
            end_term_use,
            end_points,
            intersection_uv: self.map_intersection_uv_data_for(node)?,
            intersection_data,
        })
    }

    fn intersection_limit(
        &self,
        intersection: &RawNode,
        field: &'static str,
    ) -> Result<(Vec<Vector3>, IntersectionLimitKind, Option<u8>), ParseError> {
        let limit = self.required_typed_node(intersection, field, "LIMIT")?;
        let (count, kind) = match self.character(limit, "type")? {
            b'H' => (1, IntersectionLimitKind::Help),
            b'L' => (1, IntersectionLimitKind::Limit),
            b'T' => (2, IntersectionLimitKind::Terminator),
            // A spine boundary ('B') describes a degenerate blend boundary with one point.
            b'B' => (1, IntersectionLimitKind::Boundary),
            _ => {
                return Err(self.invalid_geometry(
                    limit,
                    "type",
                    "invalid intersection limit kind",
                ));
            }
        };
        let term_use = self.optional_character(limit, "term_use")?;
        Ok((self.intersection_points(limit, count)?, kind, term_use))
    }

    fn map_intersection_uv_data_for(
        &self,
        intersection: &RawNode,
    ) -> Result<Option<IntersectionData>, ParseError> {
        if self
            .roles
            .field(intersection, "intersection_data")
            .is_none()
            || self.pointer(intersection, "intersection_data")? == 0
        {
            return Ok(None);
        }
        let data =
            self.required_typed_node(intersection, "intersection_data", "INTERSECTION_DATA")?;
        let chart = self.required_typed_node(intersection, "chart", "CHART")?;
        let chart_count = self.nonnegative_integer(chart, "chart_count")?;
        let chart_points =
            self.intersection_points(chart, usize::try_from(chart_count).unwrap_or(usize::MAX))?;
        let start_terminator = matches!(
            self.intersection_limit(intersection, "start")?.1,
            IntersectionLimitKind::Terminator
        );
        let end_terminator = matches!(
            self.intersection_limit(intersection, "end")?.1,
            IntersectionLimitKind::Terminator
        );
        self.map_intersection_data(data, chart_points.len(), start_terminator, end_terminator)
            .map(Some)
    }

    fn map_intersection_data(
        &self,
        node: &RawNode,
        chart_count: usize,
        start_terminator: bool,
        end_terminator: bool,
    ) -> Result<IntersectionData, ParseError> {
        let uv_type = self.intersection_uv_type(node)?;
        let raw_values = self
            .field(node, "values")?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Double(value) if value.is_none_or(f64::is_finite) => Ok(*value),
                _ => Err(self.invalid_geometry(
                    node,
                    "values",
                    "intersection UV array contains a non-double or non-finite value",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let source = self.source(node);
        let (parameters_per_hvec, has_first, has_second) = match uv_type {
            IntersectionUvType::None => (0, false, false),
            IntersectionUvType::First => (2, true, false),
            IntersectionUvType::Second => (2, false, true),
            IntersectionUvType::Both => (4, true, true),
            IntersectionUvType::Unsupported(_) => {
                return Ok(IntersectionData {
                    source,
                    uv_type,
                    raw_values,
                    start_is_terminator: start_terminator,
                    start_terminator: None,
                    end_is_terminator: end_terminator,
                    chart: None,
                    end_terminator: None,
                });
            }
        };
        let term_count = usize::from(start_terminator) + usize::from(end_terminator);
        let hvec_count = chart_count.checked_add(term_count).ok_or_else(|| {
            self.invalid_geometry(node, "values", "intersection hvec count overflow")
        })?;
        let expected = hvec_count.checked_mul(parameters_per_hvec).ok_or_else(|| {
            self.invalid_geometry(node, "values", "intersection UV value count overflow")
        })?;
        self.validate_count(node, "values", expected, raw_values.len())?;
        let mut values = raw_values.chunks_exact(parameters_per_hvec.max(1));
        let mut take_coordinates = || -> Result<IntersectionUvCoordinates, ParseError> {
            if parameters_per_hvec == 0 {
                Ok(IntersectionUvCoordinates {
                    first_surface: None,
                    second_surface: None,
                })
            } else {
                let chunk = values.next().ok_or_else(|| {
                    self.invalid_geometry(
                        node,
                        "values",
                        "validated intersection UV count is incomplete",
                    )
                })?;
                Ok(IntersectionUvCoordinates {
                    first_surface: has_first.then(|| [chunk[0], chunk[1]]),
                    second_surface: has_second.then(|| {
                        let offset = if has_first { 2 } else { 0 };
                        [chunk[offset], chunk[offset + 1]]
                    }),
                })
            }
        };
        let start_uv = if start_terminator {
            Some(take_coordinates()?)
        } else {
            None
        };
        let chart_uv = (0..chart_count)
            .map(|_| take_coordinates())
            .collect::<Result<Vec<_>, _>>()?;
        let end_uv = if end_terminator {
            Some(take_coordinates()?)
        } else {
            None
        };
        Ok(IntersectionData {
            source,
            uv_type,
            raw_values,
            start_is_terminator: start_terminator,
            start_terminator: start_uv,
            end_is_terminator: end_terminator,
            chart: Some(chart_uv),
            end_terminator: end_uv,
        })
    }

    fn intersection_uv_type(&self, node: &RawNode) -> Result<IntersectionUvType, ParseError> {
        let uv_field = self
            .roles
            .field(node, "uv_type")
            .or_else(|| self.roles.field(node, "uv_kind"))
            .ok_or_else(|| self.invalid_field(node, "uv_type", "missing UV type field"))?;
        let uv_field_name = if uv_field.definition.name == "uv_kind" {
            "uv_kind"
        } else {
            "uv_type"
        };
        let uv_type = match self.value(node, uv_field_name)? {
            FieldValue::UnsignedByte(1) => IntersectionUvType::None,
            FieldValue::UnsignedByte(2) => IntersectionUvType::First,
            FieldValue::UnsignedByte(3) => IntersectionUvType::Second,
            FieldValue::UnsignedByte(4) => IntersectionUvType::Both,
            FieldValue::UnsignedByte(value) => IntersectionUvType::Unsupported(*value),
            _ => {
                return Err(self.invalid_field(
                    node,
                    uv_field_name,
                    "UV type is not an unsigned byte",
                ));
            }
        };
        Ok(uv_type)
    }

    fn intersection_points(
        &self,
        node: &RawNode,
        count: usize,
    ) -> Result<Vec<Vector3>, ParseError> {
        let values = &self.field(node, "hvec")?.values;
        if count == 0 || values.len() != count {
            return Err(self.invalid_geometry(node, "hvec", "intersection point count mismatch"));
        }
        values
            .iter()
            .map(|value| match value {
                FieldValue::IntersectionPoint([Some(x), Some(y), Some(z)])
                    if x.is_finite() && y.is_finite() && z.is_finite() =>
                {
                    Ok(Vector3 {
                        x: *x,
                        y: *y,
                        z: *z,
                    })
                }
                _ => Err(self.invalid_geometry(
                    node,
                    "hvec",
                    "intersection point is null or non-finite",
                )),
            })
            .collect()
    }

    fn surface_parametric_curve(&self, node: &RawNode) -> Result<CurveKind, ParseError> {
        let parameter_curve = self.required_typed_node(node, "b_curve", "B_CURVE")?;
        let nurbs = self.required_typed_node(parameter_curve, "nurbs", "NURBS_CURVE")?;
        let rational = self.logical(nurbs, "rational")?;
        let dimension = self.positive_short(nurbs, "vertex_dim")?;
        if dimension != if rational { 3 } else { 2 } {
            return Err(self.invalid_geometry(
                nurbs,
                "vertex_dim",
                "surface parameter curve must have two coordinates plus an optional weight",
            ));
        }
        let tolerance_to_original = self.optional_double(node, "tolerance_to_original")?;
        if tolerance_to_original.is_some_and(|value| value < 0.0) {
            return Err(self.invalid_geometry(node, "tolerance_to_original", "negative tolerance"));
        }
        Ok(CurveKind::SurfaceParametric {
            surface: self.required_id(node, "surface", "surface geometry", &self.ids.surfaces)?,
            parameter_curve: self.required_id(
                node,
                "b_curve",
                "curve geometry",
                &self.ids.curves,
            )?,
            original_curve: self.optional_id(
                node,
                "original",
                "curve geometry",
                &self.ids.curves,
            )?,
            tolerance_to_original,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn nurbs_curve(&self, owner: &RawNode) -> Result<NurbsCurve, ParseError> {
        let nurbs = self.required_typed_node(owner, "nurbs", "NURBS_CURVE")?;
        let control_count = self.nonnegative_integer(nurbs, "n_vertices")?;
        let vertex_dimension = self.positive_short(nurbs, "vertex_dim")?;
        let distinct_knot_count = self.nonnegative_integer(nurbs, "n_knots")?;
        let vertices_node =
            self.required_typed_node(nurbs, "bspline_vertices", "BSPLINE_VERTICES")?;
        let multiplicities_node = self.required_typed_node(nurbs, "knot_mult", "KNOT_MULT")?;
        let knots_node = self.required_typed_node(nurbs, "knots", "KNOT_SET")?;
        let mut flat_vertices = self.double_array(vertices_node, "vertices")?;
        let distinct_knot_count_usize = usize::try_from(distinct_knot_count).unwrap_or(usize::MAX);
        let multiplicities = self.positive_short_array_prefix(
            multiplicities_node,
            "mult",
            distinct_knot_count_usize,
        )?;
        let knots = self.double_array_prefix(knots_node, "knots", distinct_knot_count_usize)?;
        let expected_vertices = usize::try_from(control_count)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::from(vertex_dimension));
        if flat_vertices.len() < expected_vertices {
            self.validate_count(nurbs, "vertices", expected_vertices, flat_vertices.len())?;
        }
        // Values after the declared control points are ignored, as for the
        // knots: SolidWorks 2019 sizes the arrays of a periodic curve alike.
        flat_vertices.truncate(expected_vertices);
        let degree = self.positive_short(nurbs, "degree")?;
        let rational = self.logical(nurbs, "rational")?;
        let periodic = self.logical(nurbs, "periodic")?;
        let closed = self.logical(nurbs, "closed")?;
        if control_count <= u32::from(degree)
            || !(if rational { 3..=4 } else { 2..=3 }).contains(&vertex_dimension)
            || (periodic && !closed)
        {
            return Err(self.invalid_geometry(
                nurbs,
                "n_vertices",
                "invalid NURBS dimensions or closure",
            ));
        }
        if rational
            && flat_vertices
                .chunks(usize::from(vertex_dimension))
                .any(|vertex| vertex.last().is_none_or(|weight| *weight <= 0.0))
        {
            return Err(self.invalid_geometry(nurbs, "vertices", "NURBS weights must be positive"));
        }
        self.validate_nurbs_knots(
            nurbs,
            "knots",
            &knots,
            &multiplicities,
            control_count,
            degree,
        )?;
        Ok(NurbsCurve {
            degree,
            control_vertex_count: control_count,
            vertex_dimension,
            knot_type: self.byte(nurbs, "knot_type")?,
            periodic,
            closed,
            rational,
            curve_form: self.byte(nurbs, "curve_form")?,
            control_vertices: flat_vertices
                .chunks(usize::from(vertex_dimension))
                .map(<[f64]>::to_vec)
                .collect(),
            knots,
            knot_multiplicities: multiplicities,
            sources: vec![
                self.source(nurbs),
                self.source(vertices_node),
                self.source(multiplicities_node),
                self.source(knots_node),
            ],
        })
    }

    #[allow(clippy::too_many_lines)]
    fn nurbs_surface(&self, owner: &RawNode) -> Result<NurbsSurface, ParseError> {
        let nurbs = self.required_typed_node(owner, "nurbs", "NURBS_SURF")?;
        let u_count = self.nonnegative_integer(nurbs, "n_u_vertices")?;
        let v_count = self.nonnegative_integer(nurbs, "n_v_vertices")?;
        let vertex_dimension = self.positive_short(nurbs, "vertex_dim")?;
        let vertices_node =
            self.required_typed_node(nurbs, "bspline_vertices", "BSPLINE_VERTICES")?;
        let u_mult_node = self.required_typed_node(nurbs, "u_knot_mult", "KNOT_MULT")?;
        let v_mult_node = self.required_typed_node(nurbs, "v_knot_mult", "KNOT_MULT")?;
        let u_knots_node = self.required_typed_node(nurbs, "u_knots", "KNOT_SET")?;
        let v_knots_node = self.required_typed_node(nurbs, "v_knots", "KNOT_SET")?;
        let mut flat_vertices = self.double_array(vertices_node, "vertices")?;
        let u_knot_count =
            usize::try_from(self.nonnegative_integer(nurbs, "n_u_knots")?).unwrap_or(usize::MAX);
        let v_knot_count =
            usize::try_from(self.nonnegative_integer(nurbs, "n_v_knots")?).unwrap_or(usize::MAX);
        let u_multiplicities =
            self.positive_short_array_prefix(u_mult_node, "mult", u_knot_count)?;
        let v_multiplicities =
            self.positive_short_array_prefix(v_mult_node, "mult", v_knot_count)?;
        let u_knots = self.double_array_prefix(u_knots_node, "knots", u_knot_count)?;
        let v_knots = self.double_array_prefix(v_knots_node, "knots", v_knot_count)?;
        let vertex_count = usize::try_from(u_count)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::try_from(v_count).unwrap_or(usize::MAX));
        let expected_vertices = vertex_count.saturating_mul(usize::from(vertex_dimension));
        if flat_vertices.len() < expected_vertices {
            self.validate_count(nurbs, "vertices", expected_vertices, flat_vertices.len())?;
        }
        flat_vertices.truncate(expected_vertices);
        let u_degree = self.positive_short(nurbs, "u_degree")?;
        let v_degree = self.positive_short(nurbs, "v_degree")?;
        self.validate_nurbs_knots(
            nurbs,
            "u_knots",
            &u_knots,
            &u_multiplicities,
            u_count,
            u_degree,
        )?;
        self.validate_nurbs_knots(
            nurbs,
            "v_knots",
            &v_knots,
            &v_multiplicities,
            v_count,
            v_degree,
        )?;
        let rational = self.logical(nurbs, "rational")?;
        let u_periodic = self.logical(nurbs, "u_periodic")?;
        let v_periodic = self.logical(nurbs, "v_periodic")?;
        let u_closed = self.logical(nurbs, "u_closed")?;
        let v_closed = self.logical(nurbs, "v_closed")?;
        if u_count <= u32::from(u_degree)
            || v_count <= u32::from(v_degree)
            || vertex_dimension != if rational { 4 } else { 3 }
            || (u_periodic && !u_closed)
            || (v_periodic && !v_closed)
        {
            return Err(self.invalid_geometry(
                nurbs,
                "vertices",
                "invalid NURBS surface dimensions or closure",
            ));
        }
        if rational
            && flat_vertices
                .chunks(usize::from(vertex_dimension))
                .any(|vertex| vertex.last().is_none_or(|weight| *weight <= 0.0))
        {
            return Err(self.invalid_geometry(nurbs, "vertices", "NURBS weights must be positive"));
        }
        Ok(NurbsSurface {
            u_degree,
            v_degree,
            u_control_vertex_count: u_count,
            v_control_vertex_count: v_count,
            vertex_dimension,
            u_knot_type: self.byte(nurbs, "u_knot_type")?,
            v_knot_type: self.byte(nurbs, "v_knot_type")?,
            u_periodic,
            v_periodic,
            u_closed,
            v_closed,
            rational,
            surface_form: self.byte(nurbs, "surface_form")?,
            control_vertices: flat_vertices
                .chunks(usize::from(vertex_dimension))
                .map(<[f64]>::to_vec)
                .collect(),
            u_knots,
            v_knots,
            u_knot_multiplicities: u_multiplicities,
            v_knot_multiplicities: v_multiplicities,
            sources: vec![
                self.source(nurbs),
                self.source(vertices_node),
                self.source(u_mult_node),
                self.source(v_mult_node),
                self.source(u_knots_node),
                self.source(v_knots_node),
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_nurbs_knots(
        &self,
        node: &RawNode,
        field: &'static str,
        knots: &[f64],
        multiplicities: &[u16],
        control_count: u32,
        degree: u16,
    ) -> Result<(), ParseError> {
        if knots.len() < 2
            || knots.windows(2).any(|pair| pair[0] >= pair[1])
            || multiplicities.iter().any(|&count| count > degree + 1)
            || multiplicities[1..multiplicities.len() - 1]
                .iter()
                .any(|&count| count > degree)
        {
            return Err(self.invalid_geometry(
                node,
                field,
                "invalid distinct knots or multiplicities",
            ));
        }
        let expected_expanded = u64::from(control_count) + u64::from(degree) + 1;
        let actual_expanded = multiplicities
            .iter()
            .map(|value| u64::from(*value))
            .sum::<u64>();
        if actual_expanded != expected_expanded {
            return Err(self.invalid_geometry(
                node,
                field,
                format!(
                    "expanded knot count {actual_expanded} does not equal vertices + degree + 1 ({expected_expanded})"
                ),
            ));
        }
        // Only [t_degree, t_control_count] is fully defined. Find these knots
        // without allocating the expanded vector, including imaginary knots.
        let knot_at = |index: u64| {
            let mut end = 0_u64;
            knots
                .iter()
                .zip(multiplicities)
                .find_map(|(&knot, &count)| {
                    end += u64::from(count);
                    (index < end).then_some(knot)
                })
        };
        if knot_at(u64::from(degree)) >= knot_at(u64::from(control_count)) {
            return Err(self.invalid_geometry(
                node,
                field,
                "NURBS active parameter range must have positive length",
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn validate_topology(
        &self,
        bodies: &[Body],
        regions: &[Region],
        shells: &[Shell],
        faces: &[Face],
        loops: &[Loop],
        half_edges: &[HalfEdge],
        edges: &[Edge],
        vertices: &[Vertex],
    ) -> Result<TopologyValidation, ParseError> {
        for region in regions {
            let Some(body) = bodies.iter().find(|body| body.id == region.body) else {
                return Err(self.invalid_topology_source(
                    &region.source,
                    "region_body",
                    "region owner does not exist",
                ));
            };
            if !body.regions.contains(&region.id) {
                return Err(self.invalid_topology_source(
                    &region.source,
                    "region_body_inverse",
                    "region owner does not contain the region in its region chain",
                ));
            }
        }
        for shell in shells {
            let Some(region) = regions.iter().find(|region| region.id == shell.region) else {
                return Err(self.invalid_topology_source(
                    &shell.source,
                    "shell_region",
                    "shell region does not exist",
                ));
            };
            if !region.shells.contains(&shell.id) {
                return Err(self.invalid_topology_source(
                    &shell.source,
                    "shell_region_inverse",
                    "shell region does not contain the shell in its shell chain",
                ));
            }
        }
        for face in faces {
            let Some(back_shell) = shells.iter().find(|shell| shell.id == face.back_shell) else {
                return Err(self.invalid_topology_source(
                    &face.source,
                    "face_shells",
                    "face references an unmapped back shell",
                ));
            };
            let Some(front_shell) = shells.iter().find(|shell| shell.id == face.front_shell) else {
                return Err(self.invalid_topology_source(
                    &face.source,
                    "face_shells",
                    "face references an unmapped front shell",
                ));
            };
            if !back_shell.back_faces.contains(&face.id)
                || !front_shell.front_faces.contains(&face.id)
            {
                return Err(self.invalid_topology_source(
                    &face.source,
                    "face_shell_inverse",
                    "face shell owners do not contain the face in the matching face chains",
                ));
            }
            for loop_id in &face.loops {
                let Some(loop_value) = loops.get(usize::try_from(*loop_id).unwrap_or(usize::MAX))
                else {
                    return Err(self.invalid_topology_source(
                        &face.source,
                        "face_loops",
                        "face references an unmapped loop",
                    ));
                };
                if loop_value.face != face.id {
                    return Err(self.invalid_topology_source(
                        &loop_value.source,
                        "loop_face_inverse",
                        "loop owner does not match the face loop chain",
                    ));
                }
            }
        }
        for loop_value in loops {
            let count = loop_value.half_edges.len();
            for (position, half_edge_id) in loop_value.half_edges.iter().enumerate() {
                let half_edge = self.entity(
                    half_edges,
                    *half_edge_id,
                    &loop_value.source,
                    "loop_half_edges",
                )?;
                if half_edge.loop_id != Some(loop_value.id) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "halfedge_loop_inverse",
                        "half-edge loop does not match its containing ring",
                    ));
                }
                let expected_forward = loop_value.half_edges[(position + 1) % count];
                let expected_backward = loop_value.half_edges[(position + count - 1) % count];
                if half_edge.forward != Some(expected_forward)
                    || half_edge.backward != Some(expected_backward)
                {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "loop_ring_inverse",
                        "half-edge forward/backward links are not inverse ring links",
                    ));
                }
            }
        }
        for edge in edges {
            if matches!(self.roles, RoleAccess::OnshapeSch30000)
                && !bodies.iter().any(|body| {
                    body.source.node_index == edge.owner.node_index && body.edges.contains(&edge.id)
                })
                && !shells.iter().any(|shell| {
                    shell.source.node_index == edge.owner.node_index
                        && shell.wire_edges.contains(&edge.id)
                })
            {
                return Err(self.invalid_topology_source(
                    &edge.source,
                    "edge_owner_inverse",
                    "edge owner does not contain the edge in its chain",
                ));
            }
            let count = edge.half_edges.len();
            for (position, half_edge_id) in edge.half_edges.iter().enumerate() {
                let half_edge =
                    self.entity(half_edges, *half_edge_id, &edge.source, "edge_half_edges")?;
                if half_edge.edge != Some(edge.id) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "halfedge_edge_inverse",
                        "half-edge owner does not match its containing edge ring",
                    ));
                }
                let expected_other = edge.half_edges[(position + 1) % count];
                if half_edge.other != Some(expected_other) {
                    return Err(self.invalid_topology_source(
                        &half_edge.source,
                        "edge_ring_inverse",
                        "half-edge other links do not form the mapped edge ring",
                    ));
                }
            }
        }
        for vertex in vertices {
            if matches!(self.roles, RoleAccess::OnshapeSch30000)
                && !bodies.iter().any(|body| {
                    body.source.node_index == vertex.owner.node_index
                        && body.vertices.contains(&vertex.id)
                })
                && !shells.iter().any(|shell| {
                    shell.source.node_index == vertex.owner.node_index
                        && shell.isolated_vertex == Some(vertex.id)
                })
            {
                return Err(self.invalid_topology_source(
                    &vertex.source,
                    "vertex_owner_inverse",
                    "vertex owner does not contain the vertex",
                ));
            }
            if usize::try_from(vertex.point).map_or(true, |id| id >= self.ids.points.len()) {
                return Err(self.invalid_topology_source(
                    &vertex.source,
                    "vertex_point",
                    "vertex point geometry does not exist",
                ));
            }
        }
        for body in bodies {
            let body_regions = body
                .regions
                .iter()
                .filter_map(|id| regions.get(usize::try_from(*id).ok()?))
                .collect::<Vec<_>>();
            let body_shell_ids = body_regions
                .iter()
                .flat_map(|region| region.shells.iter().copied())
                .collect::<BTreeSet<_>>();
            let body_face_ids = body_shell_ids
                .iter()
                .filter_map(|id| shells.get(usize::try_from(*id).ok()?))
                .flat_map(|shell| shell.back_faces.iter().chain(&shell.front_faces).copied())
                .collect::<BTreeSet<_>>();
            if body_regions
                .iter()
                .any(|region| matches!(region.kind, RegionKind::Unsupported(_)))
            {
                return Err(self.invalid_topology_source(
                    &body.source,
                    "unsupported_region_type",
                    "body contains a region with an unsupported classification",
                ));
            }
            match body.kind {
                BodyKind::Solid => {
                    if body_regions.len() < 2
                        || !body_regions
                            .iter()
                            .any(|region| region.kind == RegionKind::Solid)
                    {
                        return Err(self.invalid_topology_source(
                            &body.source,
                            "solid_regions",
                            "solid body requires at least two regions including one solid region",
                        ));
                    }
                    if body_face_ids.is_empty() {
                        return Err(self.invalid_topology_source(
                            &body.source,
                            "solid_faces",
                            "solid body requires at least one face",
                        ));
                    }
                    for edge_id in &body.edges {
                        let edge = self.entity(edges, *edge_id, &body.source, "body_edges")?;
                        let visible = edge
                            .half_edges
                            .iter()
                            .filter_map(|id| half_edges.get(usize::try_from(*id).ok()?))
                            .filter(|half_edge| !half_edge.dummy)
                            .collect::<Vec<_>>();
                        if visible.len() != 2
                            || !matches!(
                                (visible[0].sense, visible[1].sense),
                                (Sense::Positive, Sense::Negative)
                                    | (Sense::Negative, Sense::Positive)
                            )
                        {
                            return Err(self.invalid_topology_source(
                                &edge.source,
                                "solid_edge_fins",
                                "solid edge requires exactly two non-dummy fins with opposite senses",
                            ));
                        }
                    }
                }
                BodyKind::Sheet
                    if body_regions
                        .iter()
                        .any(|region| region.kind != RegionKind::Void) =>
                {
                    return Err(self.invalid_topology_source(
                        &body.source,
                        "sheet_regions",
                        "sheet body regions must all be void",
                    ));
                }
                BodyKind::Sheet if body_face_ids.is_empty() => {
                    return Err(self.invalid_topology_source(
                        &body.source,
                        "sheet_faces",
                        "sheet body requires at least one face",
                    ));
                }
                BodyKind::Wire
                    if body_regions.len() != 1 || body_regions[0].kind != RegionKind::Void =>
                {
                    return Err(self.invalid_topology_source(
                        &body.source,
                        "wire_regions",
                        "wire body requires one void region",
                    ));
                }
                BodyKind::Wire | BodyKind::Sheet | BodyKind::General => {}
            }
        }
        let euler_characteristic = i64::try_from(vertices.len()).unwrap_or(i64::MAX)
            - i64::try_from(edges.len()).unwrap_or(i64::MAX)
            + i64::try_from(faces.len()).unwrap_or(i64::MAX);
        Ok(TopologyValidation {
            valid: true,
            closed_loop_count: loops.len(),
            closed_edge_ring_count: edges.len(),
            euler_characteristic,
        })
    }

    fn entity<'b, T>(
        &self,
        values: &'b [T],
        id: BrepId,
        source: &SourceNodeRef,
        relationship: &'static str,
    ) -> Result<&'b T, ParseError> {
        values
            .get(usize::try_from(id).unwrap_or(usize::MAX))
            .ok_or_else(|| {
                self.invalid_topology_source(
                    source,
                    relationship,
                    "document-local B-Rep identifier is out of range",
                )
            })
    }

    fn node(&self, index: u32) -> Result<&'a RawNode, ParseError> {
        self.nodes.get(&index).copied().ok_or_else(|| {
            ParseError::new(
                ErrorKind::InvalidBrepReference,
                0,
                format!("raw node index {index} is unresolved"),
                ErrorDetails::NodeIndex { node_index: index },
            )
        })
    }

    fn field<'b>(&self, node: &'b RawNode, name: &'static str) -> Result<&'b RawField, ParseError> {
        self.roles
            .field(node, name)
            .ok_or_else(|| self.invalid_field(node, name, "required effective field is absent"))
    }

    fn value<'b>(
        &self,
        node: &'b RawNode,
        name: &'static str,
    ) -> Result<&'b FieldValue, ParseError> {
        let field = self.field(node, name)?;
        if field.values.len() != 1 {
            return Err(self.invalid_field(
                node,
                name,
                format!("required scalar has {} values", field.values.len()),
            ));
        }
        Ok(&field.values[0])
    }

    fn pointer(&self, node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
        match self.value(node, name)? {
            FieldValue::PointerIndex(value) => Ok(*value),
            _ => Err(self.invalid_field(node, name, "required field is not a pointer")),
        }
    }

    fn required_pointer(&self, node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
        let target = self.pointer(node, name)?;
        if target == 0 {
            return Err(self.invalid_reference(
                node,
                name,
                target,
                "non-null raw node",
                "required B-Rep pointer is null",
            ));
        }
        Ok(target)
    }

    fn pointer_array(&self, node: &RawNode, name: &'static str) -> Result<Vec<u32>, ParseError> {
        self.field(node, name)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::PointerIndex(index) => Ok(*index),
                _ => Err(self.invalid_field(node, name, "array contains a non-pointer value")),
            })
            .collect()
    }

    fn id(
        &self,
        ids: &BTreeMap<u32, BrepId>,
        node: &RawNode,
        expected: &'static str,
    ) -> Result<BrepId, ParseError> {
        ids.get(&node.index).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                "self",
                node.index,
                expected,
                "node was not assigned a document-local B-Rep identifier",
            )
        })
    }

    fn id_from_target(
        &self,
        node: &RawNode,
        field: &'static str,
        target: u32,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<BrepId, ParseError> {
        if target == 0 {
            return Err(self.invalid_reference(
                node,
                field,
                target,
                expected,
                "required B-Rep pointer is null",
            ));
        }
        let target_node = self.nodes.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                field,
                target,
                expected,
                "required B-Rep pointer is unresolved",
            )
        })?;
        ids.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                field,
                target,
                expected,
                format!(
                    "pointer targets {} rather than {expected}",
                    target_node.definition.name
                ),
            )
        })
    }

    fn required_id(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<BrepId, ParseError> {
        self.id_from_target(node, field, self.pointer(node, field)?, expected, ids)
    }

    fn optional_id(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Option<BrepId>, ParseError> {
        let target = self.pointer(node, field)?;
        if target == 0 || self.record_missing_optional_target(node, field, target)? {
            return Ok(None);
        }
        self.id_from_target(node, field, target, expected, ids)
            .map(Some)
    }

    fn required_id_pair(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<[BrepId; 2], ParseError> {
        let targets = self.pointer_array(node, field)?;
        if targets.len() != 2 {
            return Err(self.invalid_field(
                node,
                field,
                "required pointer pair must contain exactly two values",
            ));
        }
        Ok([
            self.id_from_target(node, field, targets[0], expected, ids)?,
            self.id_from_target(node, field, targets[1], expected, ids)?,
        ])
    }

    fn optional_id_pair(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<[Option<BrepId>; 2], ParseError> {
        let targets = self.pointer_array(node, field)?;
        if targets.len() != 2 {
            return Err(self.invalid_field(
                node,
                field,
                "optional pointer pair must contain exactly two values",
            ));
        }
        let convert = |target, value_index| {
            if target == 0
                || self.record_missing_optional_target(
                    node,
                    &format!("{field}[{value_index}]"),
                    target,
                )?
            {
                Ok(None)
            } else {
                self.id_from_target(node, field, target, expected, ids)
                    .map(Some)
            }
        };
        Ok([convert(targets[0], 0)?, convert(targets[1], 1)?])
    }

    fn required_typed_node(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: &'static str,
    ) -> Result<&'a RawNode, ParseError> {
        let target = self.required_pointer(node, field)?;
        let target_node = self.nodes.get(&target).copied().ok_or_else(|| {
            self.invalid_reference(
                node,
                field,
                target,
                expected,
                "required geometry auxiliary pointer is unresolved",
            )
        })?;
        if self.roles.type_name(target_node) != expected {
            return Err(self.invalid_reference(
                node,
                field,
                target,
                expected,
                format!("pointer targets {}", target_node.definition.name),
            ));
        }
        Ok(target_node)
    }

    fn required_source(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<SourceNodeRef, ParseError> {
        let target = self.required_pointer(node, field)?;
        self.nodes
            .get(&target)
            .copied()
            .map(|item| self.source(item))
            .ok_or_else(|| {
                self.invalid_reference(
                    node,
                    field,
                    target,
                    "raw node",
                    "required source pointer is unresolved",
                )
            })
    }

    fn optional_source(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<SourceNodeRef>, ParseError> {
        let target = self.pointer(node, field)?;
        if target == 0 || self.record_missing_optional_target(node, field, target)? {
            return Ok(None);
        }
        self.nodes
            .get(&target)
            .copied()
            .map(|item| Some(self.source(item)))
            .ok_or_else(|| {
                self.invalid_reference(
                    node,
                    field,
                    target,
                    "raw node",
                    "optional source pointer is non-null but unresolved",
                )
            })
    }

    fn optional_source_if_field(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<SourceNodeRef>, ParseError> {
        if self.roles.field(node, field).is_none() {
            return Ok(None);
        }
        self.optional_source(node, field)
    }

    fn record_missing_optional_target(
        &self,
        node: &RawNode,
        field: &str,
        target: u32,
    ) -> Result<bool, ParseError> {
        if target == 0 || self.nodes.contains_key(&target) {
            return Ok(false);
        }
        push_diagnostic(
            &mut self.diagnostics.borrow_mut(),
            self.max_diagnostics,
            node.byte_range.start,
            BrepDiagnostic {
                code: "reference.unresolved_optional_index",
                message: format!(
                    "optional pointer field {field} targets absent node index {target}; interpreted as zero"
                ),
                role: "optional_reference",
                source: self.source(node),
            },
        )?;
        Ok(true)
    }

    fn null_chain(
        &self,
        owner: &RawNode,
        first_field: &'static str,
        expected: &'static str,
        next_field: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Vec<BrepId>, ParseError> {
        let mut current = self.pointer(owner, first_field)?;
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        while current != 0 {
            if !seen.insert(current) {
                return Err(self.invalid_topology(
                    owner,
                    first_field,
                    format!("{expected} chain contains a cycle at raw index {current}"),
                ));
            }
            let node = self.nodes.get(&current).copied().ok_or_else(|| {
                self.invalid_reference(
                    owner,
                    first_field,
                    current,
                    expected,
                    "chain pointer is unresolved",
                )
            })?;
            let Some(id) = ids.get(&current).copied() else {
                return Err(self.invalid_reference(
                    owner,
                    first_field,
                    current,
                    expected,
                    format!("chain targets {}", node.definition.name),
                ));
            };
            output.push(id);
            current = self.pointer(node, next_field)?;
        }
        Ok(output)
    }

    fn required_ring(
        &self,
        owner: &RawNode,
        first_field: &'static str,
        expected: &'static str,
        next_field: &'static str,
        ids: &BTreeMap<u32, BrepId>,
    ) -> Result<Vec<BrepId>, ParseError> {
        let first = self.required_pointer(owner, first_field)?;
        let mut current = first;
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        loop {
            if !seen.insert(current) {
                if current == first {
                    break;
                }
                return Err(self.invalid_topology(
                    owner,
                    first_field,
                    format!("{expected} ring repeats raw index {current} before closing"),
                ));
            }
            let node = self.nodes.get(&current).copied().ok_or_else(|| {
                self.invalid_reference(
                    owner,
                    first_field,
                    current,
                    expected,
                    "ring pointer is unresolved",
                )
            })?;
            let Some(id) = ids.get(&current).copied() else {
                return Err(self.invalid_reference(
                    owner,
                    first_field,
                    current,
                    expected,
                    format!("ring targets {}", node.definition.name),
                ));
            };
            output.push(id);
            current = self.pointer(node, next_field)?;
            if current == 0 {
                return Err(self.invalid_topology(
                    node,
                    next_field,
                    format!("{expected} ring terminated at null rather than closing"),
                ));
            }
        }
        Ok(output)
    }

    fn byte(&self, node: &RawNode, field: &'static str) -> Result<u8, ParseError> {
        match self.value(node, field)? {
            FieldValue::UnsignedByte(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not an unsigned byte")),
        }
    }

    fn character(&self, node: &RawNode, field: &'static str) -> Result<u8, ParseError> {
        match self.value(node, field)? {
            FieldValue::Character(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not a character")),
        }
    }

    fn optional_character(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<u8>, ParseError> {
        let mut named = node
            .fields
            .iter()
            .filter(|raw| raw.definition.name == field);
        let named_field = named.next();
        if named.next().is_some() {
            return Err(self.invalid_field(node, field, "optional field name is ambiguous"));
        }
        let raw_field = self.roles.field(node, field).or(named_field);
        let Some(raw_field) = raw_field else {
            if node
                .definition
                .fields
                .iter()
                .any(|declared| declared.name == field)
            {
                return Err(self.invalid_field(
                    node,
                    field,
                    "declared optional field is absent from the decoded node",
                ));
            }
            return Ok(None);
        };
        if node
            .fields
            .iter()
            .filter(|raw| raw.definition.name == raw_field.definition.name)
            .count()
            != 1
        {
            return Err(self.invalid_field(node, field, "optional field name is ambiguous"));
        }
        let mut declarations = node
            .definition
            .fields
            .iter()
            .filter(|declared| declared.name == raw_field.definition.name);
        let Some(declared) = declarations.next() else {
            return Err(self.invalid_field(
                node,
                field,
                "decoded optional field has no schema declaration",
            ));
        };
        if declarations.next().is_some() {
            return Err(self.invalid_field(node, field, "optional schema field is ambiguous"));
        }
        if declared != raw_field.definition.as_ref() {
            return Err(self.invalid_field(
                node,
                field,
                "decoded optional field definition disagrees with its schema",
            ));
        }
        if declared.field_type != crate::FieldType::Character
            || declared.element_count != 0
            || !declared.transmitted
        {
            return Err(self.invalid_field(
                node,
                field,
                "optional field schema is not one transmitted character",
            ));
        }
        match raw_field.values.as_slice() {
            [FieldValue::Character(value)] => Ok(Some(*value)),
            _ => Err(self.invalid_field(node, field, "optional field is not one character")),
        }
    }

    fn sense(&self, node: &RawNode, field: &'static str) -> Result<Sense, ParseError> {
        match self.character(node, field)? {
            b'+' => Ok(Sense::Positive),
            b'-' => Ok(Sense::Negative),
            b'?' => Ok(Sense::Unknown),
            value => Err(self.invalid_field(
                node,
                field,
                format!("sense must be '+', '-', or '?', observed 0x{value:02x}"),
            )),
        }
    }

    fn logical(&self, node: &RawNode, field: &'static str) -> Result<bool, ParseError> {
        match self.value(node, field)? {
            FieldValue::Logical(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not logical")),
        }
    }

    fn double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        match self.value(node, field)? {
            FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
            _ => Err(self.invalid_geometry(node, field, "required double is null or non-finite")),
        }
    }

    fn optional_integer(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<i32>, ParseError> {
        match self.value(node, field)? {
            FieldValue::Integer(value) => Ok(*value),
            _ => Err(self.invalid_field(node, field, "required field is not an integer")),
        }
    }

    fn integer_array(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Vec<Option<i32>>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Integer(item) => Ok(*item),
                _ => Err(self.invalid_field(node, field, "array contains a non-integer value")),
            })
            .collect()
    }

    fn optional_double_array(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Vec<Option<f64>>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Double(item) => Ok(item.filter(|item| item.is_finite())),
                _ => Err(self.invalid_field(node, field, "array contains a non-double value")),
            })
            .collect()
    }

    /// The vectors of an attribute value node; a null or non-finite one is `None`,
    /// as an unset real of `REAL_VALUES` is.
    fn optional_vector_array(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Vec<Option<Vector3>>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Vector([Some(x), Some(y), Some(z)])
                    if x.is_finite() && y.is_finite() && z.is_finite() =>
                {
                    Ok(Some(Vector3 {
                        x: *x,
                        y: *y,
                        z: *z,
                    }))
                }
                FieldValue::Vector(_) => Ok(None),
                _ => Err(self.invalid_field(node, field, "array contains a non-vector value")),
            })
            .collect()
    }

    fn tag_array(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Vec<Option<i32>>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Tag(item) => Ok(*item),
                _ => Err(self.invalid_field(node, field, "array contains a non-tag value")),
            })
            .collect()
    }

    fn character_text(&self, node: &RawNode, field: &'static str) -> Result<String, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Character(byte) => Ok(char::from(*byte)),
                _ => Err(self.invalid_field(node, field, "array contains a non-character value")),
            })
            .collect()
    }

    fn unicode_text(&self, node: &RawNode, field: &'static str) -> Result<String, ParseError> {
        let units = self
            .field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::UnicodeCharacter(unit) => Ok(*unit),
                _ => Err(self.invalid_field(node, field, "array contains a non-unicode value")),
            })
            .collect::<Result<Vec<u16>, _>>()?;
        Ok(String::from_utf16_lossy(&units))
    }

    fn optional_double(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<f64>, ParseError> {
        match self.value(node, field)? {
            FieldValue::Double(None) => Ok(None),
            FieldValue::Double(Some(value)) if value.is_finite() => Ok(Some(*value)),
            _ => Err(self.invalid_geometry(node, field, "optional double is not finite")),
        }
    }

    fn optional_vector(
        &self,
        node: &RawNode,
        field: &'static str,
    ) -> Result<Option<Vector3>, ParseError> {
        let (FieldValue::Vector(values) | FieldValue::IntersectionPoint(values)) =
            self.value(node, field)?
        else {
            return Err(self.invalid_field(node, field, "optional field is not a vector"));
        };
        match values {
            [None, None, None] => Ok(None),
            [Some(x), Some(y), Some(z)] if x.is_finite() && y.is_finite() && z.is_finite() => {
                Ok(Some(Vector3 {
                    x: *x,
                    y: *y,
                    z: *z,
                }))
            }
            _ => Err(self.invalid_geometry(
                node,
                field,
                "optional vector is partly null or not finite",
            )),
        }
    }

    fn positive_double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        let value = self.double(node, field)?;
        if value <= 0.0 {
            return Err(self.invalid_geometry(node, field, "value must be positive"));
        }
        Ok(value)
    }

    fn nonnegative_double(&self, node: &RawNode, field: &'static str) -> Result<f64, ParseError> {
        let value = self.double(node, field)?;
        if value < 0.0 {
            return Err(self.invalid_geometry(node, field, "value must be non-negative"));
        }
        Ok(value)
    }

    fn vector(&self, node: &RawNode, field: &'static str) -> Result<Vector3, ParseError> {
        let (FieldValue::Vector(values) | FieldValue::IntersectionPoint(values)) =
            self.value(node, field)?
        else {
            return Err(self.invalid_field(node, field, "required field is not a vector"));
        };
        let [Some(x), Some(y), Some(z)] = values else {
            return Err(self.invalid_geometry(node, field, "required vector is null"));
        };
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(self.invalid_geometry(
                node,
                field,
                "vector contains a non-finite component",
            ));
        }
        Ok(Vector3 {
            x: *x,
            y: *y,
            z: *z,
        })
    }

    fn nonnegative_integer(&self, node: &RawNode, field: &'static str) -> Result<u32, ParseError> {
        match self.value(node, field)? {
            FieldValue::Integer(Some(value)) if *value >= 0 => {
                Ok(u32::try_from(*value).unwrap_or(u32::MAX))
            }
            _ => Err(self.invalid_geometry(node, field, "required integer is null or negative")),
        }
    }

    fn positive_short(&self, node: &RawNode, field: &'static str) -> Result<u16, ParseError> {
        match self.value(node, field)? {
            FieldValue::ShortInteger(Some(value)) if *value > 0 => {
                Ok(u16::try_from(*value).unwrap_or(u16::MAX))
            }
            _ => Err(self.invalid_geometry(
                node,
                field,
                "required short integer is null or non-positive",
            )),
        }
    }

    fn nonnegative_short(&self, node: &RawNode, field: &'static str) -> Result<u16, ParseError> {
        match self.value(node, field)? {
            FieldValue::ShortInteger(Some(value)) if *value >= 0 => {
                Ok(u16::try_from(*value).unwrap_or(u16::MAX))
            }
            _ => Err(self.invalid_geometry(
                node,
                field,
                "required short integer is null or negative",
            )),
        }
    }

    fn double_pair(&self, node: &RawNode, field: &'static str) -> Result<[f64; 2], ParseError> {
        let values = self.double_array(node, field)?;
        self.validate_count(node, field, 2, values.len())?;
        Ok([values[0], values[1]])
    }

    fn double_array(&self, node: &RawNode, field: &'static str) -> Result<Vec<f64>, ParseError> {
        self.field(node, field)?
            .values
            .iter()
            .map(|value| match value {
                FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
                _ => Err(self.invalid_geometry(
                    node,
                    field,
                    "double array contains null or non-finite data",
                )),
            })
            .collect()
    }

    fn double_array_prefix(
        &self,
        node: &RawNode,
        field: &'static str,
        count: usize,
    ) -> Result<Vec<f64>, ParseError> {
        let values = &self.field(node, field)?.values;
        if values.len() < count {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!(
                    "{field} has {} values; expected at least {count}",
                    values.len()
                ),
                ErrorDetails::CountMismatch {
                    field,
                    expected: count,
                    actual: values.len(),
                },
            ));
        }
        let output = values[..count]
            .iter()
            .map(|value| match value {
                FieldValue::Double(Some(value)) if value.is_finite() => Ok(*value),
                _ => Err(self.invalid_geometry(
                    node,
                    field,
                    "meaningful knot prefix contains null or non-finite data",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Values after the declared count are ignored: Parasolid pads the
        // array with nulls, SolidWorks 2019 leaves earlier values there.
        Ok(output)
    }

    fn positive_short_array_prefix(
        &self,
        node: &RawNode,
        field: &'static str,
        count: usize,
    ) -> Result<Vec<u16>, ParseError> {
        let values = &self.field(node, field)?.values;
        if values.len() < count {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!(
                    "{field} has {} values; expected at least {count}",
                    values.len()
                ),
                ErrorDetails::CountMismatch {
                    field,
                    expected: count,
                    actual: values.len(),
                },
            ));
        }
        let output = values[..count]
            .iter()
            .map(|value| match value {
                FieldValue::ShortInteger(Some(value)) if *value > 0 => {
                    Ok(u16::try_from(*value).unwrap_or(u16::MAX))
                }
                _ => Err(self.invalid_geometry(
                    node,
                    field,
                    "multiplicity array contains null or non-positive data",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Values after the declared count are ignored, as for the knots.
        Ok(output)
    }

    #[allow(clippy::unused_self)]
    fn validate_count(
        &self,
        node: &RawNode,
        field: &'static str,
        expected: usize,
        actual: usize,
    ) -> Result<(), ParseError> {
        if expected != actual {
            return Err(ParseError::new(
                ErrorKind::InvalidGeometryParameter,
                node.byte_range.start,
                format!("{field} has {actual} values; expected {expected}"),
                ErrorDetails::CountMismatch {
                    field,
                    expected,
                    actual,
                },
            ));
        }
        Ok(())
    }

    fn source(&self, node: &RawNode) -> SourceNodeRef {
        let node_id = self
            .roles
            .field(node, "node_id")
            .and_then(|field| field.values.first())
            .and_then(|value| match value {
                FieldValue::Integer(value) => *value,
                _ => None,
            });
        SourceNodeRef {
            node_index: node.index,
            node_type: node.node_type,
            type_name: node.definition.name.clone(),
            node_id,
            byte_range: node.byte_range.clone(),
        }
    }

    #[allow(clippy::unused_self)]
    fn invalid_field(
        &self,
        node: &RawNode,
        field: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            node.byte_range.start,
            message,
            ErrorDetails::BrepField {
                node_index: node.index,
                field,
            },
        )
    }

    #[allow(clippy::unused_self)]
    fn invalid_reference(
        &self,
        node: &RawNode,
        field: &'static str,
        target_index: u32,
        expected_type: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidBrepReference,
            node.byte_range.start,
            message,
            ErrorDetails::BrepReference {
                node_index: node.index,
                field,
                target_index,
                expected_type,
            },
        )
    }

    fn invalid_topology(
        &self,
        node: &RawNode,
        relationship: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        self.invalid_topology_source(&self.source(node), relationship, message)
    }

    #[allow(clippy::unused_self)]
    fn invalid_topology_source(
        &self,
        source: &SourceNodeRef,
        relationship: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidBrepTopology,
            source.byte_range.start,
            message,
            ErrorDetails::BrepInvariant {
                node_index: source.node_index,
                relationship,
            },
        )
    }

    #[allow(clippy::unused_self)]
    fn invalid_geometry(
        &self,
        node: &RawNode,
        field: &'static str,
        message: impl Into<String>,
    ) -> ParseError {
        ParseError::new(
            ErrorKind::InvalidGeometryParameter,
            node.byte_range.start,
            message,
            ErrorDetails::BrepField {
                node_index: node.index,
                field,
            },
        )
    }
}

fn push_diagnostic(
    diagnostics: &mut Vec<BrepDiagnostic>,
    max_diagnostics: usize,
    offset: usize,
    diagnostic: BrepDiagnostic,
) -> Result<(), ParseError> {
    let actual = diagnostics.len().saturating_add(1);
    if actual > max_diagnostics {
        return Err(ParseError::limit(
            offset,
            "diagnostics",
            actual,
            max_diagnostics,
        ));
    }
    diagnostics.push(diagnostic);
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::similar_names,
    reason = "Fixture indices and values are bounded and kept explicit for regression checks."
)]
#[allow(
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use crate::{FieldDefinition, FieldType, RawField, SchemaSource, TypeDefinition};

    use super::*;

    fn field(
        name: &str,
        pointer_class: u16,
        values: Vec<FieldValue>,
    ) -> (FieldDefinition, Vec<FieldValue>) {
        let field_type = values
            .first()
            .map_or(FieldType::Double, FieldValue::field_type);
        (
            FieldDefinition {
                name: name.to_owned(),
                field_type,
                pointer_class,
                element_count: u32::from(values.len() > 1),
                transmitted: true,
            },
            values,
        )
    }

    fn node(
        index: u32,
        node_type: u16,
        name: &str,
        fields: Vec<(FieldDefinition, Vec<FieldValue>)>,
    ) -> RawNode {
        let definitions = fields
            .iter()
            .map(|(definition, _)| definition.clone())
            .collect::<Vec<_>>();
        let definition = TypeDefinition::from_fields(
            node_type,
            name,
            format!("Synthetic {name}"),
            definitions,
            SchemaSource::Base,
        );
        let variable_length = definition.variable.then(|| {
            fields.last().map_or(0, |(_, values)| {
                u32::try_from(values.len()).unwrap_or(u32::MAX)
            })
        });
        RawNode {
            node_type,
            index,
            user_fields: Vec::new(),
            variable_length,
            definition: std::sync::Arc::new(definition),
            first_schema: None,
            fields: fields
                .into_iter()
                .map(|(definition, values)| RawField {
                    definition: std::sync::Arc::new(definition),
                    values,
                    byte_range: usize::try_from(index).unwrap_or(usize::MAX)
                        ..usize::try_from(index)
                            .unwrap_or(usize::MAX)
                            .saturating_add(1),
                })
                .collect(),
            byte_range: usize::try_from(index).unwrap_or(usize::MAX)
                ..usize::try_from(index)
                    .unwrap_or(usize::MAX)
                    .saturating_add(1),
        }
    }

    fn common_curve_fields(owner: u32) -> Vec<(FieldDefinition, Vec<FieldValue>)> {
        vec![
            field("owner", 1010, vec![FieldValue::PointerIndex(owner)]),
            field("next", 1008, vec![FieldValue::PointerIndex(0)]),
            field("previous", 1008, vec![FieldValue::PointerIndex(0)]),
            field("geometric_owner", 141, vec![FieldValue::PointerIndex(0)]),
            field("sense", 0, vec![FieldValue::Character(b'+')]),
        ]
    }

    fn common_surface_fields(owner: u32) -> Vec<(FieldDefinition, Vec<FieldValue>)> {
        vec![
            field("owner", 1007, vec![FieldValue::PointerIndex(owner)]),
            field("next", 1006, vec![FieldValue::PointerIndex(0)]),
            field("previous", 1006, vec![FieldValue::PointerIndex(0)]),
            field("geometric_owner", 141, vec![FieldValue::PointerIndex(0)]),
            field("sense", 0, vec![FieldValue::Character(b'+')]),
        ]
    }

    fn synthetic_attribute(
        declared_kinds: &[u8],
        value_targets: &[u32],
        user_fields: Vec<Option<i32>>,
    ) -> Result<Attribute, ParseError> {
        let mut definition = node(
            2,
            80,
            "ATTRIB_DEF",
            vec![
                field("identifier", 79, vec![FieldValue::PointerIndex(3)]),
                field("type_id", 0, vec![FieldValue::Integer(Some(1))]),
                field(
                    "fields",
                    0,
                    declared_kinds
                        .iter()
                        .copied()
                        .map(FieldValue::UnsignedByte)
                        .collect(),
                ),
            ],
        );
        definition.user_fields = user_fields;
        let nodes = vec![
            node(
                1,
                81,
                "ATTRIBUTE",
                vec![
                    field("node_id", 0, vec![FieldValue::Integer(Some(1))]),
                    field("definition", 80, vec![FieldValue::PointerIndex(2)]),
                    field("owner", 0, vec![FieldValue::PointerIndex(4)]),
                    field(
                        "fields",
                        0,
                        value_targets
                            .iter()
                            .copied()
                            .map(FieldValue::PointerIndex)
                            .collect(),
                    ),
                ],
            ),
            definition,
            node(
                3,
                79,
                "ATT_DEF_ID",
                vec![field("string", 0, vec![FieldValue::Character(b'x')])],
            ),
            node(4, 999, "OWNER", vec![]),
        ];
        Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map_attribute(1)
    }

    #[test]
    fn validates_attribute_definition_zero_padding() {
        assert!(synthetic_attribute(&[1, 0], &[0], vec![Some(7)]).is_ok());
        assert_eq!(
            synthetic_attribute(&[1, 0], &[0], vec![])
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
        assert_eq!(
            synthetic_attribute(&[1, 2], &[0], vec![Some(7)])
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
        assert_eq!(
            synthetic_attribute(&[1], &[0, 0], vec![])
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
    }

    #[test]
    fn maps_synthetic_intersection_uv_data_with_terminator_slots() {
        let values = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let node = node(
            10,
            204,
            "INTERSECTION_DATA",
            vec![
                field("uv_type", 0, vec![FieldValue::UnsignedByte(2)]),
                field(
                    "values",
                    0,
                    values
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                ),
            ],
        );
        let mapper = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            std::slice::from_ref(&node),
            RoleAccess::Named,
        );
        let data = mapper
            .map_intersection_data(&node, 1, true, true)
            .expect("decode synthetic UV data");
        assert_eq!(data.uv_type, IntersectionUvType::First);
        assert_eq!(
            data.start_terminator.unwrap().first_surface,
            Some([Some(0.1), Some(0.2)])
        );
        assert_eq!(
            data.chart.unwrap()[0].first_surface,
            Some([Some(0.3), Some(0.4)])
        );
        assert_eq!(
            data.end_terminator.unwrap().first_surface,
            Some([Some(0.5), Some(0.6)])
        );
    }

    fn wire_acorn_nodes() -> Vec<RawNode> {
        vec![
            node(
                1,
                12,
                "BODY",
                vec![
                    field("body_type", 0, vec![FieldValue::UnsignedByte(2)]),
                    field("res_size", 0, vec![FieldValue::Double(Some(1.0e-6))]),
                    field("res_linear", 0, vec![FieldValue::Double(Some(1.0e-8))]),
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(4)]),
                ],
            ),
            node(
                2,
                13,
                "REGION",
                vec![
                    field("type", 0, vec![FieldValue::Character(b'V')]),
                    field("body", 12, vec![FieldValue::PointerIndex(1)]),
                    field("shell", 14, vec![FieldValue::PointerIndex(3)]),
                    field("next", 13, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                3,
                14,
                "SHELL",
                vec![
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("face", 15, vec![FieldValue::PointerIndex(0)]),
                    field("front_face", 15, vec![FieldValue::PointerIndex(0)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(4)]),
                    field("next", 14, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                4,
                26,
                "VERTEX",
                vec![
                    field("point", 88, vec![FieldValue::PointerIndex(5)]),
                    field("tolerance", 0, vec![FieldValue::Double(None)]),
                    field("owner", 1003, vec![FieldValue::PointerIndex(3)]),
                    field("next", 26, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                5,
                88,
                "POINT",
                vec![
                    field(
                        "pvec",
                        0,
                        vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
                    ),
                    field("owner", 1003, vec![FieldValue::PointerIndex(4)]),
                ],
            ),
        ]
    }

    fn nodes_with_valid_then_broken_attributes() -> Vec<RawNode> {
        let mut nodes = wire_acorn_nodes();
        nodes.extend([
            node(
                6,
                81,
                "ATTRIBUTE",
                vec![
                    field("node_id", 0, vec![FieldValue::Integer(Some(6))]),
                    field("definition", 80, vec![FieldValue::PointerIndex(7)]),
                    field("owner", 0, vec![FieldValue::PointerIndex(1)]),
                    field("fields", 0, vec![FieldValue::PointerIndex(9)]),
                ],
            ),
            node(
                7,
                80,
                "ATTRIB_DEF",
                vec![
                    field("identifier", 79, vec![FieldValue::PointerIndex(8)]),
                    field("type_id", 0, vec![FieldValue::Integer(Some(1))]),
                    field("fields", 0, vec![FieldValue::UnsignedByte(1)]),
                ],
            ),
            node(
                8,
                79,
                "ATT_DEF_ID",
                vec![field("string", 0, vec![FieldValue::Character(b'x')])],
            ),
            node(
                9,
                82,
                "INT_VALUES",
                vec![field("values", 0, vec![FieldValue::Integer(Some(42))])],
            ),
            node(
                10,
                81,
                "ATTRIBUTE",
                vec![
                    field("node_id", 0, vec![FieldValue::Integer(Some(10))]),
                    field("definition", 80, vec![FieldValue::PointerIndex(99)]),
                    field("owner", 0, vec![FieldValue::PointerIndex(1)]),
                    field("fields", 0, vec![]),
                ],
            ),
        ]);
        nodes
    }

    /// A root assembly (6) placing the body through a transform (7, 8) and a
    /// negative instance (9) of a sub-assembly (10).
    fn assembly_nodes(part: u32, instance_type: u8, transform: u32, scale: f64) -> Vec<RawNode> {
        let pointer = |value: u32| vec![FieldValue::PointerIndex(value)];
        let double = |value: f64| vec![FieldValue::Double(Some(value))];
        let integer = |value: i32| vec![FieldValue::Integer(Some(value))];
        vec![
            node(
                6,
                10,
                "ASSEMBLY",
                vec![
                    field("res_size", 0, double(1.0e3)),
                    field("res_linear", 0, double(1.0e-8)),
                    field("ref_instance", 11, pointer(0)),
                    field("type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("sub_instance", 11, pointer(7)),
                ],
            ),
            node(
                7,
                11,
                "INSTANCE",
                vec![
                    field("node_id", 0, integer(1)),
                    field("type", 0, vec![FieldValue::UnsignedByte(instance_type)]),
                    field("part", 1005, pointer(part)),
                    field("transform", 100, pointer(transform)),
                    field("assembly", 10, pointer(6)),
                    field("next_in_part", 11, pointer(9)),
                    field("next_of_part", 11, pointer(0)),
                ],
            ),
            node(
                8,
                100,
                "TRANSFORM",
                vec![
                    field("node_id", 0, integer(2)),
                    field(
                        "rotation_matrix",
                        0,
                        [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                            .into_iter()
                            .map(|value| FieldValue::Double(Some(value)))
                            .collect(),
                    ),
                    field(
                        "translation_vector",
                        0,
                        vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
                    ),
                    field("scale", 0, double(scale)),
                    field("flag", 0, integer(7)),
                    field(
                        "perspective_vector",
                        0,
                        vec![FieldValue::Vector([None, None, None])],
                    ),
                ],
            ),
            node(
                9,
                11,
                "INSTANCE",
                vec![
                    field("node_id", 0, integer(3)),
                    field("type", 0, vec![FieldValue::UnsignedByte(2)]),
                    field("part", 1005, pointer(10)),
                    field("transform", 100, pointer(0)),
                    field("assembly", 10, pointer(6)),
                    field("next_in_part", 11, pointer(0)),
                    field("next_of_part", 11, pointer(0)),
                ],
            ),
            node(
                10,
                10,
                "ASSEMBLY",
                vec![
                    field("res_size", 0, double(1.0e3)),
                    field("res_linear", 0, double(1.0e-8)),
                    field("ref_instance", 11, pointer(9)),
                    field("type", 0, vec![FieldValue::UnsignedByte(3)]),
                    field("sub_instance", 11, pointer(0)),
                ],
            ),
        ]
    }

    fn nodes_with_assembly_owned_attributes(broken_node: Option<u32>) -> Vec<RawNode> {
        let mut nodes = wire_acorn_nodes();
        let mut assemblies = assembly_nodes(1, 1, 8, 2.0);
        if let Some(index) = broken_node {
            let record = assemblies
                .iter_mut()
                .find(|node| node.index == index)
                .expect("selected assembly record exists");
            let type_field = record
                .fields
                .iter_mut()
                .find(|field| field.definition.name == "type")
                .expect("assembly record has a type");
            type_field.values = vec![FieldValue::UnsignedByte(99)];
        }
        nodes.extend(assemblies);
        nodes.extend([
            node(
                20,
                81,
                "ATTRIBUTE",
                vec![
                    field("node_id", 0, vec![FieldValue::Integer(Some(20))]),
                    field("definition", 80, vec![FieldValue::PointerIndex(21)]),
                    field("owner", 0, vec![FieldValue::PointerIndex(6)]),
                    field("fields", 0, vec![FieldValue::PointerIndex(23)]),
                ],
            ),
            node(
                21,
                80,
                "ATTRIB_DEF",
                vec![
                    field("identifier", 79, vec![FieldValue::PointerIndex(22)]),
                    field("type_id", 0, vec![FieldValue::Integer(Some(1))]),
                    field("fields", 0, vec![FieldValue::UnsignedByte(1)]),
                ],
            ),
            node(
                22,
                79,
                "ATT_DEF_ID",
                vec![field("string", 0, vec![FieldValue::Character(b'x')])],
            ),
            node(
                23,
                82,
                "INT_VALUES",
                vec![field("values", 0, vec![FieldValue::Integer(Some(42))])],
            ),
            node(
                24,
                81,
                "ATTRIBUTE",
                vec![
                    field("node_id", 0, vec![FieldValue::Integer(Some(24))]),
                    field("definition", 80, vec![FieldValue::PointerIndex(21)]),
                    field("owner", 0, vec![FieldValue::PointerIndex(7)]),
                    field("fields", 0, vec![FieldValue::PointerIndex(23)]),
                ],
            ),
        ]);
        nodes
    }

    #[test]
    fn maps_assembly_instances_and_transforms_beside_the_bodies() {
        let mut nodes = wire_acorn_nodes();
        nodes.extend(assembly_nodes(1, 1, 8, 2.0));
        let model = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map();
        assert!(model.is_ok());
        if let Ok(model) = model {
            assert!(model.topology.valid);
            assert_eq!(model.assemblies.len(), 2);
            assert_eq!(model.instances.len(), 2);
            let root = &model.assemblies[0];
            assert_eq!(root.kind, AssemblyKind::Collective);
            assert_eq!(root.instances, [0, 1]);
            assert!(root.referencing_instances.is_empty());
            assert_eq!(root.source.node_id, None);
            let sub = &model.assemblies[1];
            assert_eq!(sub.kind, AssemblyKind::Disjunctive);
            assert_eq!(sub.referencing_instances, [1]);
            assert!(sub.instances.is_empty());
            let placed = &model.instances[0];
            assert!(!placed.negative);
            assert_eq!(placed.part, InstancePart::Body(0));
            assert_eq!(placed.assembly, 0);
            assert_eq!(placed.source.node_id, Some(1));
            assert!(placed.transform.is_some());
            if let Some(transform) = &placed.transform {
                assert_eq!(
                    transform.translation,
                    Vector3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    }
                );
                assert_eq!(
                    transform.perspective,
                    Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    }
                );
                assert!(transform.has(Transform::TRANSLATION));
                assert!(transform.has(Transform::ROTATION));
                assert!(transform.has(Transform::SCALING));
                assert!(!transform.has(Transform::REFLECTION));
                assert_eq!(
                    transform.apply(Vector3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0
                    }),
                    Vector3 {
                        x: 2.0,
                        y: 6.0,
                        z: 6.0
                    }
                );
                assert_eq!(transform.source.node_id, Some(2));
            }
            let negative = &model.instances[1];
            assert!(negative.negative);
            assert_eq!(negative.part, InstancePart::Assembly(1));
            assert!(negative.transform.is_none());
        }
    }

    #[test]
    fn rejects_malformed_instances_and_transforms() {
        let cases = [
            // The part is a POINT.
            (
                assembly_nodes(5, 1, 8, 2.0),
                ErrorKind::InvalidBrepReference,
            ),
            // Unknown instance type.
            (assembly_nodes(1, 3, 8, 2.0), ErrorKind::InvalidBrepField),
            // The transform pointer targets the body.
            (
                assembly_nodes(1, 1, 1, 2.0),
                ErrorKind::InvalidBrepReference,
            ),
            // A zero scale collapses the part.
            (
                assembly_nodes(1, 1, 8, 0.0),
                ErrorKind::InvalidGeometryParameter,
            ),
        ];
        for (extra, kind) in cases {
            let mut nodes = wire_acorn_nodes();
            nodes.extend(extra);
            let error = Mapper::new(
                BrepSourceFormat::Binary,
                "SCH_TEST",
                &nodes,
                RoleAccess::Named,
            )
            .map()
            .err();
            assert_eq!(error.map(|error| error.kind()), Some(kind));
        }
    }

    fn sheet_nodes(region_type: u8) -> Vec<RawNode> {
        vec![
            node(
                1,
                12,
                "BODY",
                vec![
                    field("body_type", 0, vec![FieldValue::UnsignedByte(3)]),
                    field("res_size", 0, vec![FieldValue::Double(Some(1.0e-6))]),
                    field("res_linear", 0, vec![FieldValue::Double(Some(1.0e-8))]),
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                2,
                13,
                "REGION",
                vec![
                    field("type", 0, vec![FieldValue::Character(region_type)]),
                    field("body", 12, vec![FieldValue::PointerIndex(1)]),
                    field("shell", 14, vec![FieldValue::PointerIndex(3)]),
                    field("next", 13, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                3,
                14,
                "SHELL",
                vec![
                    field("region", 13, vec![FieldValue::PointerIndex(2)]),
                    field("face", 15, vec![FieldValue::PointerIndex(4)]),
                    field("front_face", 15, vec![FieldValue::PointerIndex(4)]),
                    field("edge", 17, vec![FieldValue::PointerIndex(0)]),
                    field("vertex", 26, vec![FieldValue::PointerIndex(0)]),
                    field("next", 14, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
            node(
                4,
                15,
                "FACE",
                vec![
                    field("shell", 14, vec![FieldValue::PointerIndex(3)]),
                    field("front_shell", 14, vec![FieldValue::PointerIndex(3)]),
                    field("loop", 16, vec![FieldValue::PointerIndex(0)]),
                    field("surface", 141, vec![FieldValue::PointerIndex(0)]),
                    field("sense", 0, vec![FieldValue::Character(b'+')]),
                    field("next", 15, vec![FieldValue::PointerIndex(0)]),
                    field("next_front", 15, vec![FieldValue::PointerIndex(0)]),
                ],
            ),
        ]
    }

    #[test]
    fn sheet_regions_must_be_void_for_text_and_binary_sources() {
        for source_format in [BrepSourceFormat::Text, BrepSourceFormat::Binary] {
            let void_nodes = sheet_nodes(b'V');
            let void_model = Mapper::new(source_format, "SCH_TEST", &void_nodes, RoleAccess::Named)
                .map()
                .expect("sheet with one void region and one face is valid");
            assert!(void_model.topology.valid);

            let solid_nodes = sheet_nodes(b'S');
            let error = Mapper::new(source_format, "SCH_TEST", &solid_nodes, RoleAccess::Named)
                .map()
                .expect_err("sheet with a solid region violates XT topology rules");
            assert_eq!(error.kind(), ErrorKind::InvalidBrepTopology);
        }
    }

    #[test]
    fn unsupported_region_invalidates_body_topology() {
        let source = SourceNodeRef {
            node_index: 1,
            node_type: 12,
            type_name: "BODY".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        let body = Body {
            id: 0,
            kind: BodyKind::Sheet,
            size_resolution: 0.0,
            linear_resolution: 0.0,
            regions: vec![0],
            edges: vec![],
            vertices: vec![],
            attributes: vec![],
            source: source.clone(),
        };
        let region = Region {
            id: 0,
            kind: RegionKind::Unsupported(0x3f),
            body: 0,
            shells: vec![],
            source,
        };
        let mapper = Mapper::new(BrepSourceFormat::Binary, "SCH_TEST", &[], RoleAccess::Named);

        let error = mapper
            .validate_topology(&[body], &[region], &[], &[], &[], &[], &[], &[])
            .expect_err("unsupported region classifications cannot form a valid body");

        assert_eq!(error.kind(), ErrorKind::InvalidBrepTopology);
        assert!(matches!(
            error.details(),
            ErrorDetails::BrepInvariant {
                relationship: "unsupported_region_type",
                ..
            }
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn maps_wire_acorn_analytic_line_and_exact_nurbs_curve() {
        let mut nodes = wire_acorn_nodes();
        let mut line_fields = common_curve_fields(4);
        line_fields.extend([
            field(
                "pvec",
                0,
                vec![FieldValue::Vector([Some(0.0), Some(0.0), Some(0.0)])],
            ),
            field(
                "direction",
                0,
                vec![FieldValue::Vector([Some(1.0), Some(0.0), Some(0.0)])],
            ),
        ]);
        nodes.push(node(6, 30, "LINE", line_fields));

        let mut b_curve_fields = common_curve_fields(4);
        b_curve_fields.push(field("nurbs", 136, vec![FieldValue::PointerIndex(8)]));
        nodes.push(node(7, 134, "B_CURVE", b_curve_fields));
        nodes.extend([
            node(
                8,
                136,
                "NURBS_CURVE",
                vec![
                    field("degree", 0, vec![FieldValue::ShortInteger(Some(2))]),
                    field("n_vertices", 0, vec![FieldValue::Integer(Some(3))]),
                    field("vertex_dim", 0, vec![FieldValue::ShortInteger(Some(3))]),
                    field("n_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("periodic", 0, vec![FieldValue::Logical(false)]),
                    field("closed", 0, vec![FieldValue::Logical(false)]),
                    field("rational", 0, vec![FieldValue::Logical(false)]),
                    field("curve_form", 0, vec![FieldValue::UnsignedByte(0)]),
                    field("bspline_vertices", 45, vec![FieldValue::PointerIndex(9)]),
                    field("knot_mult", 127, vec![FieldValue::PointerIndex(10)]),
                    field("knots", 128, vec![FieldValue::PointerIndex(11)]),
                ],
            ),
            node(
                9,
                45,
                "BSPLINE_VERTICES",
                vec![field(
                    "vertices",
                    0,
                    [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 2.0, 0.0, 0.0]
                        .into_iter()
                        .map(|value| FieldValue::Double(Some(value)))
                        .collect(),
                )],
            ),
            node(
                10,
                127,
                "KNOT_MULT",
                vec![field(
                    "mult",
                    0,
                    vec![
                        FieldValue::ShortInteger(Some(3)),
                        FieldValue::ShortInteger(Some(3)),
                        FieldValue::ShortInteger(Some(0)),
                        FieldValue::ShortInteger(Some(0)),
                    ],
                )],
            ),
            node(
                11,
                128,
                "KNOT_SET",
                vec![field(
                    "knots",
                    0,
                    vec![
                        FieldValue::Double(Some(0.0)),
                        FieldValue::Double(Some(1.0)),
                        FieldValue::Double(None),
                        FieldValue::Double(None),
                    ],
                )],
            ),
        ]);

        let model = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map();
        assert!(model.is_ok());
        if let Ok(model) = model {
            assert!(model.complete);
            assert_eq!(model.bodies[0].kind, BodyKind::Wire);
            assert_eq!(model.vertices.len(), 1);
            assert_eq!(
                model.metrics.bounding_box.map(|bounds| bounds.minimum),
                Some(Vector3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                })
            );
            assert!(matches!(model.curves[0].kind, CurveKind::Line { .. }));
            assert!(matches!(model.curves[1].kind, CurveKind::Nurbs(_)));
            if let CurveKind::Nurbs(nurbs) = &model.curves[1].kind {
                assert_eq!(nurbs.degree, 2);
                assert_eq!(nurbs.control_vertices.len(), 3);
                assert_eq!(nurbs.knot_multiplicities, [3, 3]);
                assert_eq!(nurbs.knots, [0.0, 1.0]);
                assert_eq!(nurbs.sources.len(), 4);
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn maps_i7_exact_analytic_trim_nurbs_and_offset_geometry() {
        fn vector(name: &str, values: [f64; 3]) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Vector(values.map(Some))])
        }

        fn double(name: &str, value: f64) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Double(Some(value))])
        }

        let mut nodes = Vec::new();

        let mut line = common_curve_fields(0);
        line.extend([
            vector("pvec", [0.0, 0.0, 0.0]),
            vector("direction", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(1, 30, "LINE", line));

        let mut circle = common_curve_fields(0);
        circle.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("radius", 5.0),
        ]);
        nodes.push(node(2, 31, "CIRCLE", circle));

        let mut ellipse = common_curve_fields(0);
        ellipse.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("major_radius", 8.0),
            double("minor_radius", 4.0),
        ]);
        nodes.push(node(3, 32, "ELLIPSE", ellipse));

        let mut parabola = common_curve_fields(0);
        parabola.extend([
            vector("origin", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("focal_length", 2.0),
        ]);
        nodes.push(node(4, 33, "PARABOLA", parabola));

        let mut hyperbola = common_curve_fields(0);
        hyperbola.extend([
            vector("origin", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            double("transverse_radius", 3.0),
            double("conjugate_radius", 2.0),
        ]);
        nodes.push(node(5, 34, "HYPERBOLA", hyperbola));

        let mut trimmed = common_curve_fields(0);
        trimmed.extend([
            field("basis_curve", 1008, vec![FieldValue::PointerIndex(2)]),
            vector("point_1", [5.0, 0.0, 0.0]),
            vector("point_2", [-5.0, 0.0, 0.0]),
            double("parm_1", 0.0),
            double("parm_2", std::f64::consts::PI),
        ]);
        nodes.push(node(6, 35, "TRIMMED_CURVE", trimmed));

        let mut plane = common_surface_fields(0);
        plane.extend([
            vector("pvec", [0.0, 0.0, 0.0]),
            vector("normal", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(7, 50, "PLANE", plane));

        let mut cylinder = common_surface_fields(0);
        cylinder.extend([
            vector("pvec", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("radius", 5.0),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(8, 51, "CYLINDER", cylinder));

        let mut cone = common_surface_fields(0);
        cone.extend([
            vector("pvec", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("radius", 8.0),
            double("sin_half_angle", -0.316_227_766_016_837_94),
            double("cos_half_angle", 0.948_683_298_050_513_8),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(9, 52, "CONE", cone));

        let mut sphere = common_surface_fields(0);
        sphere.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            double("radius", 6.0),
            vector("axis", [0.0, 0.0, 1.0]),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(10, 53, "SPHERE", sphere));

        let mut torus = common_surface_fields(0);
        torus.extend([
            vector("centre", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("major_radius", 8.0),
            double("minor_radius", 2.0),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        nodes.push(node(11, 54, "TORUS", torus));

        let mut offset = common_surface_fields(0);
        offset.extend([
            field("surface", 1006, vec![FieldValue::PointerIndex(7)]),
            double("offset", 2.0),
        ]);
        nodes.push(node(12, 55, "OFFSET_SURF", offset));

        let mut b_surface = common_surface_fields(0);
        b_surface.push(field("nurbs", 137, vec![FieldValue::PointerIndex(14)]));
        nodes.push(node(13, 135, "B_SURFACE", b_surface));
        nodes.extend([
            node(
                14,
                137,
                "NURBS_SURF",
                vec![
                    field("u_degree", 0, vec![FieldValue::ShortInteger(Some(1))]),
                    field("v_degree", 0, vec![FieldValue::ShortInteger(Some(1))]),
                    field("n_u_vertices", 0, vec![FieldValue::Integer(Some(2))]),
                    field("n_v_vertices", 0, vec![FieldValue::Integer(Some(2))]),
                    field("vertex_dim", 0, vec![FieldValue::ShortInteger(Some(3))]),
                    field("n_u_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("n_v_knots", 0, vec![FieldValue::Integer(Some(2))]),
                    field("u_knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("v_knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                    field("u_periodic", 0, vec![FieldValue::Logical(false)]),
                    field("v_periodic", 0, vec![FieldValue::Logical(false)]),
                    field("u_closed", 0, vec![FieldValue::Logical(false)]),
                    field("v_closed", 0, vec![FieldValue::Logical(false)]),
                    field("rational", 0, vec![FieldValue::Logical(false)]),
                    field("surface_form", 0, vec![FieldValue::UnsignedByte(0)]),
                    field("bspline_vertices", 45, vec![FieldValue::PointerIndex(15)]),
                    field("u_knot_mult", 127, vec![FieldValue::PointerIndex(16)]),
                    field("v_knot_mult", 127, vec![FieldValue::PointerIndex(17)]),
                    field("u_knots", 128, vec![FieldValue::PointerIndex(18)]),
                    field("v_knots", 128, vec![FieldValue::PointerIndex(19)]),
                ],
            ),
            node(
                15,
                45,
                "BSPLINE_VERTICES",
                vec![field(
                    "vertices",
                    0,
                    [
                        0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0,
                    ]
                    .into_iter()
                    .map(|value| FieldValue::Double(Some(value)))
                    .collect(),
                )],
            ),
            node(
                16,
                127,
                "KNOT_MULT",
                vec![field(
                    "mult",
                    0,
                    vec![
                        FieldValue::ShortInteger(Some(2)),
                        FieldValue::ShortInteger(Some(2)),
                    ],
                )],
            ),
            node(
                17,
                127,
                "KNOT_MULT",
                vec![field(
                    "mult",
                    0,
                    vec![
                        FieldValue::ShortInteger(Some(2)),
                        FieldValue::ShortInteger(Some(2)),
                    ],
                )],
            ),
            node(
                18,
                128,
                "KNOT_SET",
                vec![field(
                    "knots",
                    0,
                    vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0))],
                )],
            ),
            node(
                19,
                128,
                "KNOT_SET",
                vec![field(
                    "knots",
                    0,
                    vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0))],
                )],
            ),
        ]);

        let mut mapper = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        );
        let curve_kinds = (1..=6)
            .map(|index| mapper.map_curve(index).map(|curve| curve.kind.as_str()))
            .collect::<Result<Vec<_>, _>>();
        assert_eq!(
            curve_kinds,
            Ok(vec![
                "line",
                "circle",
                "ellipse",
                "parabola",
                "hyperbola",
                "trimmed",
            ])
        );
        let surface_kinds = (7..=13)
            .map(|index| {
                mapper
                    .map_surface(index)
                    .map(|surface| surface.kind.as_str())
            })
            .collect::<Result<Vec<_>, _>>();
        assert_eq!(
            surface_kinds,
            Ok(vec![
                "plane", "cylinder", "xt_cone", "sphere", "torus", "offset", "nurbs",
            ])
        );
        // A spun surface revolves a mapped curve; a null end point stays None.
        let mut spun = common_surface_fields(0);
        spun.extend([
            field("profile", 1008, vec![FieldValue::PointerIndex(2)]),
            vector("base", [0.0, 0.0, 0.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            vector("start", [0.0, 0.0, 5.0]),
            field("end", 0, vec![FieldValue::Vector([None, None, None])]),
            double("start_param", 0.5),
            field("end_param", 0, vec![FieldValue::Double(None)]),
            vector("x_axis", [1.0, 0.0, 0.0]),
            field("scale", 0, vec![FieldValue::Double(None)]),
        ]);
        let spun_index = u32::try_from(nodes.len()).unwrap_or(u32::MAX) + 1;
        nodes.push(node(spun_index, 68, "SPUN_SURF", spun));
        // A swept surface extrudes a mapped curve; its scale is internal and null.
        let mut swept = common_surface_fields(0);
        swept.extend([
            field("section", 1008, vec![FieldValue::PointerIndex(2)]),
            vector("sweep", [0.0, 0.0, 1.0]),
            field("scale", 0, vec![FieldValue::Double(None)]),
        ]);
        let swept_index = spun_index + 1;
        nodes.push(node(swept_index, 67, "SWEPT_SURF", swept));
        let mut mapper = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        );
        for index in 1..=6 {
            assert!(mapper.map_curve(index).is_ok());
        }
        let spun = mapper.map_surface(spun_index).map(|surface| surface.kind);
        assert!(matches!(
            spun,
            Ok(SurfaceKind::Spun {
                base: Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                start: Some(Vector3 { z: 5.0, .. }),
                end: None,
                start_parameter: Some(0.5),
                end_parameter: None,
                x_axis: Some(Vector3 { x: 1.0, .. }),
                ..
            })
        ));
        if let Ok(SurfaceKind::Spun { profile, .. }) = spun {
            assert_eq!(
                profile,
                mapper
                    .map_curve(2)
                    .map(|curve| curve.id)
                    .unwrap_or(u32::MAX)
            );
        }
        let swept = mapper.map_surface(swept_index).map(|surface| surface.kind);
        assert!(matches!(
            swept,
            Ok(SurfaceKind::Swept {
                direction: Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0
                },
                ..
            })
        ));
        if let Ok(SurfaceKind::Swept { section, .. }) = swept {
            assert_eq!(
                section,
                mapper
                    .map_curve(2)
                    .map(|curve| curve.id)
                    .unwrap_or(u32::MAX)
            );
        }
        let nurbs = mapper.map_surface(13);
        assert!(matches!(
            nurbs.map(|surface| surface.kind),
            Ok(SurfaceKind::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_control_vertex_count: 2,
                v_control_vertex_count: 2,
                rational: false,
                ..
            }))
        ));
    }

    #[test]
    fn cone_kind_uses_only_exact_reviewed_text_profiles() {
        let vector = |name: &str, values: [f64; 3]| {
            field(name, 0, vec![FieldValue::Vector(values.map(Some))])
        };
        let double = |name: &str, value: f64| field(name, 0, vec![FieldValue::Double(Some(value))]);
        let mut cone = common_surface_fields(0);
        cone.extend([
            vector("pvec", [1.0, 2.0, 3.0]),
            vector("axis", [0.0, 0.0, 1.0]),
            double("radius", 2.0),
            double("sin_half_angle", 0.6),
            double("cos_half_angle", 0.8),
            vector("x_axis", [1.0, 0.0, 0.0]),
        ]);
        let nodes = [node(1, 52, "CONE", cone)];
        for (source_format, schema_key, roles, expected_cone) in [
            (
                BrepSourceFormat::Text,
                "SCH_3600188_36001_13006",
                RoleAccess::Sch3600188,
                true,
            ),
            (
                BrepSourceFormat::Text,
                "SCH_3700192_36001_13006",
                RoleAccess::TopsolidCatalog,
                true,
            ),
            (
                BrepSourceFormat::Text,
                "SCH_3700192_36001_13006",
                RoleAccess::Named,
                false,
            ),
            (
                BrepSourceFormat::Binary,
                "SCH_3700192_36001_13006",
                RoleAccess::TopsolidCatalog,
                false,
            ),
            (
                BrepSourceFormat::Text,
                "SCH_OTHER",
                RoleAccess::Sch3600188,
                false,
            ),
        ] {
            let surface = Mapper::new(source_format, schema_key, &nodes, roles)
                .map_surface(1)
                .expect("map cone");
            assert_eq!(
                matches!(surface.kind, SurfaceKind::Cone { .. }),
                expected_cone
            );
        }
    }

    #[test]
    fn trims_open_nurbs_only_with_parameters_in_active_range() {
        fn nodes(basis_sense: u8, periodic: bool, parameters: [f64; 2]) -> Vec<RawNode> {
            let mut basis = common_curve_fields(0);
            basis[4] = field("sense", 0, vec![FieldValue::Character(basis_sense)]);
            basis.push(field("nurbs", 136, vec![FieldValue::PointerIndex(3)]));
            let mut trimmed = common_curve_fields(0);
            trimmed.extend([
                field("basis_curve", 1008, vec![FieldValue::PointerIndex(1)]),
                field(
                    "point_1",
                    0,
                    vec![FieldValue::Vector([Some(0.0), Some(0.0), Some(0.0)])],
                ),
                field(
                    "point_2",
                    0,
                    vec![FieldValue::Vector([Some(1.0), Some(0.0), Some(0.0)])],
                ),
                field("parm_1", 0, vec![FieldValue::Double(Some(parameters[0]))]),
                field("parm_2", 0, vec![FieldValue::Double(Some(parameters[1]))]),
            ]);
            vec![
                node(1, 134, "B_CURVE", basis),
                node(2, 133, "TRIMMED_CURVE", trimmed),
                node(
                    3,
                    136,
                    "NURBS_CURVE",
                    vec![
                        field("degree", 0, vec![FieldValue::ShortInteger(Some(1))]),
                        field("n_vertices", 0, vec![FieldValue::Integer(Some(2))]),
                        field("vertex_dim", 0, vec![FieldValue::ShortInteger(Some(3))]),
                        field("n_knots", 0, vec![FieldValue::Integer(Some(2))]),
                        field("knot_type", 0, vec![FieldValue::UnsignedByte(1)]),
                        field("periodic", 0, vec![FieldValue::Logical(periodic)]),
                        field("closed", 0, vec![FieldValue::Logical(periodic)]),
                        field("rational", 0, vec![FieldValue::Logical(false)]),
                        field("curve_form", 0, vec![FieldValue::UnsignedByte(1)]),
                        field("bspline_vertices", 45, vec![FieldValue::PointerIndex(4)]),
                        field("knot_mult", 127, vec![FieldValue::PointerIndex(5)]),
                        field("knots", 128, vec![FieldValue::PointerIndex(6)]),
                    ],
                ),
                node(
                    4,
                    45,
                    "BSPLINE_VERTICES",
                    vec![field(
                        "vertices",
                        0,
                        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                            .into_iter()
                            .map(|value| FieldValue::Double(Some(value)))
                            .collect(),
                    )],
                ),
                node(
                    5,
                    127,
                    "KNOT_MULT",
                    vec![field(
                        "mult",
                        0,
                        vec![
                            FieldValue::ShortInteger(Some(2)),
                            FieldValue::ShortInteger(Some(2)),
                        ],
                    )],
                ),
                node(
                    6,
                    128,
                    "KNOT_SET",
                    vec![field(
                        "knots",
                        0,
                        vec![FieldValue::Double(Some(0.0)), FieldValue::Double(Some(1.0))],
                    )],
                ),
            ]
        }

        let map_trim = |nodes: &[RawNode]| {
            Mapper::new(BrepSourceFormat::Text, "SCH_TEST", nodes, RoleAccess::Named)
                .map_curve(2)
                .map(|curve| curve.kind)
        };
        assert!(matches!(
            map_trim(&nodes(b'+', false, [0.0, 1.0])),
            Ok(CurveKind::Trimmed {
                start_parameter: 0.0,
                end_parameter: 1.0,
                ..
            })
        ));
        for (sense, parameters) in [(b'+', [-0.1, 0.5]), (b'-', [1.5, -0.1])] {
            assert_eq!(
                map_trim(&nodes(sense, false, parameters))
                    .err()
                    .map(|error| error.kind()),
                Some(ErrorKind::InvalidGeometryParameter)
            );
        }
        assert!(matches!(
            map_trim(&nodes(b'+', true, [0.25, 1.25])),
            Ok(CurveKind::Trimmed {
                start_parameter: 0.25,
                end_parameter: 1.25,
                ..
            })
        ));
    }

    #[test]
    fn trimmed_polyline_requires_known_basis_sense() {
        let mut polyline = common_curve_fields(0);
        polyline[4] = field("sense", 0, vec![FieldValue::Character(b'?')]);
        polyline.extend([
            field("node_id", 0, vec![FieldValue::Integer(Some(77))]),
            field("data", 185, vec![FieldValue::PointerIndex(2)]),
        ]);
        let mut trimmed = common_curve_fields(0);
        trimmed.extend([
            field("basis_curve", 1008, vec![FieldValue::PointerIndex(1)]),
            field("point_1", 0, vec![FieldValue::Vector([Some(0.0); 3])]),
            field(
                "point_2",
                0,
                vec![FieldValue::Vector([Some(2.0), Some(2.0), Some(0.0)])],
            ),
            field("parm_1", 0, vec![FieldValue::Double(Some(17.0))]),
            field("parm_2", 0, vec![FieldValue::Double(Some(19.0))]),
        ]);
        let nodes = vec![
            node(1, 900, "POLYLINE", polyline),
            node(
                2,
                901,
                "POLYLINE_DATA",
                vec![
                    field("n_pvecs", 0, vec![FieldValue::Integer(Some(3))]),
                    field("closed", 0, vec![FieldValue::Logical(false)]),
                    field("base_parm", 0, vec![FieldValue::Double(Some(17.0))]),
                    field("pvec", 85, vec![FieldValue::PointerIndex(3)]),
                ],
            ),
            node(
                3,
                902,
                "POINT_VALUES",
                vec![field(
                    "values",
                    0,
                    [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 2.0, 0.0]]
                        .into_iter()
                        .map(|point| FieldValue::Vector(point.map(Some)))
                        .collect(),
                )],
            ),
            node(4, 133, "TRIMMED_CURVE", trimmed),
        ];
        let error = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map_curve(4)
        .expect_err("unknown-sense basis polyline cannot orient a trim");
        assert_eq!(error.kind(), ErrorKind::InvalidGeometryParameter);
    }

    #[test]
    fn derives_unset_trim_parameters_only_on_a_line() {
        fn vector(name: &str, values: [f64; 3]) -> (FieldDefinition, Vec<FieldValue>) {
            field(name, 0, vec![FieldValue::Vector(values.map(Some))])
        }
        // R(t) = (1, 2, 3) + t (0, 0.6, 0.8) passes (1, 3.5, 5) at t = 2.5 and
        // (1, 6.5, 9) at t = 7.5.
        let low = [1.0, 3.5, 5.0];
        let high = [1.0, 6.5, 9.0];
        let trim = |sense: u8,
                    direction: [f64; 3],
                    points: [[f64; 3]; 2],
                    parameters: [Option<f64>; 2],
                    on_circle: bool| {
            let mut basis = common_curve_fields(0);
            basis[4] = field("sense", 0, vec![FieldValue::Character(sense)]);
            let basis = if on_circle {
                basis.extend([
                    vector("centre", [1.0, 2.0, 3.0]),
                    vector("normal", [1.0, 0.0, 0.0]),
                    vector("x_axis", [0.0, 0.6, 0.8]),
                    field("radius", 0, vec![FieldValue::Double(Some(2.5))]),
                ]);
                node(1, 31, "CIRCLE", basis)
            } else {
                basis.extend([
                    vector("pvec", [1.0, 2.0, 3.0]),
                    vector("direction", direction),
                ]);
                node(1, 30, "LINE", basis)
            };
            let mut trimmed = common_curve_fields(0);
            trimmed.extend([
                field("basis_curve", 1008, vec![FieldValue::PointerIndex(1)]),
                vector("point_1", points[0]),
                vector("point_2", points[1]),
                field("parm_1", 0, vec![FieldValue::Double(parameters[0])]),
                field("parm_2", 0, vec![FieldValue::Double(parameters[1])]),
            ]);
            let nodes = [basis, node(2, 133, "TRIMMED_CURVE", trimmed)];
            Mapper::new(
                BrepSourceFormat::Text,
                "SCH_TEST",
                &nodes,
                RoleAccess::Named,
            )
            .map_curve(2)
            .map(|curve| curve.kind)
        };
        let unit = [0.0, 0.6, 0.8];
        for (sense, points, expected) in [
            (b'+', [low, high], [2.5, 7.5]),
            (b'-', [high, low], [7.5, 2.5]),
        ] {
            let kind = trim(sense, unit, points, [None, None], false);
            assert!(matches!(
                kind,
                Ok(CurveKind::Trimmed {
                    start_parameter,
                    end_parameter,
                    ..
                }) if (start_parameter - expected[0]).abs() < 1.0e-12
                    && (end_parameter - expected[1]).abs() < 1.0e-12
            ));
        }
        // Stored parameters are never replaced.
        assert!(matches!(
            trim(b'+', unit, [low, high], [Some(-4.0), Some(9.0)], false),
            Ok(CurveKind::Trimmed {
                start_parameter: -4.0,
                end_parameter: 9.0,
                ..
            })
        ));
        let off_line = [1.000_001, 6.5, 9.0];
        for kind in [
            // The derived order must still follow the basis sense.
            trim(b'+', unit, [high, low], [None, None], false),
            trim(b'-', unit, [low, high], [None, None], false),
            trim(b'+', unit, [low, high], [Some(2.5), None], false),
            trim(b'+', unit, [low, high], [None, Some(7.5)], false),
            trim(b'+', unit, [low, high], [None, None], true),
            trim(b'+', unit, [low, off_line], [None, None], false),
            trim(b'+', [0.0, 0.6, 0.9], [low, high], [None, None], false),
        ] {
            assert_eq!(
                kind.err().map(|error| error.kind()),
                Some(ErrorKind::InvalidGeometryParameter)
            );
        }
    }

    #[test]
    fn retains_an_unsupported_curve_and_marks_the_model_incomplete() {
        let mut nodes = wire_acorn_nodes();
        nodes.push(node(6, 200, "FUTURE_CURVE", common_curve_fields(4)));

        let model = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map();
        assert!(model.is_ok());
        if let Ok(model) = model {
            assert!(!model.complete);
            assert_eq!(model.diagnostics.len(), 1);
            assert_eq!(model.diagnostics[0].code, "geometry.unsupported_curve");
            assert!(matches!(
                model.curves[0].kind,
                CurveKind::Unsupported { .. }
            ));
            assert_eq!(model.curves[0].source.node_index, 6);
        }
    }

    #[test]
    fn attribute_recovery_is_opt_in_and_discards_all_attributes_without_losing_geometry() {
        let nodes = nodes_with_valid_then_broken_attributes();
        let strict = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map();
        assert_eq!(
            strict.err().map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepReference)
        );

        let mut valid_nodes = nodes.clone();
        valid_nodes.retain(|node| node.index != 10);
        let valid = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &valid_nodes,
            RoleAccess::Named,
        )
        .map_with_metadata_recovery()
        .expect("valid semantic attributes remain mapped");
        assert_eq!(valid.attributes.len(), 1);
        assert_eq!(valid.bodies[0].attributes, [0]);

        let geometry_nodes = wire_acorn_nodes();
        let geometry = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &geometry_nodes,
            RoleAccess::Named,
        )
        .map()
        .expect("geometry-only fixture maps");
        let recovered = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map_with_metadata_recovery()
        .expect("attribute errors are recoverable");

        assert_eq!(recovered.bodies, geometry.bodies);
        assert_eq!(recovered.regions, geometry.regions);
        assert_eq!(recovered.shells, geometry.shells);
        assert_eq!(recovered.faces, geometry.faces);
        assert_eq!(recovered.vertices, geometry.vertices);
        assert_eq!(recovered.points, geometry.points);
        assert_eq!(recovered.topology, geometry.topology);
        assert_eq!(recovered.complete, geometry.complete);
        assert!(recovered.attributes.is_empty());
        assert!(
            recovered
                .bodies
                .iter()
                .all(|body| body.attributes.is_empty())
        );
        assert_eq!(recovered.diagnostics.len(), 1);
        assert_eq!(
            recovered.diagnostics[0].code,
            "metadata.attribute_decode_failed"
        );
        assert_eq!(recovered.diagnostics[0].source.node_index, 10);
        assert!(recovered.diagnostics[0].message.contains("unresolved"));
    }

    #[test]
    fn attribute_recovery_uses_the_shared_diagnostic_limit() {
        let mut nodes = nodes_with_valid_then_broken_attributes();
        nodes.push(node(11, 200, "FUTURE_CURVE", common_curve_fields(1)));
        let error = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .with_max_diagnostics(1)
        .map_with_metadata_recovery()
        .expect_err("geometry and metadata diagnostics share the cap");

        assert_eq!(error.kind(), ErrorKind::LimitExceeded);
        assert_eq!(
            error.details(),
            &ErrorDetails::LimitExceeded {
                resource: "diagnostics",
                actual: 2,
                limit: 1,
            }
        );
    }

    #[test]
    fn metadata_recovery_discards_broken_assembly_group_and_preserves_owner_integrity() {
        let valid_nodes = nodes_with_assembly_owned_attributes(None);
        let valid = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &valid_nodes,
            RoleAccess::Named,
        )
        .map_with_metadata_recovery()
        .expect("valid assembly metadata maps");
        assert_eq!(valid.assemblies.len(), 2);
        assert_eq!(valid.instances.len(), 2);
        assert_eq!(valid.attributes[0].owner, AttributeOwner::Assembly(0));
        assert_eq!(valid.attributes[1].owner, AttributeOwner::Instance(0));

        let geometry_nodes = wire_acorn_nodes();
        let geometry = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &geometry_nodes,
            RoleAccess::Named,
        )
        .map()
        .expect("geometry-only fixture maps");

        for failing_node in [6, 10, 7] {
            let nodes = nodes_with_assembly_owned_attributes(Some(failing_node));
            let strict = Mapper::new(
                BrepSourceFormat::Binary,
                "SCH_TEST",
                &nodes,
                RoleAccess::Named,
            )
            .map();
            assert_eq!(
                strict.err().map(|error| error.kind()),
                Some(ErrorKind::InvalidBrepField)
            );

            let recovered = Mapper::new(
                BrepSourceFormat::Binary,
                "SCH_TEST",
                &nodes,
                RoleAccess::Named,
            )
            .map_with_metadata_recovery()
            .expect("assembly metadata errors are recoverable");
            assert!(recovered.assemblies.is_empty());
            assert!(recovered.instances.is_empty());
            assert_eq!(recovered.bodies, geometry.bodies);
            assert_eq!(recovered.regions, geometry.regions);
            assert_eq!(recovered.shells, geometry.shells);
            assert_eq!(recovered.vertices, geometry.vertices);
            assert_eq!(recovered.points, geometry.points);
            assert_eq!(recovered.topology, geometry.topology);
            assert_eq!(recovered.complete, geometry.complete);
            assert_eq!(recovered.attributes.len(), 2);
            assert!(matches!(
                &recovered.attributes[0].owner,
                AttributeOwner::Other(source) if source.node_index == 6
            ));
            assert!(matches!(
                &recovered.attributes[1].owner,
                AttributeOwner::Other(source) if source.node_index == 7
            ));
            assert_eq!(recovered.diagnostics.len(), 1);
            assert_eq!(
                recovered.diagnostics[0].code,
                "metadata.assembly_decode_failed"
            );
            assert_eq!(recovered.diagnostics[0].source.node_index, failing_node);
        }
    }

    #[test]
    fn assembly_recovery_diagnostic_uses_the_shared_limit() {
        let mut nodes = nodes_with_assembly_owned_attributes(Some(6));
        nodes.push(node(30, 200, "FUTURE_CURVE", common_curve_fields(1)));
        let error = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .with_max_diagnostics(1)
        .map_with_metadata_recovery()
        .expect_err("geometry and assembly diagnostics share the cap");

        assert_eq!(error.kind(), ErrorKind::LimitExceeded);
        assert_eq!(
            error.details(),
            &ErrorDetails::LimitExceeded {
                resource: "diagnostics",
                actual: 2,
                limit: 1,
            }
        );
    }

    #[test]
    fn invalid_topology_precedes_metadata_diagnostic_limit() {
        let mut nodes = sheet_nodes(b'S');
        let mut assemblies = assembly_nodes(1, 1, 8, 2.0);
        let root = assemblies
            .iter_mut()
            .find(|node| node.index == 6)
            .expect("root assembly exists");
        root.fields
            .iter_mut()
            .find(|field| field.definition.name == "type")
            .expect("root assembly type exists")
            .values = vec![FieldValue::UnsignedByte(99)];
        nodes.extend(assemblies);
        nodes.push(node(30, 200, "FUTURE_CURVE", common_curve_fields(4)));

        let error = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .with_max_diagnostics(1)
        .map_with_metadata_recovery()
        .expect_err("invalid topology takes precedence over metadata diagnostics");

        assert_eq!(error.kind(), ErrorKind::InvalidBrepTopology);
    }

    #[test]
    fn maps_missing_optional_references_to_null_with_provenance() {
        for source_format in [BrepSourceFormat::Binary, BrepSourceFormat::Text] {
            let mut nodes = wire_acorn_nodes();
            nodes[2]
                .fields
                .iter_mut()
                .find(|field| field.definition.name == "vertex")
                .expect("shell vertex field")
                .values = vec![FieldValue::PointerIndex(99)];
            nodes[4]
                .fields
                .iter_mut()
                .find(|field| field.definition.name == "owner")
                .expect("point owner field")
                .values = vec![FieldValue::PointerIndex(98)];

            let model = Mapper::new(source_format, "SCH_TEST", &nodes, RoleAccess::Named)
                .map()
                .expect("map document with missing optional references");

            assert_eq!(nodes[2].fields[4].values, [FieldValue::PointerIndex(99)]);
            assert_eq!(nodes[4].fields[1].values, [FieldValue::PointerIndex(98)]);
            assert!(model.topology.valid);
            assert!(!model.complete);
            assert_eq!(model.shells[0].isolated_vertex, None);
            assert_eq!(model.points[0].owner, None);
            assert_eq!(model.diagnostics.len(), 2);

            let shell_diagnostic = model
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.source.node_index == 3)
                .expect("shell reference diagnostic");
            assert_eq!(shell_diagnostic.code, "reference.unresolved_optional_index");
            assert_eq!(shell_diagnostic.role, "optional_reference");
            assert_eq!(shell_diagnostic.source.type_name, "SHELL");
            assert!(shell_diagnostic.message.contains("field vertex"));
            assert!(shell_diagnostic.message.contains("index 99"));

            let point_diagnostic = model
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.source.node_index == 5)
                .expect("point reference diagnostic");
            assert!(point_diagnostic.message.contains("field owner"));
            assert!(point_diagnostic.message.contains("index 98"));
        }
    }

    #[test]
    fn maps_missing_optional_pair_item_to_null_with_provenance() {
        for source_format in [BrepSourceFormat::Binary, BrepSourceFormat::Text] {
            let mut blend_fields = common_surface_fields(0);
            blend_fields.extend([
                field("blend_type", 0, vec![FieldValue::Character(b'R')]),
                field(
                    "surface",
                    1006,
                    vec![FieldValue::PointerIndex(1), FieldValue::PointerIndex(2)],
                ),
                field("spine", 1008, vec![FieldValue::PointerIndex(3)]),
                field(
                    "range",
                    0,
                    vec![
                        FieldValue::Double(Some(0.005)),
                        FieldValue::Double(Some(-0.005)),
                    ],
                ),
                field(
                    "thumb_weight",
                    0,
                    vec![FieldValue::Double(Some(1.0)), FieldValue::Double(Some(1.0))],
                ),
                field(
                    "boundary",
                    1006,
                    vec![FieldValue::PointerIndex(99), FieldValue::PointerIndex(0)],
                ),
                field("start", 41, vec![FieldValue::PointerIndex(0)]),
                field("end", 41, vec![FieldValue::PointerIndex(0)]),
            ]);
            let nodes = [
                node(1, 50, "PLANE", common_surface_fields(4)),
                node(2, 50, "PLANE", common_surface_fields(4)),
                node(3, 30, "LINE", common_curve_fields(4)),
                node(4, 56, "BLENDED_EDGE", blend_fields),
            ];
            let mut mapper = Mapper::new(source_format, "SCH_TEST", &nodes, RoleAccess::Named);

            let mapped = mapper.map_surface(4).expect("map blended surface");
            assert!(matches!(
                mapped.kind,
                SurfaceKind::BlendedEdge {
                    boundary_surfaces: [None, None],
                    ..
                }
            ));
            let diagnostics = mapper.diagnostics.borrow();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].source.node_index, 4);
            assert_eq!(diagnostics[0].source.type_name, "BLENDED_EDGE");
            assert_eq!(diagnostics[0].code, "reference.unresolved_optional_index");
            assert!(diagnostics[0].message.contains("field boundary[0]"));
            assert!(diagnostics[0].message.contains("index 99"));
        }
    }

    #[test]
    fn unresolved_optional_reference_to_wrong_type_still_errors() {
        let mut nodes = wire_acorn_nodes();
        nodes[2]
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "vertex")
            .expect("shell vertex field")
            .values = vec![FieldValue::PointerIndex(2)];

        let error = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map()
        .err();
        assert_eq!(
            error.as_ref().map(ParseError::kind),
            Some(ErrorKind::InvalidBrepReference)
        );
        assert_eq!(
            error.as_ref().map(ParseError::details),
            Some(&ErrorDetails::BrepReference {
                node_index: 3,
                field: "vertex",
                target_index: 2,
                expected_type: "VERTEX",
            })
        );
    }

    #[test]
    fn bounds_retained_geometry_diagnostics() {
        let mut nodes = wire_acorn_nodes();
        nodes.push(node(6, 200, "FUTURE_CURVE", common_curve_fields(4)));
        nodes.push(node(7, 201, "ANOTHER_FUTURE_CURVE", common_curve_fields(4)));

        let result = Mapper::new(
            BrepSourceFormat::Text,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .with_max_diagnostics(1)
        .map();

        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.kind(), ErrorKind::LimitExceeded);
            assert_eq!(error.offset(), 7);
            assert_eq!(
                error.details(),
                &ErrorDetails::LimitExceeded {
                    resource: "diagnostics",
                    actual: 2,
                    limit: 1,
                }
            );
        }
    }

    #[test]
    fn retains_unknown_sense_used_by_an_isolated_fin() {
        let nodes = [node(
            1,
            16,
            "HALFEDGE",
            vec![field("sense", 0, vec![FieldValue::Character(b'?')])],
        )];
        let mapper = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        );

        assert_eq!(mapper.sense(&nodes[0], "sense"), Ok(Sense::Unknown));
        assert_eq!(Sense::Unknown.multiplier(), None);
    }

    #[test]
    fn maps_exact_blended_edge_and_boundary_surface_definitions() {
        let mut blend_fields = common_surface_fields(0);
        blend_fields.extend([
            field("blend_type", 0, vec![FieldValue::Character(b'R')]),
            field(
                "surface",
                1006,
                vec![FieldValue::PointerIndex(1), FieldValue::PointerIndex(2)],
            ),
            field("spine", 1008, vec![FieldValue::PointerIndex(3)]),
            field(
                "range",
                0,
                vec![
                    FieldValue::Double(Some(0.005)),
                    FieldValue::Double(Some(-0.005)),
                ],
            ),
            field(
                "thumb_weight",
                0,
                vec![FieldValue::Double(Some(1.0)), FieldValue::Double(Some(1.0))],
            ),
            field(
                "boundary",
                1006,
                vec![FieldValue::PointerIndex(0), FieldValue::PointerIndex(0)],
            ),
            field("start", 41, vec![FieldValue::PointerIndex(0)]),
            field("end", 41, vec![FieldValue::PointerIndex(0)]),
        ]);
        let mut boundary_fields = common_surface_fields(0);
        boundary_fields.extend([
            field("boundary", 0, vec![FieldValue::ShortInteger(Some(1))]),
            field("blend", 1006, vec![FieldValue::PointerIndex(4)]),
        ]);
        let nodes = [
            node(1, 50, "PLANE", common_surface_fields(4)),
            node(2, 50, "PLANE", common_surface_fields(4)),
            node(3, 30, "LINE", common_curve_fields(4)),
            node(4, 56, "BLENDED_EDGE", blend_fields),
            node(5, 59, "BLEND_BOUND", boundary_fields),
        ];
        let mut mapper = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        );

        let blend = mapper.map_surface(4);
        assert!(blend.is_ok());
        if let Ok(blend) = blend {
            assert!(matches!(
                blend.kind,
                SurfaceKind::BlendedEdge {
                    blend_type: BlendType::RollingBall,
                    supporting_surfaces: [0, 1],
                    spine_curve: 0,
                    ranges: [0.005, -0.005],
                    boundary_surfaces: [None, None],
                    ..
                }
            ));
        }
        let boundary = mapper.map_surface(5);
        assert!(boundary.is_ok());
        if let Ok(boundary) = boundary {
            assert_eq!(
                boundary.kind,
                SurfaceKind::BlendBoundary {
                    boundary_index: 1,
                    blend_surface: 2,
                }
            );
        }
    }

    #[test]
    fn rejects_an_unresolved_required_body_chain_pointer() {
        let mut nodes = wire_acorn_nodes();
        let body = &mut nodes[0];
        let region = body
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "region");
        assert!(region.is_some());
        if let Some(region) = region {
            region.values = vec![FieldValue::PointerIndex(99)];
        }

        let error = Mapper::new(
            BrepSourceFormat::Binary,
            "SCH_TEST",
            &nodes,
            RoleAccess::Named,
        )
        .map()
        .err();
        assert_eq!(
            error.as_ref().map(ParseError::kind),
            Some(ErrorKind::InvalidBrepReference)
        );
        assert_eq!(
            error.as_ref().map(ParseError::details),
            Some(&ErrorDetails::BrepReference {
                node_index: 1,
                field: "region",
                target_index: 99,
                expected_type: "REGION",
            })
        );
    }

    mod mesh_surface_tests {
        use super::*;
        use crate::SchemaProvider;
        use crate::brep::MeshIndexOrigin;

        const KEY: &str = "SCH_3300259_32001_13006";

        fn profile_type(key: &crate::SchemaKey, name: &str) -> Option<TypeDefinition> {
            let provider = crate::BuiltinProfileRegistry::compiled()
                .unwrap()
                .provider_for_key(key)?;
            (0..=u16::MAX).find_map(|node_type| {
                provider
                    .type_definition(key.effective(), node_type)
                    .filter(|definition| definition.name == name)
                    .cloned()
            })
        }

        fn exact_node(
            definition: TypeDefinition,
            index: u32,
            overrides: &[(&str, Vec<FieldValue>)],
        ) -> RawNode {
            let fields = definition
                .fields
                .iter()
                .map(|field| {
                    overrides
                        .iter()
                        .find(|(name, _)| *name == field.name)
                        .map_or_else(
                            || vec![builtin_default(field.field_type)],
                            |(_, values)| values.clone(),
                        )
                })
                .collect::<Vec<_>>();
            let variable_length = definition
                .variable
                .then(|| u32::try_from(fields.last().map_or(0, Vec::len)).unwrap());
            RawNode {
                node_type: definition.node_type,
                index,
                variable_length,
                fields: definition
                    .fields
                    .iter()
                    .cloned()
                    .zip(fields)
                    .map(|(definition, values)| RawField {
                        definition: definition.into(),
                        values,
                        byte_range: 0..0,
                    })
                    .collect(),
                definition: definition.into(),
                first_schema: None,
                byte_range: 0..0,
                user_fields: Vec::new(),
            }
        }

        fn builtin_default(field_type: FieldType) -> FieldValue {
            match field_type {
                FieldType::UnsignedByte => FieldValue::UnsignedByte(0),
                FieldType::Character => FieldValue::Character(b'+'),
                FieldType::Logical => FieldValue::Logical(false),
                FieldType::ShortInteger => FieldValue::ShortInteger(Some(0)),
                FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(0),
                FieldType::Integer => FieldValue::Integer(Some(0)),
                FieldType::PointerIndex => FieldValue::PointerIndex(0),
                FieldType::OpaquePointer => unreachable!(),
                FieldType::Tag => FieldValue::Tag(Some(0)),
                FieldType::Double => FieldValue::Double(Some(0.0)),
                FieldType::Interval => FieldValue::Interval([Some(0.0); 2]),
                FieldType::Vector => FieldValue::Vector([Some(0.0); 3]),
                FieldType::Box3 => FieldValue::Box3([Some(0.0); 6]),
                FieldType::IntersectionPoint => FieldValue::IntersectionPoint([Some(0.0); 3]),
            }
        }

        fn exact_field_values(
            name: &'static str,
            values: Vec<FieldValue>,
        ) -> (&'static str, Vec<FieldValue>) {
            (name, values)
        }

        fn mesh_and_face_nodes() -> Vec<RawNode> {
            let key = crate::SchemaKey::parse(KEY).unwrap();
            let mesh = profile_type(&key, "MESH").unwrap();
            let psm = profile_type(&key, "PSM_MESH").unwrap();
            let mut nodes = vec![
                exact_node(
                    mesh,
                    1,
                    &[
                        exact_field_values("owner", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("sense", vec![FieldValue::Character(b'+')]),
                        exact_field_values("transform", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("psm_imesh", vec![FieldValue::PointerIndex(2)]),
                    ],
                ),
                exact_node(profile_type(&key, "SHELL").unwrap(), 8, &[]),
                exact_node(
                    profile_type(&key, "FACE").unwrap(),
                    9,
                    &[
                        exact_field_values("next", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("previous", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("loop", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("shell", vec![FieldValue::PointerIndex(8)]),
                        exact_field_values("surface", vec![FieldValue::PointerIndex(1)]),
                        exact_field_values("sense", vec![FieldValue::Character(b'-')]),
                        exact_field_values("next_on_surface", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values(
                            "previous_on_surface",
                            vec![FieldValue::PointerIndex(0)],
                        ),
                        exact_field_values("next_front", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("previous_front", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("front_shell", vec![FieldValue::PointerIndex(8)]),
                    ],
                ),
            ];
            nodes.extend([
                exact_node(
                    psm,
                    2,
                    &[
                        exact_field_values("precision", vec![FieldValue::UnsignedByte(1)]),
                        exact_field_values("owner", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("position_pool", vec![FieldValue::PointerIndex(3)]),
                        exact_field_values("normal_pool", vec![FieldValue::PointerIndex(0)]),
                        exact_field_values("position_indices", vec![FieldValue::PointerIndex(5)]),
                        exact_field_values("normal_type", vec![FieldValue::UnsignedByte(1)]),
                        exact_field_values("normal_indices", vec![FieldValue::PointerIndex(0)]),
                    ],
                ),
                exact_node(
                    profile_type(&key, "VECTOR_COMB").unwrap(),
                    3,
                    &[
                        exact_field_values("encoding", vec![FieldValue::UnsignedByte(1)]),
                        exact_field_values("n_vectors", vec![FieldValue::Integer(Some(3))]),
                        exact_field_values("n_max_vectors", vec![FieldValue::Integer(Some(4))]),
                        exact_field_values("shift", vec![FieldValue::Integer(Some(2))]),
                        exact_field_values("teeth", vec![FieldValue::PointerIndex(4)]),
                    ],
                ),
                exact_node(
                    profile_type(&key, "VECTOR_TOOTH").unwrap(),
                    4,
                    &[exact_field_values(
                        "values",
                        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 9.0, 9.0, 9.0]
                            .into_iter()
                            .map(|value| FieldValue::Double(Some(value)))
                            .collect(),
                    )],
                ),
                exact_node(
                    profile_type(&key, "INTEGER_COMB").unwrap(),
                    5,
                    &[
                        exact_field_values("encoding", vec![FieldValue::UnsignedByte(1)]),
                        exact_field_values("n_integers", vec![FieldValue::Integer(Some(3))]),
                        exact_field_values("n_max_integers", vec![FieldValue::Integer(Some(4))]),
                        exact_field_values(
                            "n_bits_per_integer",
                            vec![FieldValue::Integer(Some(32))],
                        ),
                        exact_field_values("shift", vec![FieldValue::Integer(Some(2))]),
                        exact_field_values("teeth", vec![FieldValue::PointerIndex(6)]),
                    ],
                ),
                exact_node(
                    profile_type(&key, "INTEGER_TOOTH").unwrap(),
                    6,
                    &[exact_field_values(
                        "values",
                        [0, 1, 2, 99]
                            .into_iter()
                            .map(|value| FieldValue::Integer(Some(value)))
                            .collect(),
                    )],
                ),
            ]);
            nodes
        }

        fn mesh_model(face: Face, surfaces: Vec<SurfaceGeometry>) -> BrepModel {
            BrepModel {
                source_format: BrepSourceFormat::Text,
                schema_key: KEY.to_owned(),
                complete: true,
                bodies: vec![],
                regions: vec![],
                shells: vec![],
                faces: vec![face],
                loops: vec![],
                half_edges: vec![],
                edges: vec![],
                vertices: vec![],
                points: vec![],
                curves: vec![],
                surfaces,
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
                assemblies: vec![],
                instances: vec![],
                attributes: vec![],
                diagnostics: vec![],
            }
        }

        #[test]
        #[allow(
            clippy::float_cmp,
            clippy::panic,
            reason = "The fixture checks exact transforms and requires an embedded mesh."
        )]
        fn maps_embedded_mesh_as_face_surface_without_guessing_index_origin() {
            let mut nodes = mesh_and_face_nodes();
            nodes[0]
                .fields
                .iter_mut()
                .find(|field| field.definition.name == "transform")
                .unwrap()
                .values = vec![FieldValue::PointerIndex(7)];
            nodes.push(exact_node(
                profile_type(&crate::SchemaKey::parse(KEY).unwrap(), "TRANSFORM").unwrap(),
                7,
                &[
                    exact_field_values(
                        "rotation_matrix",
                        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                            .into_iter()
                            .map(|value| FieldValue::Double(Some(value)))
                            .collect(),
                    ),
                    exact_field_values(
                        "translation_vector",
                        vec![FieldValue::Vector([Some(10.0), Some(0.0), Some(0.0)])],
                    ),
                    exact_field_values("scale", vec![FieldValue::Double(Some(2.0))]),
                    exact_field_values("flag", vec![FieldValue::Integer(Some(0))]),
                    exact_field_values("perspective_vector", vec![FieldValue::Vector([None; 3])]),
                ],
            ));
            let mut mapper = Mapper::new(
                BrepSourceFormat::Text,
                KEY,
                &nodes,
                RoleAccess::TopsolidCatalog,
            );
            let face = mapper.map_face(9).unwrap();
            let surface = mapper.map_surface(1).unwrap();
            assert_eq!(face.surface, Some(surface.id));
            assert_eq!(face.sense, Sense::Negative);
            assert_eq!(surface.sense, Sense::Positive);
            assert!(matches!(
                &surface.kind,
                SurfaceKind::Mesh {
                    transform_index: Some(7),
                    transform: Some(transform),
                    ..
                } if transform.index == 7 && transform.translation == [10.0, 0.0, 0.0]
            ));
            let triangles = surface
                .kind
                .mesh_triangles_with_index_origin(MeshIndexOrigin::ZeroBased, face.sense)
                .unwrap()
                .unwrap();
            assert_eq!(
                triangles[0],
                [[20.0, 0.0, 0.0], [20.0, 2.0, 0.0], [22.0, 0.0, 0.0]]
            );
            let brep = mesh_model(face.clone(), vec![surface.clone()]);
            assert_eq!(
                brep.mesh_triangles_for_face_with_index_origin(face.id, MeshIndexOrigin::ZeroBased)
                    .unwrap()
                    .unwrap(),
                triangles
            );
            assert_eq!(
                brep.mesh_triangles_for_face_with_inferred_index_origin(face.id)
                    .unwrap()
                    .unwrap(),
                triangles
            );

            let mut ambiguous_brep = brep.clone();
            let SurfaceKind::Mesh {
                embedded: Some(mesh),
                ..
            } = &mut ambiguous_brep.surfaces[0].kind
            else {
                panic!("expected embedded mesh surface");
            };
            mesh.position_indices.values[..3].copy_from_slice(&[1, 2, 2]);
            assert!(matches!(
                ambiguous_brep.mesh_triangles_for_face_with_inferred_index_origin(face.id),
                Err(crate::brep::MeshDataError::Invalid {
                    field: "position_indices",
                    reason: "index origin is ambiguous",
                    ..
                })
            ));
            assert!(
                brep.mesh_triangles_for_face_with_index_origin(999, MeshIndexOrigin::ZeroBased)
                    .unwrap()
                    .is_none()
            );

            let mut face_without_surface = face.clone();
            face_without_surface.surface = None;
            assert!(
                mesh_model(face_without_surface, vec![surface.clone()])
                    .mesh_triangles_for_face_with_index_origin(face.id, MeshIndexOrigin::ZeroBased)
                    .unwrap()
                    .is_none()
            );
            let mut non_mesh_surface = surface.clone();
            non_mesh_surface.kind = SurfaceKind::Unsupported {
                type_name: "MESH".to_owned(),
            };
            assert!(
                mesh_model(face.clone(), vec![non_mesh_surface])
                    .mesh_triangles_for_face_with_index_origin(face.id, MeshIndexOrigin::ZeroBased)
                    .unwrap()
                    .is_none()
            );

            let mut duplicate_face = mesh_model(face.clone(), vec![surface.clone()]);
            duplicate_face.faces.push(face.clone());
            assert!(matches!(
                duplicate_face
                    .mesh_triangles_for_face_with_index_origin(face.id, MeshIndexOrigin::ZeroBased),
                Err(crate::brep::MeshDataError::Invalid {
                    field: "face_id",
                    reason: "face ID resolves to multiple mapped faces",
                    ..
                })
            ));
            let mut duplicate_surface = mesh_model(face.clone(), vec![surface.clone()]);
            duplicate_surface.surfaces.push(surface);
            assert!(matches!(
                duplicate_surface
                    .mesh_triangles_for_face_with_index_origin(face.id, MeshIndexOrigin::ZeroBased),
                Err(crate::brep::MeshDataError::Invalid {
                    field: "surface_id",
                    reason: "surface ID resolves to multiple mapped surfaces",
                    ..
                })
            ));
        }
    }
}
