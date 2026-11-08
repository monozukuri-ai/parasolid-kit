use super::evaluate::evaluate_prepared_nurbs_surface;
use super::nurbs_surface::PreparedNurbsSurface;
use super::polyline_data::PolylineData;
use super::{
    BrepModel, BrepModelWithResolvedBodyHierarchy, BrepSourceFormat, CurveEvaluation,
    CurveGeometry, CurveKind, Sense, SurfaceEvaluation, SurfaceGeometry, SurfaceKind,
    SweptSurfaceData, Vector3, evaluate_curve, evaluate_curve_point,
    evaluate_mapped_nurbs_curve_candidate_2d, evaluate_mapped_xt_swept_point_candidate,
    evaluate_rolling_ball, evaluate_ruled_sweep_point_candidate,
    evaluate_surface as evaluate_surface_kind,
    evaluate_surface_point as evaluate_surface_point_kind, triangulate_polygon_2d,
};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::{HashMap, HashSet};

#[inline]
fn vec3_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
fn vec3_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn vec3_scale(a: [f64; 3], scale: f64) -> [f64; 3] {
    [a[0] * scale, a[1] * scale, a[2] * scale]
}

#[inline]
fn vec3_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn vec3_cross_roundoff_bound_squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    let factor = 256.0 * f64::EPSILON;
    [
        (a[1] * b[2]).abs() + (a[2] * b[1]).abs(),
        (a[2] * b[0]).abs() + (a[0] * b[2]).abs(),
        (a[0] * b[1]).abs() + (a[1] * b[0]).abs(),
    ]
    .into_iter()
    .map(|magnitude| (factor * magnitude).powi(2))
    .sum()
}

#[inline]
fn vec3_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn vec3_norm(a: [f64; 3]) -> f64 {
    vec3_dot(a, a).sqrt()
}

#[inline]
fn vec3_normalize(a: [f64; 3]) -> [f64; 3] {
    let length = vec3_norm(a);
    if length > 1e-12 {
        vec3_scale(a, 1.0 / length)
    } else {
        a
    }
}

#[inline]
fn signed_area_2d(polygon: &[[f64; 2]]) -> f64 {
    if polygon.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for index in 0..polygon.len() {
        let next = (index + 1) % polygon.len();
        area += polygon[index][0] * polygon[next][1] - polygon[next][0] * polygon[index][1];
    }
    area * 0.5
}

fn orthonormal_frame_from_axis_and_ref(
    axis: [f64; 3],
    x_ref: [f64; 3],
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let z = if vec3_norm(axis) > 1e-12 {
        vec3_normalize(axis)
    } else {
        [0.0, 0.0, 1.0]
    };
    let x_proj = vec3_sub(x_ref, vec3_scale(z, vec3_dot(x_ref, z)));
    let x = if vec3_norm(x_proj) > 1e-12 {
        vec3_normalize(x_proj)
    } else {
        let fallback = if z[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        vec3_normalize(vec3_sub(fallback, vec3_scale(z, vec3_dot(fallback, z))))
    };
    let y = vec3_normalize(vec3_cross(z, x));
    (x, y, z)
}

/// Polygonal mesh generated from B-Rep tessellation.
#[derive(Clone, Debug, Default)]
pub struct TessellatedMesh {
    /// Triangles as `[[p0, p1, p2]; 3]`.
    pub triangles: Vec<[[f64; 3]; 3]>,
    /// Per-vertex outward normals aligned with triangles: `[[n0, n1, n2]; 3]`.
    pub normals: Vec<[[f64; 3]; 3]>,
    /// Sharp/feature wire edges as pairs `[[p0, p1]; 2]`.
    pub wire_edges: Vec<[[f64; 3]; 2]>,
}

fn unique_entity_map<T>(items: &[T], id: impl Fn(&T) -> u32) -> Option<HashMap<u32, &T>> {
    let mut indexed = HashMap::with_capacity(items.len());
    for item in items {
        if indexed.insert(id(item), item).is_some() {
            return None;
        }
    }
    Some(indexed)
}

fn append_intersection_samples(
    ring: &mut Vec<[f64; 3]>,
    samples: impl IntoIterator<Item = [f64; 3]>,
) {
    for point in samples {
        if ring.last() != Some(&point) {
            ring.push(point);
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "Exact IEEE equality identifies a duplicate ring-closing vertex without merging nearby geometry."
)]
fn remove_ring_closure_duplicate(ring: &mut Vec<[f64; 3]>, exact_only: bool) {
    if let [first, .., last] = ring.as_slice() {
        let duplicate = if exact_only {
            *first == *last
        } else {
            dist_sq(first, last) < 1e-10
        };
        if duplicate {
            ring.pop();
        }
    }
}

/// Triangulates a B-Rep model into a [`TessellatedMesh`].
#[must_use]
pub fn tessellate_brep_to_mesh(brep: &BrepModel, chord_error_mm: f64) -> TessellatedMesh {
    tessellate_brep_to_mesh_inner(brep, chord_error_mm, None, None, None, false, None, false)
}

/// Tessellates faces owned by one mapped BODY through its region and shell links.
///
/// Returns `None` when `body_id` is missing or ambiguous, when an ownership
/// link is missing, ambiguous, or inconsistent, or when any selected face
/// cannot be tessellated. A valid body with no faces returns an empty mesh.
#[must_use]
pub fn tessellate_brep_body_to_mesh(
    brep: &BrepModel,
    body_id: u32,
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let bodies = unique_entity_map(&brep.bodies, |body| body.id)?;
    let regions = unique_entity_map(&brep.regions, |region| region.id)?;
    let shells = unique_entity_map(&brep.shells, |shell| shell.id)?;
    let faces = unique_entity_map(&brep.faces, |face| face.id)?;
    let body = bodies.get(&body_id)?;
    let region_shell_links = brep
        .regions
        .iter()
        .flat_map(|region| region.shells.iter().map(move |shell| (region.id, *shell)))
        .collect::<HashSet<_>>();
    let shell_back_face_links = brep
        .shells
        .iter()
        .flat_map(|shell| shell.back_faces.iter().map(move |face| (shell.id, *face)))
        .collect::<HashSet<_>>();
    let shell_front_face_links = brep
        .shells
        .iter()
        .flat_map(|shell| shell.front_faces.iter().map(move |face| (shell.id, *face)))
        .collect::<HashSet<_>>();

    let mut face_ids = HashSet::new();
    let mut region_ids = HashSet::with_capacity(body.regions.len());
    let mut shell_regions = HashMap::new();
    for region_id in &body.regions {
        if !region_ids.insert(*region_id) {
            continue;
        }
        let region = regions.get(region_id)?;
        if region.body != body.id || matches!(region.kind, super::RegionKind::Unsupported(_)) {
            return None;
        }
        for shell_id in &region.shells {
            if let Some(previous_region) = shell_regions.insert(*shell_id, region.id) {
                if previous_region != region.id {
                    return None;
                }
                continue;
            }
            let shell = shells.get(shell_id)?;
            if shell.region != region.id {
                return None;
            }
            for (face_links, back_side) in [(&shell.back_faces, true), (&shell.front_faces, false)]
            {
                for face_id in face_links {
                    let face = faces.get(face_id)?;
                    if (back_side && face.back_shell != shell.id)
                        || (!back_side && face.front_shell != shell.id)
                    {
                        return None;
                    }
                    let back_shell = shells.get(&face.back_shell)?;
                    let front_shell = shells.get(&face.front_shell)?;
                    let back_region = regions.get(&back_shell.region)?;
                    let front_region = regions.get(&front_shell.region)?;
                    if !region_shell_links.contains(&(back_region.id, back_shell.id))
                        || !shell_back_face_links.contains(&(back_shell.id, *face_id))
                        || !region_shell_links.contains(&(front_region.id, front_shell.id))
                        || !shell_front_face_links.contains(&(front_shell.id, *face_id))
                        || back_region.body != body.id
                        || front_region.body != body.id
                    {
                        return None;
                    }
                    face_ids.insert(*face_id);
                }
            }
        }
    }

    let mut completed_face_ids = HashSet::with_capacity(face_ids.len());
    let mesh = tessellate_brep_to_mesh_inner(
        brep,
        chord_error_source_units,
        None,
        Some(&face_ids),
        Some(&mut completed_face_ids),
        false,
        None,
        false,
    );
    (completed_face_ids == face_ids).then_some(mesh)
}

/// Tessellates a BODY, expanding any resolved compound BODY children in order.
///
/// Child geometry is already expressed in the parent model frame; assembly
/// instance transforms are not applied here. Returns `None` for duplicate
/// hierarchy parents, missing or ambiguous BODY IDs, cycles, or failed child
/// tessellation.
#[must_use]
pub fn tessellate_resolved_brep_body_to_mesh(
    brep: &BrepModelWithResolvedBodyHierarchy,
    body_id: u32,
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let bodies = unique_entity_map(&brep.model.bodies, |body| body.id)?;
    let mut children_by_parent = HashMap::with_capacity(brep.body_children.len());
    for children in &brep.body_children {
        let mut unique_children = HashSet::with_capacity(children.child_brep_body_ids.len());
        if children.child_brep_body_ids.is_empty()
            || children
                .child_brep_body_ids
                .iter()
                .any(|child_id| !unique_children.insert(*child_id))
            || children_by_parent
                .insert(children.parent_brep_body_id, &children.child_brep_body_ids)
                .is_some()
        {
            return None;
        }
    }
    let mut stack = vec![body_id];
    let mut visited = HashSet::new();
    let mut mesh = TessellatedMesh::default();
    while let Some(body_id) = stack.pop() {
        bodies.get(&body_id)?;
        if !visited.insert(body_id) {
            return None;
        }
        if let Some(child_ids) = children_by_parent.get(&body_id) {
            stack.extend(child_ids.iter().rev().copied());
        } else {
            let child_mesh =
                tessellate_brep_body_to_mesh(&brep.model, body_id, chord_error_source_units)?;
            mesh.triangles.extend(child_mesh.triangles);
            mesh.normals.extend(child_mesh.normals);
            mesh.wire_edges.extend(child_mesh.wire_edges);
        }
    }
    Some(mesh)
}

/// Tessellates one mapped face by its B-Rep face ID.
#[must_use]
pub fn tessellate_brep_face(
    brep: &BrepModel,
    face_id: u32,
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let face = brep.faces.iter().find(|face| face.id == face_id)?;
    let mesh = tessellate_brep_to_mesh_inner(
        brep,
        chord_error_source_units,
        Some(face.source.node_index),
        None,
        None,
        false,
        None,
        false,
    );
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// Tessellates one mapped face using bounded candidates for supported XT
/// intersection trims. Unsupported candidates fail the face closed.
#[must_use]
pub fn tessellate_brep_face_with_intersection_candidates(
    brep: &BrepModel,
    face_id: u32,
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let face = brep.faces.iter().find(|face| face.id == face_id)?;
    let mesh = tessellate_brep_to_mesh_inner(
        brep,
        chord_error_source_units,
        Some(face.source.node_index),
        None,
        None,
        false,
        None,
        true,
    );
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// Tessellates one mapped face only when every trim loop has a validated
/// explicit same-support UV curve. Mapped sweeps require a unit direction;
/// this does not inverse-project missing loops.
#[must_use]
pub fn tessellate_brep_face_with_explicit_uv(
    brep: &BrepModel,
    source_face_node_index: u32,
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let swept_direction = brep
        .faces
        .iter()
        .find(|face| face.source.node_index == source_face_node_index)
        .and_then(|face| face.surface)
        .and_then(|surface_id| {
            brep.surfaces
                .iter()
                .find(|surface| surface.id == surface_id)
        })
        .and_then(|surface| {
            surface
                .kind
                .swept_section_direction()
                .map(|(_, direction)| direction.to_array())
        });
    if let Some(direction) = swept_direction {
        let length = direction[0].hypot(direction[1]).hypot(direction[2]);
        if !length.is_finite() || (length - 1.0).abs() > 64.0 * f64::EPSILON {
            return None;
        }
    }
    let mesh = tessellate_brep_to_mesh_inner(
        brep,
        chord_error_source_units,
        Some(source_face_node_index),
        None,
        None,
        true,
        swept_direction,
        false,
    );
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// Tessellates one mapped swept face with validated explicit same-support UV trims.
/// The caller must supply the already-resolved finite, nonzero sweep direction.
#[must_use]
pub fn tessellate_swept_brep_face_with_explicit_uv(
    brep: &BrepModel,
    source_face_node_index: u32,
    sweep_direction: [f64; 3],
    chord_error_source_units: f64,
) -> Option<TessellatedMesh> {
    let face = brep
        .faces
        .iter()
        .find(|face| face.source.node_index == source_face_node_index)?;
    let surface_id = face.surface?;
    let surface = brep
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)?;
    surface.kind.swept_section_direction()?;
    if sweep_direction.iter().any(|value| !value.is_finite())
        || sweep_direction.iter().all(|value| *value == 0.0)
    {
        return None;
    }
    let mesh = tessellate_brep_to_mesh_inner(
        brep,
        chord_error_source_units,
        Some(source_face_node_index),
        None,
        None,
        true,
        Some(sweep_direction),
        false,
    );
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// Tessellates one `X_T` swept face from caller-validated UV loops.
///
/// The raw swept node, mapped section, and mapped face/support must all come
/// from the same parsed document. The face and support source indexes and the
/// section pointer are checked here; `uv_loops` must already be validated by
/// the caller as the complete trims for this face on that support, in face-loop
/// order. The serialized scale is not used.
#[must_use]
pub fn tessellate_mapped_xt_swept_face_with_explicit_uv(
    brep: &BrepModel,
    swept: &crate::brep::SweptSurfaceData,
    section: &crate::brep::CurveGeometry,
    source_face_node_index: u32,
    uv_loops: &[Vec<[f64; 2]>],
) -> Option<TessellatedMesh> {
    tessellate_mapped_xt_swept_face_internal(
        brep,
        swept,
        section,
        source_face_node_index,
        uv_loops,
        None,
    )
    .map(|(mesh, _)| mesh)
}

/// Tessellates one `X_T` swept face until sampled surface deviation is within
/// `max_deviation_source_units`.
///
/// Every triangle is sampled at its three edge midpoints and centroid against
/// linear interpolation. Refinement is uniform across the face so shared edges
/// within this face split identically. It does not guarantee matching seams
/// with adjacent faces. This is a sampled error check, not a certified bound;
/// the function returns `None` when the tolerance is invalid, the sample check
/// cannot pass, or the fixed triangle budget is exceeded.
#[must_use]
pub fn tessellate_mapped_xt_swept_face_with_sampled_chord_error(
    brep: &BrepModel,
    swept: &crate::brep::SweptSurfaceData,
    section: &crate::brep::CurveGeometry,
    source_face_node_index: u32,
    uv_loops: &[Vec<[f64; 2]>],
    max_deviation_source_units: f64,
) -> Option<TessellatedMesh> {
    if !max_deviation_source_units.is_finite() || max_deviation_source_units <= 0.0 {
        return None;
    }
    tessellate_mapped_xt_swept_face_internal(
        brep,
        swept,
        section,
        source_face_node_index,
        uv_loops,
        Some(max_deviation_source_units),
    )
    .map(|(mesh, _)| mesh)
}

const MAX_SAMPLED_SWEEP_TRIANGLES: usize = 500_000;
const MAX_SAMPLED_SWEEP_DEPTH: usize = 7;

#[allow(
    clippy::float_cmp,
    reason = "Exact direction equality preserves the mapped sweep's source-defined orientation."
)]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_mapped_xt_swept_face_internal(
    brep: &BrepModel,
    swept: &crate::brep::SweptSurfaceData,
    section: &crate::brep::CurveGeometry,
    source_face_node_index: u32,
    uv_loops: &[Vec<[f64; 2]>],
    max_sampled_deviation: Option<f64>,
) -> Option<(TessellatedMesh, Option<usize>)> {
    if brep.source_format != crate::brep::BrepSourceFormat::Text {
        return None;
    }
    let face = brep
        .faces
        .iter()
        .find(|face| face.source.node_index == source_face_node_index)?;
    if face.loops.is_empty()
        || uv_loops.len() != face.loops.len()
        || face.loops.iter().any(|loop_id| {
            !brep
                .loops
                .iter()
                .any(|loop_| loop_.id == *loop_id && loop_.face == face.id)
        })
        || uv_loops.iter().any(|uv_loop| uv_loop.len() < 3)
    {
        return None;
    }
    if max_sampled_deviation.is_some() {
        let uv_vertex_count = uv_loops
            .iter()
            .try_fold(0usize, |count, loop_| count.checked_add(loop_.len()))?;
        if uv_vertex_count > 2048 {
            return None;
        }
    }
    let surface_id = face.surface?;
    let surface = brep
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)?;
    if surface.source.node_index != swept.source.node_index {
        return None;
    }
    let section_node_index = swept.section?;
    if swept.source.type_name != "SWEPT_SURF"
        || swept.source.node_type != 67
        || surface.source.type_name != "SWEPT_SURF"
        || surface.source.node_type != 67
        || section.source.node_index != section_node_index
        || section.source.type_name != "B_CURVE"
        || !brep.curves.iter().any(|candidate| candidate == section)
        || uv_loops
            .iter()
            .flatten()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    if let Some((mapped_section_id, mapped_direction)) = surface.kind.swept_section_direction() {
        if mapped_section_id != section.id
            || mapped_direction.to_array() != [swept.sweep[0]?, swept.sweep[1]?, swept.sweep[2]?]
        {
            return None;
        }
    } else if !matches!(&surface.kind, SurfaceKind::Unsupported { type_name } if type_name == "SWEPT_SURF")
    {
        return None;
    }
    let direction = [swept.sweep[0]?, swept.sweep[1]?, swept.sweep[2]?];
    if !matches!(&section.kind, CurveKind::Nurbs(_)) {
        return None;
    }
    let mut loops_3d = Vec::with_capacity(uv_loops.len());
    for uv_loop in uv_loops {
        let mut sampled_loop_points = Vec::with_capacity(uv_loop.len());
        for &[u, v] in uv_loop {
            sampled_loop_points
                .push(evaluate_mapped_xt_swept_point_candidate(swept, Some(section), u, v).ok()?);
        }
        loops_3d.push(sampled_loop_points);
    }
    let [u_min, u_max, v_min, v_max] = uv_loops.iter().flatten().fold(
        [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ],
        |[u_min, u_max, v_min, v_max], [u, v]| {
            [u_min.min(*u), u_max.max(*u), v_min.min(*v), v_max.max(*v)]
        },
    );
    let u_span = u_max - u_min;
    let v_span = v_max - v_min;
    if !u_span.is_finite() || u_span <= 0.0 || !v_span.is_finite() || v_span <= 0.0 {
        return None;
    }
    let mut result = TessellatedMesh::default();
    for loop_3d in &loops_3d {
        for index in 0..loop_3d.len() {
            result
                .wire_edges
                .push([loop_3d[index], loop_3d[(index + 1) % loop_3d.len()]]);
        }
    }
    let face_sense = (face.sense == Sense::Positive) == (surface.sense == Sense::Positive);
    let mut sampled_depth = None;
    let evaluated = try_tessellate_trimmed_surface_with_uv_and_sampled_error(
        &surface.kind,
        |_, _, _| None,
        |u, v| evaluate_ruled_sweep_surface(&section.kind, direction, u, v),
        &loops_3d,
        Some(uv_loops),
        [u_span / 16.0, v_span / 16.0],
        true,
        face_sense,
        &mut result,
        max_sampled_deviation,
        MAX_SAMPLED_SWEEP_TRIANGLES,
        MAX_SAMPLED_SWEEP_DEPTH,
        &mut sampled_depth,
    );
    if !matches!(evaluated, TrimmedSurfaceResult::Tessellated) {
        return None;
    }
    if let Some(depth) = sampled_depth {
        result.wire_edges = sampled_uv_wire_edges(uv_loops, depth, |u, v| {
            evaluate_mapped_xt_swept_point_candidate(swept, Some(section), u, v).ok()
        })?;
    }
    Some((result, sampled_depth))
}

#[allow(
    clippy::float_cmp,
    reason = "Exact parameter equality detects collapsed trim intervals without changing source parameter semantics."
)]
#[allow(
    clippy::too_many_lines,
    reason = "Coordinates loop sampling, surface-specific paths, and fail-closed face assembly; extracting these branches risks changing shared failure behavior."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_brep_to_mesh_inner(
    brep: &BrepModel,
    chord_error_mm: f64,
    source_face_node_index: Option<u32>,
    selected_face_ids: Option<&HashSet<u32>>,
    mut completed_face_ids: Option<&mut HashSet<u32>>,
    require_explicit_uv_loops: bool,
    swept_direction: Option<[f64; 3]>,
    sample_intersection_curves: bool,
) -> TessellatedMesh {
    let mut result = TessellatedMesh::default();
    let chord_error = if chord_error_mm.is_finite() && chord_error_mm > 0.0 {
        chord_error_mm
    } else {
        0.5
    };

    let edge_map: HashMap<u32, &crate::brep::Edge> = brep.edges.iter().map(|e| (e.id, e)).collect();
    let mut duplicate_edge_half_edges = HashSet::new();
    if sample_intersection_curves {
        let mut seen_edge_half_edges = HashSet::new();
        for half_edge_id in brep.edges.iter().flat_map(|edge| &edge.half_edges) {
            if !seen_edge_half_edges.insert(*half_edge_id) {
                duplicate_edge_half_edges.insert(*half_edge_id);
            }
        }
    }
    let curve_map: HashMap<u32, &crate::brep::CurveGeometry> =
        brep.curves.iter().map(|c| (c.id, c)).collect();
    let vertex_map: HashMap<u32, &crate::brep::Vertex> =
        brep.vertices.iter().map(|v| (v.id, v)).collect();
    let point_map: HashMap<u32, [f64; 3]> = brep
        .points
        .iter()
        .map(|p| (p.id, p.position.to_array()))
        .collect();
    let he_map: HashMap<u32, &crate::brep::HalfEdge> =
        brep.half_edges.iter().map(|h| (h.id, h)).collect();
    let loop_map: HashMap<u32, &crate::brep::Loop> = brep.loops.iter().map(|l| (l.id, l)).collect();
    let surf_map: HashMap<u32, &crate::brep::SurfaceGeometry> =
        brep.surfaces.iter().map(|s| (s.id, s)).collect();

    for face in &brep.faces {
        if source_face_node_index.is_some_and(|source_index| face.source.node_index != source_index)
            || selected_face_ids.is_some_and(|face_ids| !face_ids.contains(&face.id))
        {
            continue;
        }
        let face_triangle_start = result.triangles.len();
        let face_wire_start = result.wire_edges.len();
        let surf = face.surface.and_then(|sid| surf_map.get(&sid).copied());

        if !require_explicit_uv_loops
            && matches!(
                surf.map(|surface| &surface.kind),
                Some(SurfaceKind::Mesh { .. })
            )
        {
            let Ok(Some(triangles)) =
                brep.mesh_triangles_for_face_with_inferred_index_origin(face.id)
            else {
                continue;
            };
            for triangle in triangles {
                let normal = vec3_normalize(vec3_cross(
                    vec3_sub(triangle[1], triangle[0]),
                    vec3_sub(triangle[2], triangle[0]),
                ));
                result.triangles.push(triangle);
                result.normals.push([normal; 3]);
            }
            if result.triangles.len() > face_triangle_start {
                if let Some(completed) = completed_face_ids.as_deref_mut() {
                    completed.insert(face.id);
                }
            }
            continue;
        }

        // Extract ordered 3D loop boundary vertices
        let mut loops_3d: Vec<Vec<[f64; 3]>> = Vec::new();
        let mut explicit_uv_loops = Vec::new();
        let supports_explicit_uv = matches!(
            surf.map(|s| &s.kind),
            Some(
                SurfaceKind::Nurbs(_)
                    | SurfaceKind::BlendedEdge { .. }
                    | SurfaceKind::Swept { .. }
                    | SurfaceKind::SweptWithSource { .. }
                    | SurfaceKind::Offset { .. }
            )
        );
        let mut all_loops_have_explicit_uv = supports_explicit_uv;
        let mut curve_sampling_failed = false;
        for &loop_id in &face.loops {
            if curve_sampling_failed {
                break;
            }
            if let Some(l) = loop_map.get(&loop_id) {
                let mut ring: Vec<[f64; 3]> = Vec::new();
                let mut sampled_intersection_boundary = false;
                let n_he = l.half_edges.len();
                if n_he == 0 {
                    curve_sampling_failed = true;
                    break;
                }
                let direct_uv = if supports_explicit_uv {
                    let Some(surface_id) = face.surface else {
                        curve_sampling_failed = true;
                        break;
                    };
                    let Some(surface) = surf else {
                        curve_sampling_failed = true;
                        break;
                    };
                    match surface_parametric_uv_loop(
                        brep,
                        surface_id,
                        &surface.kind,
                        l,
                        &he_map,
                        &edge_map,
                        &curve_map,
                        &vertex_map,
                        &point_map,
                        chord_error,
                        swept_direction,
                    ) {
                        Ok(Some(uv)) => Some(uv),
                        Ok(None) => {
                            all_loops_have_explicit_uv = false;
                            None
                        }
                        Err(()) => {
                            curve_sampling_failed = true;
                            break;
                        }
                    }
                } else {
                    None
                };
                if let Some(uv_ring) = direct_uv {
                    let Some(surface) = surf.map(|surface| &surface.kind) else {
                        curve_sampling_failed = true;
                        break;
                    };
                    if matches!(surface, SurfaceKind::BlendedEdge { .. })
                        && uv_ring.iter().any(|uv| !(0.0..=1.0).contains(&uv[1]))
                    {
                        curve_sampling_failed = true;
                        break;
                    }
                    let ring: Option<Vec<[f64; 3]>> = uv_ring
                        .iter()
                        .map(|uv| surface_point_in_brep(brep, surface, *uv, swept_direction))
                        .collect();
                    let Some(ring) = ring else {
                        curve_sampling_failed = true;
                        break;
                    };
                    if ring.len() < 3
                        || ring.iter().flatten().any(|value| !value.is_finite())
                        || ring.first().is_none_or(|origin| {
                            ring.iter()
                                .skip(1)
                                .all(|point| dist_sq(origin, point) <= 1e-20)
                        })
                    {
                        curve_sampling_failed = true;
                        break;
                    }
                    for i in 0..ring.len() {
                        result
                            .wire_edges
                            .push([ring[i], ring[(i + 1) % ring.len()]]);
                    }
                    loops_3d.push(ring);
                    explicit_uv_loops.push(uv_ring);
                    continue;
                }
                for (idx, &he_id) in l.half_edges.iter().enumerate() {
                    let Some(he) = he_map.get(&he_id) else {
                        curve_sampling_failed = true;
                        break;
                    };

                    let curve_id = he.curve.or_else(|| {
                        he.edge
                            .and_then(|eid| edge_map.get(&eid))
                            .and_then(|e| e.curve)
                    });
                    let curve_geom = curve_id.and_then(|cid| curve_map.get(&cid).copied());
                    let next_he = l
                        .half_edges
                        .get((idx + 1) % n_he)
                        .and_then(|nid| he_map.get(nid))
                        .or_else(|| he.forward.and_then(|fid| he_map.get(&fid)));
                    let prev_he = l
                        .half_edges
                        .get((idx + n_he - 1) % n_he)
                        .and_then(|pid| he_map.get(pid))
                        .or_else(|| he.backward.and_then(|bid| he_map.get(&bid)));
                    let (start_vertex, end_vertex) = fin_endpoint_vertices(
                        brep.source_format,
                        he,
                        prev_he.copied(),
                        next_he.copied(),
                    );
                    let mut p_start = start_vertex
                        .and_then(|vid| vertex_map.get(&vid))
                        .and_then(|v| point_map.get(&v.point))
                        .copied()
                        .or_else(|| {
                            let edge = he.edge.and_then(|eid| edge_map.get(&eid))?;
                            let vid = if he.sense == Sense::Positive {
                                edge.start_vertex
                            } else {
                                edge.end_vertex
                            }?;
                            let v = vertex_map.get(&vid)?;
                            point_map.get(&v.point).copied()
                        });
                    let mut p_end = end_vertex
                        .and_then(|vid| vertex_map.get(&vid))
                        .and_then(|v| point_map.get(&v.point))
                        .copied()
                        .or_else(|| {
                            let edge = he.edge.and_then(|eid| edge_map.get(&eid))?;
                            let vid = if he.sense == Sense::Positive {
                                edge.end_vertex
                            } else {
                                edge.start_vertex
                            }?;
                            let v = vertex_map.get(&vid)?;
                            point_map.get(&v.point).copied()
                        });

                    // Unwrap Trimmed curve wrapper to basis curve
                    let mut basis_curve = curve_geom;
                    let mut trimmed_params: Option<(f64, f64)> = None;
                    let mut trimmed_points: Option<([f64; 3], [f64; 3])> = None;
                    let mut wrapper_ids = Vec::new();
                    while let Some(cg) = basis_curve {
                        if let CurveKind::Trimmed {
                            basis_curve: bc,
                            start_point,
                            end_point,
                            start_parameter,
                            end_parameter,
                        } = &cg.kind
                        {
                            if wrapper_ids.len() >= 64 || wrapper_ids.contains(&cg.id) {
                                curve_sampling_failed = true;
                                break;
                            }
                            wrapper_ids.push(cg.id);
                            if trimmed_params.is_none()
                                && start_parameter.is_finite()
                                && end_parameter.is_finite()
                            {
                                trimmed_params = Some((*start_parameter, *end_parameter));
                            }
                            if trimmed_points.is_none() {
                                trimmed_points =
                                    Some((start_point.to_array(), end_point.to_array()));
                            }
                            basis_curve = curve_map.get(bc).copied();
                            if basis_curve.is_none() {
                                curve_sampling_failed = true;
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    if curve_sampling_failed {
                        break;
                    }

                    if p_start.is_none() {
                        if let Some((sp, ep)) = trimmed_points {
                            let candidate = if he.sense == Sense::Positive { sp } else { ep };
                            if candidate[0].is_finite()
                                && candidate[1].is_finite()
                                && candidate[2].is_finite()
                            {
                                p_start = Some(candidate);
                            }
                        }
                    }

                    if p_end.is_none() {
                        if let Some((sp, ep)) = trimmed_points {
                            let candidate = if he.sense == Sense::Positive { ep } else { sp };
                            if candidate[0].is_finite()
                                && candidate[1].is_finite()
                                && candidate[2].is_finite()
                            {
                                p_end = Some(candidate);
                            }
                        }
                    }

                    match basis_curve.map(|cg| (&cg.kind, cg.sense)) {
                        Some((
                            CurveKind::Circle {
                                center,
                                normal,
                                x_axis,
                                radius,
                            },
                            c_sense,
                        )) => {
                            let c = center.to_array();
                            let (u_axis, v_axis, _n_axis) = orthonormal_frame_from_axis_and_ref(
                                normal.to_array(),
                                x_axis.to_array(),
                            );
                            let r = *radius;
                            let ccw = (he.sense == Sense::Positive) == (c_sense == Sense::Positive);

                            let is_full = n_he == 1
                                || match (p_start, p_end) {
                                    (Some(ps), Some(pe)) => dist_sq(&ps, &pe) < 1e-10,
                                    _ => true,
                                };

                            let (t_start, t_end) = if is_full {
                                let t_s = if let Some(ps) = p_start {
                                    let d = vec3_sub(ps, c);
                                    vec3_dot(d, v_axis).atan2(vec3_dot(d, u_axis))
                                } else {
                                    0.0
                                };
                                let t_e = if ccw {
                                    t_s + 2.0 * std::f64::consts::PI
                                } else {
                                    t_s - 2.0 * std::f64::consts::PI
                                };
                                (t_s, t_e)
                            } else {
                                let (Some(ps), Some(pe)) = (p_start, p_end) else {
                                    curve_sampling_failed = true;
                                    break;
                                };
                                let d_s = vec3_sub(ps, c);
                                let t_s = vec3_dot(d_s, v_axis).atan2(vec3_dot(d_s, u_axis));
                                let d_e = vec3_sub(pe, c);
                                let mut t_e = vec3_dot(d_e, v_axis).atan2(vec3_dot(d_e, u_axis));
                                if ccw {
                                    while t_e <= t_s {
                                        t_e += 2.0 * std::f64::consts::PI;
                                    }
                                } else {
                                    while t_e >= t_s {
                                        t_e -= 2.0 * std::f64::consts::PI;
                                    }
                                }
                                (t_s, t_e)
                            };

                            let delta_theta = (2.0
                                * (1.0 - (chord_error / r.max(0.1)).min(0.999)).acos())
                            .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
                            let n_seg = {
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    clippy::cast_sign_loss,
                                    reason = "Finite segment counts are clamped to 4..=64; NaN survives clamp and Rust's float-to-usize cast maps it to zero, preserving the current no-sample behavior."
                                )]
                                let segment_count = ((t_end - t_start).abs() / delta_theta)
                                    .ceil()
                                    .clamp(4.0, 64.0)
                                    as usize;
                                segment_count
                            };

                            for k in 0..n_seg {
                                let frac = {
                                    #[allow(
                                        clippy::cast_precision_loss,
                                        reason = "Within this loop, k is at most 63 and n_seg at most 64, so both usize values are exactly representable as f64."
                                    )]
                                    let fraction = k as f64 / n_seg as f64;
                                    fraction
                                };
                                let t = t_start + (t_end - t_start) * frac;
                                let pt = vec3_add(
                                    c,
                                    vec3_scale(
                                        vec3_add(
                                            vec3_scale(u_axis, t.cos()),
                                            vec3_scale(v_axis, t.sin()),
                                        ),
                                        r,
                                    ),
                                );
                                if ring.last().is_none_or(|last| dist_sq(last, &pt) > 1e-10) {
                                    ring.push(pt);
                                }
                            }
                        }
                        Some((
                            CurveKind::Ellipse {
                                center,
                                normal,
                                x_axis,
                                major_radius,
                                minor_radius,
                            },
                            c_sense,
                        )) => {
                            let c = center.to_array();
                            let (u_axis, v_axis, _n_axis) = orthonormal_frame_from_axis_and_ref(
                                normal.to_array(),
                                x_axis.to_array(),
                            );
                            let a = *major_radius;
                            let b = *minor_radius;
                            let ccw = (he.sense == Sense::Positive) == (c_sense == Sense::Positive);

                            let is_full = n_he == 1
                                || match (p_start, p_end) {
                                    (Some(ps), Some(pe)) => dist_sq(&ps, &pe) < 1e-10,
                                    _ => true,
                                };

                            let (t_start, t_end) = if is_full {
                                let t_s = if let Some(ps) = p_start {
                                    let d = vec3_sub(ps, c);
                                    (vec3_dot(d, v_axis) / b.max(1e-12))
                                        .atan2(vec3_dot(d, u_axis) / a.max(1e-12))
                                } else {
                                    0.0
                                };
                                let t_e = if ccw {
                                    t_s + 2.0 * std::f64::consts::PI
                                } else {
                                    t_s - 2.0 * std::f64::consts::PI
                                };
                                (t_s, t_e)
                            } else {
                                let (Some(ps), Some(pe)) = (p_start, p_end) else {
                                    curve_sampling_failed = true;
                                    break;
                                };
                                let d_s = vec3_sub(ps, c);
                                let t_s = (vec3_dot(d_s, v_axis) / b.max(1e-12))
                                    .atan2(vec3_dot(d_s, u_axis) / a.max(1e-12));
                                let d_e = vec3_sub(pe, c);
                                let mut t_e = (vec3_dot(d_e, v_axis) / b.max(1e-12))
                                    .atan2(vec3_dot(d_e, u_axis) / a.max(1e-12));
                                if ccw {
                                    while t_e <= t_s {
                                        t_e += 2.0 * std::f64::consts::PI;
                                    }
                                } else {
                                    while t_e >= t_s {
                                        t_e -= 2.0 * std::f64::consts::PI;
                                    }
                                }
                                (t_s, t_e)
                            };

                            let r_eff = a.max(b);
                            let delta_theta = (2.0
                                * (1.0 - (chord_error / r_eff.max(0.1)).min(0.999)).acos())
                            .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
                            let n_seg = {
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    clippy::cast_sign_loss,
                                    reason = "Finite segment counts are clamped to 4..=64; NaN survives clamp and Rust's float-to-usize cast maps it to zero, preserving the current no-sample behavior."
                                )]
                                let segment_count = ((t_end - t_start).abs() / delta_theta)
                                    .ceil()
                                    .clamp(4.0, 64.0)
                                    as usize;
                                segment_count
                            };

                            for k in 0..n_seg {
                                let frac = {
                                    #[allow(
                                        clippy::cast_precision_loss,
                                        reason = "Within this loop, k is at most 63 and n_seg at most 64, so both usize values are exactly representable as f64."
                                    )]
                                    let fraction = k as f64 / n_seg as f64;
                                    fraction
                                };
                                let t = t_start + (t_end - t_start) * frac;
                                let pt = vec3_add(
                                    c,
                                    vec3_add(
                                        vec3_scale(u_axis, a * t.cos()),
                                        vec3_scale(v_axis, b * t.sin()),
                                    ),
                                );
                                if ring.last().is_none_or(|last| dist_sq(last, &pt) > 1e-10) {
                                    ring.push(pt);
                                }
                            }
                        }
                        Some((CurveKind::Nurbs(nurbs), curve_sense)) => {
                            let (p_start_param, p_end_param) = match trimmed_params {
                                Some((p0, p1))
                                    if p0.is_finite()
                                        && p1.is_finite()
                                        && (p1 - p0).abs() > 1e-12 =>
                                {
                                    (p0, p1)
                                }
                                _ => {
                                    let u_min = nurbs.knots.first().copied().unwrap_or(0.0);
                                    let u_max = nurbs.knots.last().copied().unwrap_or(1.0);
                                    // Explicit trim parameters already encode the basis curve's sense.
                                    if curve_sense == Sense::Negative {
                                        (u_max, u_min)
                                    } else {
                                        (u_min, u_max)
                                    }
                                }
                            };
                            let (t_start, t_end) = if he.sense == Sense::Positive {
                                (p_start_param, p_end_param)
                            } else {
                                (p_end_param, p_start_param)
                            };
                            let Some(basis_curve) = basis_curve else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Some(samples) =
                                sample_nurbs_trim(&basis_curve.kind, t_start, t_end, chord_error)
                            else {
                                curve_sampling_failed = true;
                                break;
                            };
                            for pt in samples {
                                if ring.last().is_none_or(|last| dist_sq(last, &pt) > 1e-10) {
                                    ring.push(pt);
                                }
                            }
                        }
                        Some((CurveKind::Intersection { .. }, curve_sense))
                            if sample_intersection_curves =>
                        {
                            let Some(basis_curve) = basis_curve else {
                                curve_sampling_failed = true;
                                break;
                            };
                            sampled_intersection_boundary = true;
                            let direct_intersection = wrapper_ids.is_empty();
                            let mut chart_work_budget = MAX_INTERSECTION_CHART_SAMPLE_WORK;
                            let (t_start, t_end, expected_start, expected_end, endpoint_candidates) =
                                if direct_intersection {
                                    let CurveKind::Intersection { surfaces, .. } =
                                        &basis_curve.kind
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Some(face_surface) = face.surface else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    if surfaces[0] == surfaces[1]
                                        || surfaces.iter().filter(|id| **id == face_surface).count()
                                            != 1
                                        || surfaces.iter().any(|id| {
                                            brep.surfaces
                                                .iter()
                                                .filter(|surface| surface.id == *id)
                                                .count()
                                                != 1
                                        })
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    if he.dummy
                                        || he.loop_id != Some(l.id)
                                        || brep
                                            .half_edges
                                            .iter()
                                            .filter(|item| item.id == he.id)
                                            .count()
                                            != 1
                                        || l.half_edges.iter().filter(|id| **id == he.id).count()
                                            != 1
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let Some(start_vertex) = start_vertex else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Some(end_vertex) = end_vertex else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    if he.sense == Sense::Unknown {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let Some(edge_id) = he.edge else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Some(edge) = edge_map.get(&edge_id).copied() else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    if brep.edges.iter().filter(|item| item.id == edge.id).count()
                                        != 1
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    if edge.half_edges.iter().filter(|id| **id == he.id).count()
                                        != 1
                                        || duplicate_edge_half_edges.contains(&he.id)
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let expected_edge_vertices = match he.sense {
                                        Sense::Positive => (edge.start_vertex, edge.end_vertex),
                                        Sense::Negative => (edge.end_vertex, edge.start_vertex),
                                        Sense::Unknown => unreachable!(),
                                    };
                                    if expected_edge_vertices
                                        != (Some(start_vertex), Some(end_vertex))
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let unique_endpoint =
                                        |vertex_id: u32| -> Result<([f64; 3], f64), ()> {
                                            if brep
                                                .vertices
                                                .iter()
                                                .filter(|vertex| vertex.id == vertex_id)
                                                .count()
                                                != 1
                                            {
                                                return Err(());
                                            }
                                            let vertex =
                                                vertex_map.get(&vertex_id).copied().ok_or(())?;
                                            if brep
                                                .points
                                                .iter()
                                                .filter(|point| point.id == vertex.point)
                                                .count()
                                                != 1
                                            {
                                                return Err(());
                                            }
                                            let (position, tolerance) = linked_vertex_point(
                                                Some(vertex_id),
                                                &vertex_map,
                                                &point_map,
                                            )?
                                            .ok_or(())?;
                                            let tolerance = match tolerance {
                                                Some(value) => value,
                                                None if brep.source_format
                                                    == BrepSourceFormat::Text =>
                                                {
                                                    xt_null_entity_tolerance(&vertex.owner, brep)?
                                                }
                                                None => 0.0,
                                            };
                                            Ok((position, tolerance))
                                        };
                                    let (
                                        Ok((start_point, start_tolerance)),
                                        Ok((end_point, end_tolerance)),
                                    ) = (
                                        unique_endpoint(start_vertex),
                                        unique_endpoint(end_vertex),
                                    )
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Ok(edge_tolerance) =
                                        intersection_edge_tolerance(he, &edge_map, brep)
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let start_tolerance =
                                        if brep.source_format == BrepSourceFormat::Text {
                                            start_tolerance
                                        } else {
                                            edge_tolerance.max(start_tolerance)
                                        };
                                    let end_tolerance =
                                        if brep.source_format == BrepSourceFormat::Text {
                                            end_tolerance
                                        } else {
                                            edge_tolerance.max(end_tolerance)
                                        };
                                    let numeric_tolerance = |point: [f64; 3], tolerance: f64| {
                                        let scale = point
                                            .iter()
                                            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
                                        tolerance.max(256.0 * f64::EPSILON * scale)
                                    };
                                    let curve = basis_curve;
                                    if brep
                                        .curves
                                        .iter()
                                        .filter(|item| item.id == curve.id)
                                        .count()
                                        != 1
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let solver_visits =
                                        intersection_chart_solver_visits(brep, &curve.kind);
                                    let chart_point_count = match &curve.kind {
                                        CurveKind::Intersection { chart_points, .. } => {
                                            chart_points.len()
                                        }
                                        _ => 0,
                                    };
                                    let CurveKind::Intersection { surfaces, .. } = &curve.kind
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Some(surface_a) = brep
                                        .surfaces
                                        .iter()
                                        .find(|surface| surface.id == surfaces[0])
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let Some(surface_b) = brep
                                        .surfaces
                                        .iter()
                                        .find(|surface| surface.id == surfaces[1])
                                    else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    let cone_cylinder_inverse = matches!(
                                        (&surface_a.kind, &surface_b.kind),
                                        (
                                            SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. },
                                            SurfaceKind::Cylinder { .. },
                                        ) | (
                                            SurfaceKind::Cylinder { .. },
                                            SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. },
                                        )
                                    );
                                    let plane_xt_cone_inverse = matches!(
                                        (&surface_a.kind, &surface_b.kind),
                                        (
                                            SurfaceKind::Plane { .. },
                                            SurfaceKind::XtCone { .. } | SurfaceKind::Cone { .. },
                                        ) | (
                                            SurfaceKind::XtCone { .. } | SurfaceKind::Cone { .. },
                                            SurfaceKind::Plane { .. },
                                        )
                                    );
                                    if !cone_cylinder_inverse && !plane_xt_cone_inverse {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let inverse_budget_exceeded = if cone_cylinder_inverse {
                                        consume_cone_cylinder_inverse_work(
                                            chart_point_count,
                                            &mut chart_work_budget,
                                        )
                                        .is_none()
                                    } else {
                                        consume_intersection_inverse_work(
                                            chart_point_count,
                                            solver_visits,
                                            &mut chart_work_budget,
                                        )
                                        .is_none()
                                    };
                                    if inverse_budget_exceeded {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    let inverse_endpoint = |point, tolerance| {
                                        if cone_cylinder_inverse {
                                            super::intersection_eval::inverse_cone_cylinder_intersection_parameter(
                                                brep,
                                                &curve.kind,
                                                point,
                                                tolerance,
                                            )
                                            .map(|(parameter, candidate)| (parameter, Some(candidate)))
                                        } else {
                                            super::intersection_eval::inverse_plane_xt_cone_intersection_parameter(
                                                brep,
                                                &curve.kind,
                                                point,
                                                tolerance,
                                            )
                                            .map(|parameter| (parameter, None))
                                        }
                                    };
                                    let (
                                        start_parameter,
                                        start_candidate,
                                        end_parameter,
                                        end_candidate,
                                    ) = match (
                                        inverse_endpoint(
                                            start_point,
                                            numeric_tolerance(start_point, start_tolerance),
                                        ),
                                        inverse_endpoint(
                                            end_point,
                                            numeric_tolerance(end_point, end_tolerance),
                                        ),
                                    ) {
                                        (
                                            Ok((start, start_candidate)),
                                            Ok((end, end_candidate)),
                                        ) if start != end => {
                                            (start, start_candidate, end, end_candidate)
                                        }
                                        _ => {
                                            curve_sampling_failed = true;
                                            break;
                                        }
                                    };
                                    if !matches!(curve_sense, Sense::Positive | Sense::Negative)
                                        || ((end_parameter > start_parameter)
                                            != (curve_sense == he.sense))
                                    {
                                        curve_sampling_failed = true;
                                        break;
                                    }
                                    (
                                        start_parameter,
                                        end_parameter,
                                        Some(start_point),
                                        Some(end_point),
                                        start_candidate.zip(end_candidate),
                                    )
                                } else {
                                    let Some((p0, p1)) = trimmed_params.filter(|(p0, p1)| {
                                        p0.is_finite() && p1.is_finite() && p0 != p1
                                    }) else {
                                        curve_sampling_failed = true;
                                        break;
                                    };
                                    match he.sense {
                                        Sense::Positive => (
                                            p0,
                                            p1,
                                            trimmed_points.map(|(start, _)| start),
                                            trimmed_points.map(|(_, end)| end),
                                            None,
                                        ),
                                        Sense::Negative => (
                                            p1,
                                            p0,
                                            trimmed_points.map(|(_, end)| end),
                                            trimmed_points.map(|(start, _)| start),
                                            None,
                                        ),
                                        Sense::Unknown => {
                                            curve_sampling_failed = true;
                                            break;
                                        }
                                    }
                                };
                            let Some(expected_start) = expected_start else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Some(expected_end) = expected_end else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Some(samples) = sample_intersection_trim(
                                brep,
                                &basis_curve.kind,
                                t_start,
                                t_end,
                                chord_error,
                                endpoint_candidates,
                                &mut chart_work_budget,
                            ) else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Some(&sample_start) = samples.first() else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Some(&sample_end) = samples.last() else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let Ok(edge_tolerance) =
                                intersection_edge_tolerance(he, &edge_map, brep)
                            else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let vertex_tolerance = |vertex_id: Option<u32>| -> Result<f64, ()> {
                                match linked_vertex_point(vertex_id, &vertex_map, &point_map) {
                                    Ok(Some((_, Some(tolerance)))) => Ok(tolerance),
                                    Ok(Some((_, None)))
                                        if brep.source_format == BrepSourceFormat::Text =>
                                    {
                                        let vertex = vertex_id
                                            .and_then(|id| vertex_map.get(&id).copied())
                                            .ok_or(())?;
                                        xt_null_entity_tolerance(&vertex.owner, brep)
                                    }
                                    Ok(Some((_, None)) | None) => Ok(0.0),
                                    Err(()) => Err(()),
                                }
                            };
                            let (Ok(start_tolerance), Ok(end_tolerance)) =
                                (vertex_tolerance(start_vertex), vertex_tolerance(end_vertex))
                            else {
                                curve_sampling_failed = true;
                                break;
                            };
                            let start_anchor_tolerance =
                                if brep.source_format == BrepSourceFormat::Text {
                                    start_tolerance
                                } else {
                                    edge_tolerance.max(start_tolerance)
                                };
                            let end_anchor_tolerance =
                                if brep.source_format == BrepSourceFormat::Text {
                                    end_tolerance
                                } else {
                                    edge_tolerance.max(end_tolerance)
                                };
                            let trim_point_tolerance =
                                if brep.source_format == BrepSourceFormat::Text {
                                    0.0
                                } else {
                                    edge_tolerance
                                };
                            let (expected_start_tolerance, expected_end_tolerance) =
                                if direct_intersection {
                                    (start_anchor_tolerance, end_anchor_tolerance)
                                } else {
                                    (trim_point_tolerance, trim_point_tolerance)
                                };
                            if !within_source_endpoint_tolerance(
                                sample_start,
                                expected_start,
                                expected_start_tolerance,
                            ) || !within_source_endpoint_tolerance(
                                sample_end,
                                expected_end,
                                expected_end_tolerance,
                            ) || p_start.is_some_and(|point| {
                                !within_source_endpoint_tolerance(
                                    sample_start,
                                    point,
                                    start_anchor_tolerance,
                                )
                            }) || p_end.is_some_and(|point| {
                                !within_source_endpoint_tolerance(
                                    sample_end,
                                    point,
                                    end_anchor_tolerance,
                                )
                            }) {
                                curve_sampling_failed = true;
                                break;
                            }
                            append_intersection_samples(&mut ring, samples);
                        }
                        Some((CurveKind::Polyline(polyline), _)) => {
                            let points = if let Some((start, end)) = trimmed_points {
                                trimmed_polyline_points(polyline, he.sense, start, end)
                            } else {
                                full_polyline_points(polyline, he.sense)
                            };
                            let Some(points) = points else {
                                curve_sampling_failed = true;
                                break;
                            };
                            for pt in points {
                                if ring.last().is_none_or(|last| dist_sq(last, &pt) > 1e-10) {
                                    ring.push(pt);
                                }
                            }
                        }
                        _ => {
                            if let Some(p) = p_start {
                                if ring.last().is_none_or(|last| dist_sq(last, &p) > 1e-10) {
                                    ring.push(p);
                                }
                            } else {
                                curve_sampling_failed = true;
                                break;
                            }
                        }
                    }
                }
                if curve_sampling_failed {
                    break;
                }
                remove_ring_closure_duplicate(&mut ring, sampled_intersection_boundary);
                let ring_has_finite_points = ring
                    .iter()
                    .flatten()
                    .all(|coordinate| coordinate.is_finite());
                let ring_scale_sq = ring.first().map_or(0.0, |origin| {
                    ring.iter()
                        .map(|point| dist_sq(origin, point))
                        .fold(0.0, f64::max)
                });
                let mut ring_has_area = false;
                if ring.len() >= 3 && ring_has_finite_points && ring_scale_sq.is_finite() {
                    let origin = ring[0];
                    let threshold = 64.0 * f64::EPSILON * ring_scale_sq;
                    let farthest_vector = ring
                        .iter()
                        .skip(1)
                        .max_by(|left, right| {
                            dist_sq(&origin, left).total_cmp(&dist_sq(&origin, right))
                        })
                        .map(|point| vec3_sub(*point, origin));
                    if let Some(farthest_vector) = farthest_vector {
                        ring_has_area = ring.iter().skip(1).any(|point| {
                            let cross = vec3_cross(farthest_vector, vec3_sub(*point, origin));
                            let area_twice = vec3_norm(cross);
                            area_twice.is_finite() && area_twice > threshold
                        });
                    }
                }
                if ring.len() < 3 || !ring_has_finite_points || !ring_has_area {
                    curve_sampling_failed = true;
                    break;
                }
                // Record wire edges only after this entire loop is valid.
                for i in 0..ring.len() {
                    result
                        .wire_edges
                        .push([ring[i], ring[(i + 1) % ring.len()]]);
                }
                loops_3d.push(ring);
            } else {
                curve_sampling_failed = true;
                break;
            }
        }

        if curve_sampling_failed {
            result.wire_edges.truncate(face_wire_start);
            continue;
        }
        if require_explicit_uv_loops
            && (!all_loops_have_explicit_uv
                || face.loops.is_empty()
                || explicit_uv_loops.len() != face.loops.len())
        {
            result.wire_edges.truncate(face_wire_start);
            continue;
        }

        let effective_sense = match surf {
            Some(s) => (face.sense == Sense::Positive) == (s.sense == Sense::Positive),
            None => face.sense == Sense::Positive,
        };

        match surf.map(|s| &s.kind) {
            Some(SurfaceKind::Plane { point, normal, .. }) => {
                tessellate_planar_face(
                    point.to_array(),
                    normal.to_array(),
                    effective_sense,
                    &loops_3d,
                    &mut result,
                );
            }
            Some(
                kind @ SurfaceKind::Cylinder {
                    point,
                    axis,
                    radius,
                    ..
                },
            ) => {
                tessellate_cylindrical_face(
                    point.to_array(),
                    axis.to_array(),
                    *radius,
                    effective_sense,
                    kind,
                    &loops_3d,
                    chord_error,
                    &mut result,
                );
            }
            Some(
                kind @ (SurfaceKind::Cone {
                    point,
                    axis,
                    radius,
                    sin_half_angle,
                    cos_half_angle,
                    ..
                }
                | SurfaceKind::XtCone {
                    point,
                    axis,
                    radius,
                    sin_half_angle,
                    cos_half_angle,
                    ..
                }),
            ) => {
                tessellate_conical_face(
                    point.to_array(),
                    axis.to_array(),
                    *radius,
                    *sin_half_angle,
                    *cos_half_angle,
                    effective_sense,
                    kind,
                    &loops_3d,
                    chord_error,
                    &mut result,
                );
            }
            Some(kind @ SurfaceKind::Sphere { center, radius, .. }) => {
                tessellate_spherical_face(
                    center.to_array(),
                    *radius,
                    effective_sense,
                    kind,
                    &loops_3d,
                    chord_error,
                    &mut result,
                );
            }
            Some(
                kind @ SurfaceKind::Torus {
                    center,
                    axis,
                    major_radius,
                    minor_radius,
                    ..
                },
            ) => {
                tessellate_toroidal_face(
                    center.to_array(),
                    axis.to_array(),
                    *major_radius,
                    *minor_radius,
                    effective_sense,
                    kind,
                    &loops_3d,
                    chord_error,
                    &mut result,
                );
            }
            Some(kind @ SurfaceKind::Nurbs(_)) => {
                tessellate_nurbs_face(
                    kind,
                    effective_sense,
                    &loops_3d,
                    all_loops_have_explicit_uv.then_some(explicit_uv_loops.as_slice()),
                    &mut result,
                );
            }
            Some(kind @ SurfaceKind::BlendedEdge { ranges, .. }) => {
                if !all_loops_have_explicit_uv
                    || explicit_uv_loops.len() != loops_3d.len()
                    || explicit_uv_loops
                        .iter()
                        .flatten()
                        .any(|uv| !uv[0].is_finite() || !(0.0..=1.0).contains(&uv[1]))
                {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                }
                let radius = ranges[0].abs();
                if !radius.is_finite() || radius <= f64::EPSILON {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                }
                let delta_uv = (8.0 * chord_error / radius)
                    .sqrt()
                    .clamp(0.01, std::f64::consts::FRAC_PI_4);
                let evaluated = try_tessellate_trimmed_surface_with_uv(
                    kind,
                    |_, _, _| None,
                    |u, v| evaluate_surface(brep, kind, u, v),
                    &loops_3d,
                    Some(&explicit_uv_loops),
                    [delta_uv; 2],
                    true,
                    effective_sense,
                    &mut result,
                );
                if !matches!(evaluated, TrimmedSurfaceResult::Tessellated) {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                }
            }
            Some(kind) if kind.swept_section_direction().is_some() => {
                let Some((section, direction)) = kind.swept_section_direction() else {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                };
                let direction_raw = swept_direction.unwrap_or_else(|| direction.to_array());
                let dir_len = vec3_norm(direction_raw);
                if !dir_len.is_finite() || dir_len <= 1e-12 {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                }
                let direction = vec3_scale(direction_raw, 1.0 / dir_len);

                let section_geom = curve_map.get(&section).copied();
                let Some(section_geom) = section_geom else {
                    result.wire_edges.truncate(face_wire_start);
                    continue;
                };

                let mut current_section = section_geom;
                let mut depth_trim = 0;
                let mut trim_range: Option<(f64, f64)> = None;
                while let CurveKind::Trimmed {
                    basis_curve,
                    start_parameter,
                    end_parameter,
                    ..
                } = &current_section.kind
                {
                    if depth_trim > 8 {
                        break;
                    }
                    depth_trim += 1;
                    if trim_range.is_none()
                        && start_parameter.is_finite()
                        && end_parameter.is_finite()
                    {
                        trim_range = Some((*start_parameter, *end_parameter));
                    }
                    if let Some(next) = curve_map.get(basis_curve) {
                        current_section = *next;
                    } else {
                        break;
                    }
                }
                let section_kind = &current_section.kind;
                match section_kind {
                    CurveKind::Nurbs(_)
                    | CurveKind::Line { .. }
                    | CurveKind::Circle { .. }
                    | CurveKind::Ellipse { .. }
                    | CurveKind::Parabola { .. }
                    | CurveKind::Hyperbola { .. } => {}
                    _ => {
                        result.wire_edges.truncate(face_wire_start);
                        continue;
                    }
                }

                if all_loops_have_explicit_uv
                    && explicit_uv_loops.len() == loops_3d.len()
                    && !explicit_uv_loops.is_empty()
                {
                    let (u_min, u_max, v_min, v_max) = explicit_uv_loops.iter().flatten().fold(
                        (
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                        ),
                        |(u_min, u_max, v_min, v_max), uv| {
                            (
                                u_min.min(uv[0]),
                                u_max.max(uv[0]),
                                v_min.min(uv[1]),
                                v_max.max(uv[1]),
                            )
                        },
                    );
                    let u_span = u_max - u_min;
                    let v_span = v_max - v_min;
                    let maximum_parameter_span = u_span.max(v_span);
                    if !maximum_parameter_span.is_finite()
                        || maximum_parameter_span <= 0.0
                        || u_span <= 0.0
                        || v_span <= 0.0
                    {
                        result.wire_edges.truncate(face_wire_start);
                        continue;
                    }
                    let delta_u = match section_kind {
                        CurveKind::Circle { radius, .. } => {
                            let r = radius.abs().max(1e-6);
                            (8.0 * chord_error / r)
                                .sqrt()
                                .clamp(0.01, std::f64::consts::FRAC_PI_4)
                                .min(u_span / 16.0)
                        }
                        CurveKind::Ellipse { major_radius, .. } => {
                            let r = major_radius.abs().max(1e-6);
                            (8.0 * chord_error / r)
                                .sqrt()
                                .clamp(0.01, std::f64::consts::FRAC_PI_4)
                                .min(u_span / 16.0)
                        }
                        _ => maximum_parameter_span / 16.0,
                    };
                    let delta_v = maximum_parameter_span / 16.0;
                    let evaluated = try_tessellate_trimmed_surface_with_uv(
                        kind,
                        |_, _, _| None,
                        |u, v| {
                            evaluate_ruled_sweep_surface_in_brep(
                                Some(brep),
                                section_kind,
                                direction,
                                u,
                                v,
                            )
                        },
                        &loops_3d,
                        Some(&explicit_uv_loops),
                        [delta_u, delta_v],
                        true,
                        effective_sense,
                        &mut result,
                    );
                    if !matches!(evaluated, TrimmedSurfaceResult::Tessellated) {
                        result.wire_edges.truncate(face_wire_start);
                        continue;
                    }
                } else if !loops_3d.is_empty() {
                    if matches!(kind, SurfaceKind::SweptWithSource { .. }) {
                        // No source tolerance currently justifies accepting a projected trim residual.
                        result.wire_edges.truncate(face_wire_start);
                        continue;
                    }
                    let success = tessellate_swept_face_projected(
                        brep,
                        section_kind,
                        trim_range,
                        direction,
                        effective_sense,
                        &loops_3d,
                        &mut result,
                    );
                    if !success {
                        result.wire_edges.truncate(face_wire_start);
                        continue;
                    }
                }
            }
            Some(SurfaceKind::Offset {
                basis_surface,
                offset,
            }) => {
                let basis = surf_map.get(basis_surface).copied();
                if let Some(basis) = basis {
                    // Offset direction follows the basis sense; unknown bases are omitted.
                    let basis_reversed = basis.sense == Sense::Negative;
                    let oriented_offset = if basis_reversed { -*offset } else { *offset };
                    let face_sense = effective_sense;
                    match &basis.kind {
                        SurfaceKind::Plane { point, normal, .. } => {
                            let normal = normal.to_array();
                            let normal_len = vec3_norm(normal);
                            if point
                                .to_array()
                                .iter()
                                .chain(normal.iter())
                                .all(|value| value.is_finite())
                                && oriented_offset.is_finite()
                                && normal_len.is_finite()
                                && normal_len > 1e-12
                            {
                                let unit_normal = vec3_scale(normal, 1.0 / normal_len);
                                let offset_pt = vec3_add(
                                    point.to_array(),
                                    vec3_scale(unit_normal, oriented_offset),
                                );
                                if offset_pt.iter().all(|value| value.is_finite()) {
                                    tessellate_planar_face(
                                        offset_pt,
                                        unit_normal,
                                        face_sense,
                                        &loops_3d,
                                        &mut result,
                                    );
                                }
                            }
                        }
                        SurfaceKind::Cylinder {
                            point,
                            axis,
                            radius,
                            x_axis,
                        } => {
                            let eff_radius = *radius + oriented_offset;
                            let axis_len = vec3_norm(axis.to_array());
                            if point
                                .to_array()
                                .iter()
                                .chain(axis.to_array().iter())
                                .chain(x_axis.to_array().iter())
                                .all(|value| value.is_finite())
                                && radius.is_finite()
                                && *radius > 1e-12
                                && axis_len.is_finite()
                                && axis_len > 1e-12
                                && oriented_offset.is_finite()
                                && eff_radius.is_finite()
                                && eff_radius > 1e-12
                            {
                                let offset_kind = SurfaceKind::Cylinder {
                                    point: *point,
                                    axis: *axis,
                                    radius: eff_radius,
                                    x_axis: *x_axis,
                                };
                                tessellate_cylindrical_face(
                                    point.to_array(),
                                    axis.to_array(),
                                    eff_radius,
                                    face_sense,
                                    &offset_kind,
                                    &loops_3d,
                                    chord_error,
                                    &mut result,
                                );
                            }
                        }
                        SurfaceKind::Cone {
                            point,
                            axis,
                            radius,
                            sin_half_angle,
                            cos_half_angle,
                            x_axis,
                        } => {
                            let angle_norm = sin_half_angle.hypot(*cos_half_angle);
                            let unit_sin = sin_half_angle / angle_norm;
                            let unit_cos = cos_half_angle / angle_norm;
                            let eff_radius = *radius + oriented_offset * unit_cos;
                            let axis_raw = axis.to_array();
                            let axis_len = vec3_norm(axis_raw);
                            if point
                                .to_array()
                                .iter()
                                .chain(axis_raw.iter())
                                .chain(x_axis.to_array().iter())
                                .all(|value| value.is_finite())
                                && radius.is_finite()
                                && *radius > 1e-12
                                && sin_half_angle.is_finite()
                                && cos_half_angle.is_finite()
                                && angle_norm.is_finite()
                                && angle_norm > 1e-12
                                && cos_half_angle.abs() > 1e-12
                                && oriented_offset.is_finite()
                                && eff_radius.is_finite()
                                && eff_radius > 1e-12
                                && axis_len.is_finite()
                                && axis_len > 1e-12
                            {
                                let axis_unit = vec3_scale(axis_raw, 1.0 / axis_len);
                                let shifted_point = Vector3::from_array(vec3_add(
                                    point.to_array(),
                                    vec3_scale(axis_unit, -oriented_offset * unit_sin),
                                ));
                                let offset_kind = SurfaceKind::Cone {
                                    point: shifted_point,
                                    axis: Vector3::from_array(axis_unit),
                                    radius: eff_radius,
                                    sin_half_angle: *sin_half_angle,
                                    cos_half_angle: *cos_half_angle,
                                    x_axis: *x_axis,
                                };
                                tessellate_conical_face(
                                    shifted_point.to_array(),
                                    axis_unit,
                                    eff_radius,
                                    *sin_half_angle,
                                    *cos_half_angle,
                                    face_sense,
                                    &offset_kind,
                                    &loops_3d,
                                    chord_error,
                                    &mut result,
                                );
                            }
                        }
                        SurfaceKind::XtCone {
                            point,
                            axis,
                            radius,
                            sin_half_angle,
                            cos_half_angle,
                            x_axis,
                        } => {
                            let angle_norm = sin_half_angle.hypot(*cos_half_angle);
                            let unit_sin = sin_half_angle / angle_norm;
                            let unit_cos = cos_half_angle / angle_norm;
                            let eff_radius = *radius - oriented_offset * unit_cos;
                            let axis_raw = axis.to_array();
                            let axis_len = vec3_norm(axis_raw);
                            if point
                                .to_array()
                                .iter()
                                .chain(axis_raw.iter())
                                .chain(x_axis.to_array().iter())
                                .all(|value| value.is_finite())
                                && radius.is_finite()
                                && *radius > 1e-12
                                && sin_half_angle.is_finite()
                                && cos_half_angle.is_finite()
                                && angle_norm.is_finite()
                                && angle_norm > 1e-12
                                && *cos_half_angle > 1e-12
                                && oriented_offset.is_finite()
                                && eff_radius.is_finite()
                                && eff_radius > 1e-12
                                && axis_len.is_finite()
                                && axis_len > 1e-12
                            {
                                let axis_unit = vec3_scale(axis_raw, 1.0 / axis_len);
                                let tan_half_angle = unit_sin / unit_cos;
                                let q_margin = chord_error * tan_half_angle.abs();
                                if loops_3d.iter().flatten().any(|p| {
                                    let v = -vec3_dot(vec3_sub(*p, point.to_array()), axis_unit);
                                    let offset_v = v - oriented_offset * unit_sin;
                                    let source_q = *radius + v * tan_half_angle;
                                    let offset_q = eff_radius + offset_v * tan_half_angle;
                                    !source_q.is_finite()
                                        || !offset_v.is_finite()
                                        || !offset_q.is_finite()
                                        || source_q <= q_margin
                                        || offset_q <= q_margin
                                }) {
                                    result.wire_edges.truncate(face_wire_start);
                                    continue;
                                }
                                let shifted_point = Vector3::from_array(vec3_add(
                                    point.to_array(),
                                    vec3_scale(axis_unit, -oriented_offset * unit_sin),
                                ));
                                let offset_kind = SurfaceKind::XtCone {
                                    point: shifted_point,
                                    axis: Vector3::from_array(axis_unit),
                                    radius: eff_radius,
                                    sin_half_angle: *sin_half_angle,
                                    cos_half_angle: *cos_half_angle,
                                    x_axis: *x_axis,
                                };
                                tessellate_conical_face(
                                    shifted_point.to_array(),
                                    axis_unit,
                                    eff_radius,
                                    *sin_half_angle,
                                    *cos_half_angle,
                                    face_sense,
                                    &offset_kind,
                                    &loops_3d,
                                    chord_error,
                                    &mut result,
                                );
                            }
                        }
                        _ => {
                            let offset_kind = SurfaceKind::Offset {
                                basis_surface: *basis_surface,
                                offset: *offset,
                            };
                            let mut tessellated = false;
                            if all_loops_have_explicit_uv
                                && !explicit_uv_loops.is_empty()
                                && explicit_uv_loops.len() == loops_3d.len()
                            {
                                let (u_min, u_max, v_min, v_max) =
                                    explicit_uv_loops.iter().flatten().fold(
                                        (
                                            f64::INFINITY,
                                            f64::NEG_INFINITY,
                                            f64::INFINITY,
                                            f64::NEG_INFINITY,
                                        ),
                                        |(u_min, u_max, v_min, v_max), uv| {
                                            (
                                                u_min.min(uv[0]),
                                                u_max.max(uv[0]),
                                                v_min.min(uv[1]),
                                                v_max.max(uv[1]),
                                            )
                                        },
                                    );
                                let u_span = u_max - u_min;
                                let v_span = v_max - v_min;
                                let maximum_parameter_span = u_span.max(v_span);
                                if maximum_parameter_span.is_finite()
                                    && maximum_parameter_span > 0.0
                                    && u_span > 0.0
                                    && v_span > 0.0
                                {
                                    let delta_u = (u_span / 16.0).max(1e-4);
                                    let delta_v = (v_span / 16.0).max(1e-4);
                                    let evaluated = try_tessellate_trimmed_surface_with_uv(
                                        &offset_kind,
                                        |_, _, _| None,
                                        |u, v| evaluate_surface(brep, &offset_kind, u, v),
                                        &loops_3d,
                                        Some(&explicit_uv_loops),
                                        [delta_u, delta_v],
                                        true,
                                        face_sense,
                                        &mut result,
                                    );
                                    if matches!(evaluated, TrimmedSurfaceResult::Tessellated) {
                                        tessellated = true;
                                    }
                                }
                            }
                            if !tessellated {
                                match &basis.kind {
                                    SurfaceKind::Sphere { center, radius, .. } => {
                                        let eff_radius = *radius + oriented_offset;
                                        if center.to_array().iter().all(|value| value.is_finite())
                                            && radius.is_finite()
                                            && *radius > 1e-12
                                            && oriented_offset.is_finite()
                                            && eff_radius.is_finite()
                                            && eff_radius > 1e-12
                                        {
                                            tessellate_spherical_face(
                                                center.to_array(),
                                                eff_radius,
                                                face_sense,
                                                &offset_kind,
                                                &loops_3d,
                                                chord_error,
                                                &mut result,
                                            );
                                        }
                                    }
                                    SurfaceKind::Torus {
                                        center,
                                        axis,
                                        major_radius,
                                        minor_radius,
                                        ..
                                    } => {
                                        let eff_minor = *minor_radius + oriented_offset;
                                        let axis_len = vec3_norm(axis.to_array());
                                        if center.to_array().iter().all(|value| value.is_finite())
                                            && major_radius.is_finite()
                                            && *major_radius > 1e-12
                                            && minor_radius.is_finite()
                                            && *minor_radius > 1e-12
                                            && axis_len.is_finite()
                                            && axis_len > 1e-12
                                            && oriented_offset.is_finite()
                                            && eff_minor.is_finite()
                                            && eff_minor > 1e-12
                                        {
                                            tessellate_toroidal_face(
                                                center.to_array(),
                                                axis.to_array(),
                                                *major_radius,
                                                eff_minor,
                                                face_sense,
                                                &offset_kind,
                                                &loops_3d,
                                                chord_error,
                                                &mut result,
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            Some(current_surf_kind @ SurfaceKind::Spun { .. }) => {
                tessellate_spun_face(
                    brep,
                    current_surf_kind,
                    effective_sense,
                    &loops_3d,
                    &mut result,
                );
            }
            Some(_) => {}
            None => {
                tessellate_generic_polygon(&loops_3d, effective_sense, &mut result);
            }
        }
        if result.triangles.len() > face_triangle_start {
            if let Some(completed) = completed_face_ids.as_deref_mut() {
                completed.insert(face.id);
            }
        }
    }

    for (triangle, normals) in result.triangles.iter_mut().zip(&mut result.normals) {
        let winding = vec3_cross(
            vec3_sub(triangle[1], triangle[0]),
            vec3_sub(triangle[2], triangle[0]),
        );
        if normals
            .iter()
            .all(|normal| vec3_dot(winding, *normal) < 0.0)
        {
            triangle.swap(1, 2);
            normals.swap(1, 2);
        }
    }

    result
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_planar_face(
    origin: [f64; 3],
    axis: [f64; 3],
    sense: bool,
    loops_3d: &[Vec<[f64; 3]>],
    result: &mut TessellatedMesh,
) {
    if loops_3d.is_empty() {
        return;
    }
    let norm = vec3_normalize(axis);
    let face_normal = if sense { norm } else { vec3_scale(norm, -1.0) };

    // Establish orthonormal 2D frame on plane
    let u_axis = if norm[2].abs() < 0.9 {
        vec3_normalize(vec3_cross(norm, [0.0, 0.0, 1.0]))
    } else {
        vec3_normalize(vec3_cross(norm, [1.0, 0.0, 0.0]))
    };
    let v_axis = vec3_cross(norm, u_axis);

    let project = |p: &[f64; 3]| -> [f64; 2] {
        let d = vec3_sub(*p, origin);
        [vec3_dot(d, u_axis), vec3_dot(d, v_axis)]
    };

    let loops_2d: Vec<Vec<[f64; 2]>> = loops_3d
        .iter()
        .map(|ring| ring.iter().map(project).collect())
        .collect();

    if loops_2d.len() == 1 {
        // Single loop: 2D ear clipping
        let poly_2d = &loops_2d[0];
        let poly_3d = &loops_3d[0];
        let tri_indices = triangulate_polygon_2d(poly_2d);
        let area = signed_area_2d(poly_2d);

        for [i0, i1, i2] in tri_indices {
            let (p0, p1, p2) = if (area > 0.0) == sense {
                (poly_3d[i0], poly_3d[i1], poly_3d[i2])
            } else {
                (poly_3d[i0], poly_3d[i2], poly_3d[i1])
            };
            result.triangles.push([p0, p1, p2]);
            result.normals.push([face_normal, face_normal, face_normal]);
        }
    } else {
        // Multi-loop face with holes: Constrained Delaunay Triangulation
        // Find outer loop (max absolute area)
        let mut outer_idx = 0;
        let mut max_area = 0.0;
        for (i, ring) in loops_2d.iter().enumerate() {
            let a = signed_area_2d(ring).abs();
            if a > max_area {
                max_area = a;
                outer_idx = i;
            }
        }

        let outer_2d = &loops_2d[outer_idx];
        let mut holes_2d: Vec<&[[f64; 2]]> = Vec::new();
        for (i, ring) in loops_2d.iter().enumerate() {
            if i != outer_idx {
                holes_2d.push(ring.as_slice());
            }
        }

        let [min_x, max_x, min_y, max_y] = loops_2d.iter().flatten().fold(
            [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ],
            |[min_x, max_x, min_y, max_y], [x, y]| {
                [min_x.min(*x), max_x.max(*x), min_y.min(*y), max_y.max(*y)]
            },
        );
        let center = [min_x / 2.0 + max_x / 2.0, min_y / 2.0 + max_y / 2.0];
        let scale = (max_x - min_x).max(max_y - min_y);
        if !center.iter().all(|value| value.is_finite()) || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let to_spade = |[x, y]: [f64; 2]| [(x - center[0]) / scale, (y - center[1]) / scale];
        let from_spade = |[x, y]: [f64; 2]| [x * scale + center[0], y * scale + center[1]];

        let mut points = Vec::new();
        let mut edges = Vec::new();

        for ring_2d in &loops_2d {
            let start = points.len();
            for &p2 in ring_2d {
                let [x, y] = to_spade(p2);
                points.push(spade::mitigate_underflow(Point2::new(x, y)));
            }
            for i in 0..ring_2d.len() {
                edges.push([start + i, start + (i + 1) % ring_2d.len()]);
            }
        }

        let mut conflict = false;
        if let Ok(cdt) = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
            points,
            edges,
            |_| conflict = true,
        ) {
            if !conflict {
                for inner_face in cdt.inner_faces() {
                    let verts = inner_face.vertices();
                    let p2_tri = [
                        from_spade([verts[0].position().x, verts[0].position().y]),
                        from_spade([verts[1].position().x, verts[1].position().y]),
                        from_spade([verts[2].position().x, verts[2].position().y]),
                    ];
                    let centroid = [
                        (p2_tri[0][0] + p2_tri[1][0] + p2_tri[2][0]) / 3.0,
                        (p2_tri[0][1] + p2_tri[1][1] + p2_tri[2][1]) / 3.0,
                    ];
                    if point_inside_polygon(centroid, outer_2d)
                        && !holes_2d.iter().any(|h| point_inside_polygon(centroid, h))
                    {
                        // Unproject to 3D
                        let p3_tri = p2_tri.map(|p2| {
                            vec3_add(
                                vec3_add(origin, vec3_scale(u_axis, p2[0])),
                                vec3_scale(v_axis, p2[1]),
                            )
                        });
                        let tri_area = signed_area_2d(&p2_tri);
                        let (p0, p1, p2) = if (tri_area > 0.0) == sense {
                            (p3_tri[0], p3_tri[1], p3_tri[2])
                        } else {
                            (p3_tri[0], p3_tri[2], p3_tri[1])
                        };
                        result.triangles.push([p0, p1, p2]);
                        result.normals.push([face_normal, face_normal, face_normal]);
                    }
                }
            }
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "Exact coordinate equality preserves the cylindrical tessellator's source vertex identity checks."
)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The sanitized chord error and delta-theta clamp bound ceil(2π / delta_theta) to 12..=128 before conversion."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_cylindrical_face(
    point: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    face_sense: bool,
    surf_kind: &SurfaceKind,
    loops_3d: &[Vec<[f64; 3]>],
    chord_error: f64,
    result: &mut TessellatedMesh,
) {
    let effective_radius = radius.max(0.1);
    let sagitta = (chord_error / effective_radius).min(0.999);
    let delta_theta = (2.0 * (1.0 - sagitta).acos())
        .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
    let n_u = ((2.0 * std::f64::consts::PI) / delta_theta).ceil() as usize;
    let n_u = n_u.clamp(16, 64);

    let ax = vec3_normalize(axis);
    let x_raw = match surf_kind {
        SurfaceKind::Cylinder { x_axis, .. } => x_axis.to_array(),
        _ => [1.0, 0.0, 0.0],
    };
    let x_proj = vec3_sub(x_raw, vec3_scale(ax, vec3_dot(x_raw, ax)));
    let x_axis = if vec3_norm(x_proj) > 1e-12 {
        vec3_normalize(x_proj)
    } else {
        let alt = if ax[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        vec3_normalize(vec3_cross(ax, alt))
    };
    let y_axis = vec3_cross(ax, x_axis);

    let project_uv = |p: [f64; 3], _ring: &[[f64; 3]], _i: usize| -> Option<[f64; 2]> {
        let offset = vec3_sub(p, point);
        let axial_parameter = vec3_dot(offset, ax);
        let radial = vec3_sub(offset, vec3_scale(ax, axial_parameter));
        let x_coordinate = vec3_dot(radial, x_axis);
        let y_coordinate = vec3_dot(radial, y_axis);
        let angle = y_coordinate.atan2(x_coordinate);
        Some([angle, axial_parameter])
    };

    match try_tessellate_trimmed_surface(
        surf_kind,
        project_uv,
        |u, v| evaluate_surface_kind(surf_kind, u, v),
        loops_3d,
        delta_theta,
        false,
        face_sense,
        result,
    ) {
        TrimmedSurfaceResult::Tessellated | TrimmedSurfaceResult::Rejected => return,
        TrimmedSurfaceResult::NonContractibleBandU | TrimmedSurfaceResult::NonContractibleBandV => {
        }
    }

    // Fallback: emit_isoparametric_grid
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for ring in loops_3d {
        for p in ring {
            let v = vec3_dot(vec3_sub(*p, point), ax);
            if v < min_val {
                min_val = v;
            }
            if v > max_val {
                max_val = v;
            }
        }
    }

    if min_val.is_finite() && max_val.is_finite() && max_val - min_val > 1e-6 {
        let n_v = 2;
        emit_isoparametric_grid(
            surf_kind,
            0.0,
            2.0 * std::f64::consts::PI,
            n_u,
            min_val,
            max_val,
            n_v,
            face_sense,
            result,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::unnecessary_wraps,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn project_cone_uv(
    point_on_ring: [f64; 3],
    ring: &[[f64; 3]],
    idx: usize,
    point: [f64; 3],
    ax: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    xt_parameterization: bool,
) -> Option<[f64; 2]> {
    let offset = vec3_sub(point_on_ring, point);
    let axial = vec3_dot(offset, ax);
    let v_parameter = if xt_parameterization { -axial } else { axial };
    let radial = vec3_sub(offset, vec3_scale(ax, axial));
    let rad_norm = vec3_norm(radial);

    let (x_c, y_c) = if rad_norm >= 1e-7 {
        (vec3_dot(radial, x_axis), vec3_dot(radial, y_axis))
    } else {
        // Vertex is at the cone apex. Determine u from adjacent non-apex point(s) in ring.
        let mut found_rad = None;
        if !ring.is_empty() {
            let ring_len = ring.len();
            // If at index 0, prefer outgoing edge neighbor P_1 so unwrapping starts aligned
            let (first_check, second_check) = if idx == 0 {
                (1 % ring_len, (ring_len - 1) % ring_len)
            } else {
                ((idx + ring_len - 1) % ring_len, (idx + 1) % ring_len)
            };

            let d_first = vec3_sub(ring[first_check], point);
            let v_first = vec3_dot(d_first, ax);
            let rad_first = vec3_sub(d_first, vec3_scale(ax, v_first));
            if vec3_norm(rad_first) >= 1e-7 {
                found_rad = Some(rad_first);
            } else {
                let d_second = vec3_sub(ring[second_check], point);
                let v_second = vec3_dot(d_second, ax);
                let rad_second = vec3_sub(d_second, vec3_scale(ax, v_second));
                if vec3_norm(rad_second) >= 1e-7 {
                    found_rad = Some(rad_second);
                } else {
                    for (k, other_p) in ring.iter().enumerate() {
                        if k == idx || k == first_check || k == second_check {
                            continue;
                        }
                        let d_k = vec3_sub(*other_p, point);
                        let v_k = vec3_dot(d_k, ax);
                        let rad_k = vec3_sub(d_k, vec3_scale(ax, v_k));
                        if vec3_norm(rad_k) >= 1e-7 {
                            found_rad = Some(rad_k);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(rn) = found_rad {
            (vec3_dot(rn, x_axis), vec3_dot(rn, y_axis))
        } else {
            (1.0, 0.0)
        }
    };

    let angle = y_c.atan2(x_c);
    Some([angle, v_parameter])
}

#[allow(
    clippy::too_many_arguments,
    clippy::unnecessary_wraps,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn project_sphere_uv(
    point_on_ring: [f64; 3],
    ring: &[[f64; 3]],
    idx: usize,
    center: [f64; 3],
    radius: f64,
    z_axis: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
) -> Option<[f64; 2]> {
    let offset = vec3_sub(point_on_ring, center);
    let z_val = vec3_dot(offset, z_axis);
    let sin_v = (z_val / radius).clamp(-1.0, 1.0);
    let latitude_parameter = sin_v.asin();
    let x_val = vec3_dot(offset, x_axis);
    let y_val = vec3_dot(offset, y_axis);
    let r_xy_sq = x_val * x_val + y_val * y_val;

    let (u_x, u_y) = if r_xy_sq >= 1e-14 {
        (x_val, y_val)
    } else {
        // Vertex is at the pole (North or South). Determine u from adjacent point(s) in ring.
        let mut found_xy = None;
        if !ring.is_empty() {
            let ring_len = ring.len();
            let (first_check, second_check) = if idx == 0 {
                (1 % ring_len, (ring_len - 1) % ring_len)
            } else {
                ((idx + ring_len - 1) % ring_len, (idx + 1) % ring_len)
            };

            let d_first = vec3_sub(ring[first_check], center);
            let xf = vec3_dot(d_first, x_axis);
            let yf = vec3_dot(d_first, y_axis);
            if xf * xf + yf * yf >= 1e-14 {
                found_xy = Some((xf, yf));
            } else {
                let d_second = vec3_sub(ring[second_check], center);
                let xs = vec3_dot(d_second, x_axis);
                let ys = vec3_dot(d_second, y_axis);
                if xs * xs + ys * ys >= 1e-14 {
                    found_xy = Some((xs, ys));
                } else {
                    for (k, other_p) in ring.iter().enumerate() {
                        if k == idx || k == first_check || k == second_check {
                            continue;
                        }
                        let d_k = vec3_sub(*other_p, center);
                        let xk = vec3_dot(d_k, x_axis);
                        let yk = vec3_dot(d_k, y_axis);
                        if xk * xk + yk * yk >= 1e-14 {
                            found_xy = Some((xk, yk));
                            break;
                        }
                    }
                }
            }
        }

        if let Some((ux, uy)) = found_xy {
            (ux, uy)
        } else {
            (1.0, 0.0)
        }
    };

    let longitude_parameter = u_y.atan2(u_x);
    Some([longitude_parameter, latitude_parameter])
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The sanitized chord error and delta-theta clamp bound ceil(2π / delta_theta) to 12..=128 before conversion."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_conical_face(
    point: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    _sin_half_angle: f64,
    _cos_half_angle: f64,
    face_sense: bool,
    surf_kind: &SurfaceKind,
    loops_3d: &[Vec<[f64; 3]>],
    chord_error: f64,
    result: &mut TessellatedMesh,
) {
    if let SurfaceKind::XtCone {
        point,
        axis,
        radius,
        sin_half_angle,
        cos_half_angle,
        ..
    } = surf_kind
    {
        if !xt_cone_loops_match_support(
            point.to_array(),
            axis.to_array(),
            *radius,
            *sin_half_angle,
            *cos_half_angle,
            loops_3d,
        ) {
            return;
        }
    }

    let r = radius.max(0.1);
    let sagitta = (chord_error / r).min(0.999);
    let delta_theta = (2.0 * (1.0 - sagitta).acos())
        .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
    let n_u = ((2.0 * std::f64::consts::PI) / delta_theta).ceil() as usize;
    let n_u = n_u.clamp(16, 64);
    let ax = vec3_normalize(axis);

    let x_raw = match surf_kind {
        SurfaceKind::Cone { x_axis, .. } | SurfaceKind::XtCone { x_axis, .. } => x_axis.to_array(),
        _ => [1.0, 0.0, 0.0],
    };
    let x_proj = vec3_sub(x_raw, vec3_scale(ax, vec3_dot(x_raw, ax)));
    let x = if vec3_norm(x_proj) > 1e-12 {
        vec3_normalize(x_proj)
    } else {
        let alt = if ax[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        vec3_normalize(vec3_cross(ax, alt))
    };
    let y = vec3_cross(ax, x);

    let project_uv = |p: [f64; 3], ring: &[[f64; 3]], idx: usize| -> Option<[f64; 2]> {
        project_cone_uv(
            p,
            ring,
            idx,
            point,
            ax,
            x,
            y,
            matches!(surf_kind, SurfaceKind::XtCone { .. }),
        )
    };

    match try_tessellate_trimmed_surface(
        surf_kind,
        project_uv,
        |u, v| evaluate_surface_kind(surf_kind, u, v),
        loops_3d,
        delta_theta,
        false,
        face_sense,
        result,
    ) {
        TrimmedSurfaceResult::Tessellated | TrimmedSurfaceResult::Rejected => return,
        TrimmedSurfaceResult::NonContractibleBandU | TrimmedSurfaceResult::NonContractibleBandV => {
        }
    }

    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for ring in loops_3d {
        for p in ring {
            let axial = vec3_dot(vec3_sub(*p, point), ax);
            let v = if matches!(surf_kind, SurfaceKind::XtCone { .. }) {
                -axial
            } else {
                axial
            };
            if v < min_val {
                min_val = v;
            }
            if v > max_val {
                max_val = v;
            }
        }
    }

    if min_val.is_finite() && max_val.is_finite() && max_val - min_val > 1e-6 {
        emit_isoparametric_grid(
            surf_kind,
            0.0,
            2.0 * std::f64::consts::PI,
            n_u,
            min_val,
            max_val,
            4,
            face_sense,
            result,
        );
    }
}

fn xt_cone_loops_match_support(
    point: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    sin_half_angle: f64,
    cos_half_angle: f64,
    loops_3d: &[Vec<[f64; 3]>],
) -> bool {
    let axis_len = vec3_norm(axis);
    if !point
        .into_iter()
        .chain(axis)
        .chain([radius, sin_half_angle, cos_half_angle])
        .all(f64::is_finite)
        || !axis_len.is_finite()
        || axis_len <= 1e-12
        || cos_half_angle.abs() <= 1e-12
    {
        return false;
    }

    let axis = vec3_scale(axis, 1.0 / axis_len);
    let tan_half_angle = sin_half_angle / cos_half_angle;
    loops_3d.iter().flatten().all(|p| {
        if !p.iter().all(|value| value.is_finite()) {
            return false;
        }
        let d = vec3_sub(*p, point);
        let axial = vec3_dot(d, axis);
        let radial = vec3_sub(d, vec3_scale(axis, axial));
        let radial_len = vec3_norm(radial);
        let expected_radius = radius - axial * tan_half_angle;
        if expected_radius < 0.0 {
            return false;
        }
        let scale = p
            .iter()
            .chain(&point)
            .chain(&d)
            .copied()
            .map(f64::abs)
            .fold(
                radius
                    .abs()
                    .max(radial_len)
                    .max(expected_radius)
                    .max((axial * tan_half_angle).abs()),
                f64::max,
            );
        // This bounds floating-point arithmetic only; it is not a geometric tolerance.
        radial_len.is_finite()
            && expected_radius.is_finite()
            && (radial_len - expected_radius).abs() <= 64.0 * f64::EPSILON * scale
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The angular count is bounded by the delta-theta clamp; projected sphere UV ranges bound the grid counts before conversion."
)]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_spherical_face(
    center: [f64; 3],
    radius: f64,
    face_sense: bool,
    surf_kind: &SurfaceKind,
    loops_3d: &[Vec<[f64; 3]>],
    chord_error: f64,
    result: &mut TessellatedMesh,
) {
    let effective_radius = radius.max(0.1);
    let sagitta = (chord_error / effective_radius).min(0.999);
    let delta_theta = (2.0 * (1.0 - sagitta).acos())
        .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
    let n_u = ((2.0 * std::f64::consts::PI) / delta_theta).ceil() as usize;
    let n_u = n_u.clamp(16, 64);
    let n_v = (n_u / 2).clamp(8, 32);

    let (axis_vec, x_axis_vec) = match surf_kind {
        SurfaceKind::Sphere { axis, x_axis, .. } => (axis.to_array(), x_axis.to_array()),
        _ => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    };
    let z_axis = vec3_normalize(axis_vec);
    let x_proj = vec3_sub(x_axis_vec, vec3_scale(z_axis, vec3_dot(x_axis_vec, z_axis)));
    let x_axis = if vec3_norm(x_proj) > 1e-12 {
        vec3_normalize(x_proj)
    } else {
        let alt = if z_axis[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        vec3_normalize(vec3_cross(z_axis, alt))
    };
    let y_axis = vec3_cross(z_axis, x_axis);

    let project_uv = |point_on_ring: [f64; 3], ring: &[[f64; 3]], idx: usize| -> Option<[f64; 2]> {
        project_sphere_uv(
            point_on_ring,
            ring,
            idx,
            center,
            effective_radius,
            z_axis,
            x_axis,
            y_axis,
        )
    };

    if !loops_3d.is_empty() {
        match try_tessellate_trimmed_surface(
            surf_kind,
            project_uv,
            |longitude_parameter, latitude_parameter| {
                evaluate_surface_kind(surf_kind, longitude_parameter, latitude_parameter)
            },
            loops_3d,
            delta_theta,
            true,
            face_sense,
            result,
        ) {
            TrimmedSurfaceResult::Tessellated | TrimmedSurfaceResult::Rejected => return,
            TrimmedSurfaceResult::NonContractibleBandU
            | TrimmedSurfaceResult::NonContractibleBandV => {}
        }
    }

    let (u_min, u_max, v_min, v_max) = if loops_3d.is_empty() {
        (
            0.0,
            2.0 * std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        )
    } else {
        let mut first_uv: Option<[f64; 2]> = None;
        let mut min_u = f64::INFINITY;
        let mut max_u = f64::NEG_INFINITY;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;

        for ring in loops_3d {
            for (idx, pt) in ring.iter().enumerate() {
                if let Some(uv) = project_uv(*pt, ring, idx) {
                    if !uv[0].is_finite() || !uv[1].is_finite() {
                        continue;
                    }
                    let [u0, v0] = *first_uv.get_or_insert(uv);
                    let mut longitude_parameter = uv[0];
                    let mut diff_u = longitude_parameter - u0;
                    while diff_u > std::f64::consts::PI {
                        longitude_parameter -= 2.0 * std::f64::consts::PI;
                        diff_u = longitude_parameter - u0;
                    }
                    while diff_u < -std::f64::consts::PI {
                        longitude_parameter += 2.0 * std::f64::consts::PI;
                        diff_u = longitude_parameter - u0;
                    }

                    let mut latitude_parameter = uv[1];
                    let mut diff_v = latitude_parameter - v0;
                    while diff_v > std::f64::consts::PI {
                        latitude_parameter -= 2.0 * std::f64::consts::PI;
                        diff_v = latitude_parameter - v0;
                    }
                    while diff_v < -std::f64::consts::PI {
                        latitude_parameter += 2.0 * std::f64::consts::PI;
                        diff_v = latitude_parameter - v0;
                    }

                    min_u = min_u.min(longitude_parameter);
                    max_u = max_u.max(longitude_parameter);
                    min_v = min_v.min(latitude_parameter);
                    max_v = max_v.max(latitude_parameter);
                }
            }
        }

        if (max_u - min_u) > 1e-6 && (max_v - min_v) > 1e-6 {
            (min_u, max_u, min_v, max_v)
        } else {
            (
                0.0,
                2.0 * std::f64::consts::PI,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            )
        }
    };

    let grid_n_u = (((u_max - u_min) / delta_theta).ceil() as usize).clamp(4, n_u);
    let grid_n_v = (((v_max - v_min) / delta_theta).ceil() as usize).clamp(4, n_v);

    emit_isoparametric_grid(
        surf_kind, u_min, u_max, grid_n_u, v_min, v_max, grid_n_v, face_sense, result,
    );
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The angular counts are bounded by their delta clamps; atan2 UVs are unwrapped by at most one period, bounding grid counts before conversion."
)]
#[allow(
    clippy::many_single_char_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_toroidal_face(
    center: [f64; 3],
    axis: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    face_sense: bool,
    surf_kind: &SurfaceKind,
    loops_3d: &[Vec<[f64; 3]>],
    chord_error: f64,
    result: &mut TessellatedMesh,
) {
    let r_maj = major_radius.max(0.1);
    let r_min = minor_radius.max(0.1);
    let delta_u = (2.0 * (1.0 - (chord_error / r_maj).min(0.999)).acos())
        .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
    let delta_v = (2.0 * (1.0 - (chord_error / r_min).min(0.999)).acos())
        .clamp(std::f64::consts::PI / 64.0, std::f64::consts::PI / 6.0);
    let n_u = ((2.0 * std::f64::consts::PI) / delta_u).ceil() as usize;
    let n_v = ((2.0 * std::f64::consts::PI) / delta_v).ceil() as usize;

    let ax = vec3_normalize(axis);
    let x_raw = match surf_kind {
        SurfaceKind::Torus { x_axis, .. } => x_axis.to_array(),
        _ => [1.0, 0.0, 0.0],
    };
    let x_proj = vec3_sub(x_raw, vec3_scale(ax, vec3_dot(x_raw, ax)));
    let x = if vec3_norm(x_proj) > 1e-12 {
        vec3_normalize(x_proj)
    } else {
        let alt = if ax[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        vec3_normalize(vec3_cross(ax, alt))
    };
    let y = vec3_cross(ax, x);

    let project_uv = |p: [f64; 3], _ring: &[[f64; 3]], _i: usize| -> Option<[f64; 2]> {
        let d = vec3_sub(p, center);
        let z_val = vec3_dot(d, ax);
        let d_xy = vec3_sub(d, vec3_scale(ax, z_val));
        let x_c = vec3_dot(d_xy, x);
        let y_c = vec3_dot(d_xy, y);
        let u = y_c.atan2(x_c);
        let r_xy = (x_c * x_c + y_c * y_c).sqrt();
        let v = z_val.atan2(r_xy - major_radius);
        Some([u, v])
    };

    let band_axis = if loops_3d.is_empty() {
        None
    } else {
        match try_tessellate_trimmed_surface(
            surf_kind,
            project_uv,
            |u, v| evaluate_surface_kind(surf_kind, u, v),
            loops_3d,
            delta_u.min(delta_v),
            true,
            face_sense,
            result,
        ) {
            TrimmedSurfaceResult::Tessellated | TrimmedSurfaceResult::Rejected => return,
            TrimmedSurfaceResult::NonContractibleBandU => Some(0),
            TrimmedSurfaceResult::NonContractibleBandV => Some(1),
        }
    };

    let (u_min, u_max, v_min, v_max) = if loops_3d.is_empty() {
        (
            0.0,
            2.0 * std::f64::consts::PI,
            0.0,
            2.0 * std::f64::consts::PI,
        )
    } else {
        let mut first_uv: Option<[f64; 2]> = None;
        let mut min_u = f64::INFINITY;
        let mut max_u = f64::NEG_INFINITY;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;

        for ring in loops_3d {
            for (idx, pt) in ring.iter().enumerate() {
                if let Some(uv) = project_uv(*pt, ring, idx) {
                    if !uv[0].is_finite() || !uv[1].is_finite() {
                        continue;
                    }
                    let [u0, v0] = *first_uv.get_or_insert(uv);
                    let mut u = uv[0];
                    let mut diff_u = u - u0;
                    while diff_u > std::f64::consts::PI {
                        u -= 2.0 * std::f64::consts::PI;
                        diff_u = u - u0;
                    }
                    while diff_u < -std::f64::consts::PI {
                        u += 2.0 * std::f64::consts::PI;
                        diff_u = u - u0;
                    }

                    let mut v = uv[1];
                    let mut diff_v = v - v0;
                    while diff_v > std::f64::consts::PI {
                        v -= 2.0 * std::f64::consts::PI;
                        diff_v = v - v0;
                    }
                    while diff_v < -std::f64::consts::PI {
                        v += 2.0 * std::f64::consts::PI;
                        diff_v = v - v0;
                    }

                    min_u = min_u.min(u);
                    max_u = max_u.max(u);
                    min_v = min_v.min(v);
                    max_v = max_v.max(v);
                }
            }
        }

        if band_axis == Some(0) {
            (0.0, 2.0 * std::f64::consts::PI, min_v, max_v)
        } else if band_axis == Some(1) {
            (min_u, max_u, 0.0, 2.0 * std::f64::consts::PI)
        } else if (max_u - min_u) > 1e-6 && (max_v - min_v) > 1e-6 {
            (min_u, max_u, min_v, max_v)
        } else {
            (
                0.0,
                2.0 * std::f64::consts::PI,
                0.0,
                2.0 * std::f64::consts::PI,
            )
        }
    };

    let grid_n_u = (((u_max - u_min) / delta_u).ceil() as usize).clamp(4, n_u.clamp(16, 64));
    let grid_n_v = (((v_max - v_min) / delta_v).ceil() as usize).clamp(4, n_v.clamp(12, 48));

    emit_isoparametric_grid(
        surf_kind, u_min, u_max, grid_n_u, v_min, v_max, grid_n_v, face_sense, result,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrimmedSurfaceResult {
    Tessellated,
    Rejected,
    NonContractibleBandU,
    NonContractibleBandV,
}

#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn try_tessellate_trimmed_surface<F, E>(
    surf_kind: &SurfaceKind,
    project_uv: F,
    eval_surface: E,
    loops_3d: &[Vec<[f64; 3]>],
    delta_theta: f64,
    check_span_v: bool,
    face_sense: bool,
    result: &mut TessellatedMesh,
) -> TrimmedSurfaceResult
where
    F: Fn([f64; 3], &[[f64; 3]], usize) -> Option<[f64; 2]>,
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    try_tessellate_trimmed_surface_with_uv(
        surf_kind,
        project_uv,
        eval_surface,
        loops_3d,
        None,
        [delta_theta; 2],
        check_span_v,
        face_sense,
        result,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn try_tessellate_trimmed_surface_with_uv<F, E>(
    surf_kind: &SurfaceKind,
    project_uv: F,
    eval_surface: E,
    loops_3d: &[Vec<[f64; 3]>],
    direct_uv_loops: Option<&[Vec<[f64; 2]>]>,
    delta_theta: [f64; 2],
    check_span_v: bool,
    face_sense: bool,
    result: &mut TessellatedMesh,
) -> TrimmedSurfaceResult
where
    F: Fn([f64; 3], &[[f64; 3]], usize) -> Option<[f64; 2]>,
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    let mut sampled_depth = None;
    try_tessellate_trimmed_surface_with_uv_and_sampled_error(
        surf_kind,
        project_uv,
        eval_surface,
        loops_3d,
        direct_uv_loops,
        delta_theta,
        check_span_v,
        face_sense,
        result,
        None,
        MAX_SAMPLED_SWEEP_TRIANGLES,
        MAX_SAMPLED_SWEEP_DEPTH,
        &mut sampled_depth,
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "Periodic UV trimming and sampled-error fallback share validation state and must preserve coordinated rejection behavior."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn try_tessellate_trimmed_surface_with_uv_and_sampled_error<F, E>(
    surf_kind: &SurfaceKind,
    project_uv: F,
    eval_surface: E,
    loops_3d: &[Vec<[f64; 3]>],
    direct_uv_loops: Option<&[Vec<[f64; 2]>]>,
    delta_theta: [f64; 2],
    check_span_v: bool,
    face_sense: bool,
    result: &mut TessellatedMesh,
    max_sampled_deviation: Option<f64>,
    max_triangle_count: usize,
    max_uniform_depth: usize,
    sampled_depth_out: &mut Option<usize>,
) -> TrimmedSurfaceResult
where
    F: Fn([f64; 3], &[[f64; 3]], usize) -> Option<[f64; 2]>,
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    if loops_3d.is_empty() {
        return TrimmedSurfaceResult::Rejected;
    }

    let mut loops_2d: Vec<Vec<[f64; 2]>> = Vec::with_capacity(loops_3d.len());

    let Some(periods) = surface_periodic_periods(surf_kind) else {
        return TrimmedSurfaceResult::Rejected;
    };
    let mut periodic_wraps = Vec::new();
    let mut cone_periodic_band_used = false;
    if let Some(direct_uv_loops) = direct_uv_loops {
        if direct_uv_loops.len() != loops_3d.len()
            || direct_uv_loops
                .iter()
                .any(|ring| ring.len() < 3 || ring.iter().flatten().any(|value| !value.is_finite()))
        {
            return TrimmedSurfaceResult::Rejected;
        }
    }
    for (loop_index, ring_3d) in loops_3d.iter().enumerate() {
        if ring_3d.len() < 3 {
            return TrimmedSurfaceResult::Rejected;
        }
        let raw_uv = if let Some(direct_uv_loops) = direct_uv_loops {
            direct_uv_loops[loop_index].clone()
        } else {
            let mut projected = Vec::with_capacity(ring_3d.len());
            for (i, p) in ring_3d.iter().enumerate() {
                let Some(uv) = project_uv(*p, ring_3d, i) else {
                    return TrimmedSurfaceResult::Rejected;
                };
                if !uv[0].is_finite() || !uv[1].is_finite() {
                    return TrimmedSurfaceResult::Rejected;
                }
                projected.push(uv);
            }
            projected
        };

        // Unwrap periodic coordinates to one local chart before testing the loop.
        let mut unwrapped = Vec::with_capacity(raw_uv.len());
        unwrapped.push(raw_uv[0]);
        let mut prev = raw_uv[0];
        for &curr in &raw_uv[1..] {
            let mut point = curr;
            for axis in 0..2 {
                if let Some(period) = periods[axis] {
                    if axis == 0 || check_span_v {
                        let Some(value) =
                            unwrap_periodic_to_nearest(point[axis], prev[axis], period)
                        else {
                            return TrimmedSurfaceResult::Rejected;
                        };
                        point[axis] = value;
                    }
                }
            }
            unwrapped.push(point);
            prev = point;
        }

        // Check closure: nonzero winding remains unsupported except for known poles.
        let mut wrap = [0.0; 2];
        if let Some(period) = periods[0] {
            let has_apex_or_pole = if direct_uv_loops.is_some() {
                false
            } else {
                match surf_kind {
                    SurfaceKind::Cone { axis, point, .. }
                    | SurfaceKind::XtCone { axis, point, .. } => {
                        let ax = vec3_normalize(axis.to_array());
                        let p0 = point.to_array();
                        ring_3d.iter().any(|pt| {
                            let d = vec3_sub(*pt, p0);
                            let v = vec3_dot(d, ax);
                            let radial = vec3_sub(d, vec3_scale(ax, v));
                            vec3_norm(radial) < 1e-7
                        })
                    }
                    SurfaceKind::Sphere { center, axis, .. } => {
                        let c0 = center.to_array();
                        let z = vec3_normalize(axis.to_array());
                        ring_3d.iter().any(|pt| {
                            let d = vec3_sub(*pt, c0);
                            let z_val = vec3_dot(d, z);
                            let d_xy = vec3_sub(d, vec3_scale(z, z_val));
                            vec3_norm(d_xy) < 1e-7
                        })
                    }
                    _ => false,
                }
            };

            if !has_apex_or_pole {
                let Some(close_diff_u) = periodic_delta_for(unwrapped[0][0] - prev[0], period)
                else {
                    return TrimmedSurfaceResult::Rejected;
                };
                let total_wrap_u = (prev[0] + close_diff_u) - unwrapped[0][0];
                wrap[0] = total_wrap_u;
            }
        }

        if let Some(period) = periods[1].filter(|_| check_span_v) {
            let Some(close_diff_v) = periodic_delta_for(unwrapped[0][1] - prev[1], period) else {
                return TrimmedSurfaceResult::Rejected;
            };
            let total_wrap_v = (prev[1] + close_diff_v) - unwrapped[0][1];
            wrap[1] = total_wrap_v;
        }
        if wrap[0].abs() > 1e-3 || wrap[1].abs() > 1e-3 {
            periodic_wraps.push(wrap);
        }

        // Clean duplicate consecutive vertices.
        let mut cleaned_uv: Vec<[f64; 2]> = Vec::with_capacity(unwrapped.len());
        for pt in unwrapped {
            if let Some(last) = cleaned_uv.last() {
                let du = pt[0] - last[0];
                let dv = pt[1] - last[1];
                if if max_sampled_deviation.is_some() {
                    du != 0.0 || dv != 0.0
                } else {
                    du * du + dv * dv > 1e-12
                } {
                    cleaned_uv.push(pt);
                }
            } else {
                cleaned_uv.push(pt);
            }
        }
        if let [first, .., last] = cleaned_uv.as_slice() {
            let du = first[0] - last[0];
            let dv = first[1] - last[1];
            if if max_sampled_deviation.is_some() {
                du == 0.0 && dv == 0.0
            } else {
                du * du + dv * dv < 1e-12
            } {
                cleaned_uv.pop();
            }
        }

        if cleaned_uv.len() >= 3 {
            loops_2d.push(cleaned_uv);
        } else if max_sampled_deviation.is_some() {
            return TrimmedSurfaceResult::Rejected;
        }
    }

    if !periodic_wraps.is_empty() {
        if matches!(surf_kind, SurfaceKind::Torus { .. }) {
            return TrimmedSurfaceResult::Rejected;
        }
        let band_axis = if loops_3d.len() == 2 && loops_2d.len() == 2 {
            let period = periodic_wraps.first().and_then(|wrap| {
                (0..2).find_map(|axis| (wrap[axis].abs() > 1e-3).then_some(periods[axis]).flatten())
            });
            period.and_then(|period| is_simple_periodic_band(&loops_2d, &periodic_wraps, period))
        } else {
            None
        };
        match band_axis {
            Some(0) => return TrimmedSurfaceResult::NonContractibleBandU,
            Some(1) => return TrimmedSurfaceResult::NonContractibleBandV,
            _ if matches!(surf_kind, SurfaceKind::Cone { .. }) => {
                let Some(period) = periods[0] else {
                    return TrimmedSurfaceResult::Rejected;
                };
                let Some(polygon) = cone_periodic_band_polygon(&loops_2d, &periodic_wraps, period)
                else {
                    return TrimmedSurfaceResult::Rejected;
                };
                loops_2d = vec![polygon];
                periodic_wraps.clear();
                cone_periodic_band_used = true;
            }
            _ => return TrimmedSurfaceResult::Rejected,
        }
    }
    if loops_2d.is_empty() {
        return TrimmedSurfaceResult::Rejected;
    }

    // Find outer loop (max absolute area)
    let mut outer_idx = 0;
    let mut max_area = 0.0;
    for (i, ring) in loops_2d.iter().enumerate() {
        let a = signed_area_2d(ring).abs();
        if a > max_area {
            max_area = a;
            outer_idx = i;
        }
    }
    if !max_area.is_finite()
        || max_area == 0.0
        || (max_sampled_deviation.is_none() && max_area < 1e-12)
    {
        return TrimmedSurfaceResult::Rejected;
    }

    if loops_2d.iter().map(Vec::len).sum::<usize>() > 2048 {
        return TrimmedSurfaceResult::Rejected;
    }
    let outer_2d = loops_2d[outer_idx].clone();
    for (i, ring) in loops_2d.iter_mut().enumerate() {
        if i == outer_idx {
            continue;
        }
        let Some(aligned) = align_periodic_hole(ring, &outer_2d, periods) else {
            return TrimmedSurfaceResult::Rejected;
        };
        *ring = aligned;
    }
    if max_sampled_deviation.is_none() && loops_have_crossing_constraints(&loops_2d) {
        return TrimmedSurfaceResult::Rejected;
    }
    if loops_have_nested_holes(&loops_2d, outer_idx) {
        return TrimmedSurfaceResult::Rejected;
    }

    let mut raw_triangles_2d = Vec::new();
    if loops_2d.len() == 1 && max_sampled_deviation.is_none() && !cone_periodic_band_used {
        let poly_2d = &loops_2d[0];
        let tri_indices = triangulate_polygon_2d(poly_2d);
        for [i0, i1, i2] in tri_indices {
            raw_triangles_2d.push([poly_2d[i0], poly_2d[i1], poly_2d[i2]]);
        }
    }

    if raw_triangles_2d.is_empty() {
        // Multi-loop face with holes (or fallback if ear-clipping returned empty)
        let outer_2d = &loops_2d[outer_idx];
        let mut holes_2d: Vec<&[[f64; 2]]> = Vec::new();
        for (i, ring) in loops_2d.iter().enumerate() {
            if i != outer_idx {
                holes_2d.push(ring.as_slice());
            }
        }

        let mut points = Vec::new();
        let mut edges = Vec::new();

        for ring in &loops_2d {
            let start = points.len();
            for p2 in ring {
                points.push(Point2::new(p2[0], p2[1]));
            }
            for i in 0..ring.len() {
                edges.push([start + i, start + (i + 1) % ring.len()]);
            }
        }

        let mut conflict = false;
        if let Ok(cdt) = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
            points,
            edges,
            |_| conflict = true,
        ) {
            if !conflict {
                for inner_face in cdt.inner_faces() {
                    let verts = inner_face.vertices();
                    let p2_tri = [
                        [verts[0].position().x, verts[0].position().y],
                        [verts[1].position().x, verts[1].position().y],
                        [verts[2].position().x, verts[2].position().y],
                    ];
                    let centroid = [
                        (p2_tri[0][0] + p2_tri[1][0] + p2_tri[2][0]) / 3.0,
                        (p2_tri[0][1] + p2_tri[1][1] + p2_tri[2][1]) / 3.0,
                    ];
                    if point_inside_polygon(centroid, outer_2d)
                        && !holes_2d.iter().any(|h| point_inside_polygon(centroid, h))
                    {
                        raw_triangles_2d.push(p2_tri);
                    }
                }
            }
        }
    }

    if raw_triangles_2d.is_empty() {
        return TrimmedSurfaceResult::Rejected;
    }
    if max_sampled_deviation.is_some() {
        let expected_uv_area = loops_2d
            .iter()
            .enumerate()
            .map(|(index, ring)| {
                let area = signed_area_2d(ring).abs();
                if index == outer_idx { area } else { -area }
            })
            .sum::<f64>();
        let triangulated_uv_area = raw_triangles_2d
            .iter()
            .map(|triangle| signed_area_2d(triangle).abs())
            .sum::<f64>();
        if !expected_uv_area.is_finite()
            || !triangulated_uv_area.is_finite()
            || expected_uv_area <= 0.0
            || (triangulated_uv_area - expected_uv_area).abs() > expected_uv_area * 1e-9
        {
            return TrimmedSurfaceResult::Rejected;
        }
    }

    let subdivided_triangles_2d = if let Some(max_deviation) = max_sampled_deviation {
        let raw_triangles_2d = raw_triangles_2d
            .into_iter()
            .filter(|triangle| signed_area_2d(triangle) != 0.0)
            .collect::<Vec<_>>();
        let Some((triangles, depth)) = uniformly_refine_triangles_by_sampled_error(
            &raw_triangles_2d,
            &eval_surface,
            max_deviation,
            max_triangle_count,
            max_uniform_depth,
        ) else {
            return TrimmedSurfaceResult::Rejected;
        };
        *sampled_depth_out = Some(depth);
        triangles
    } else {
        // Subdivide triangles whose angular span in u (or v) exceeds delta_theta.
        let check_v = check_span_v || matches!(surf_kind, SurfaceKind::Nurbs(_));
        let Some(triangles) =
            subdivide_triangles_uv(&raw_triangles_2d, delta_theta, check_v, max_triangle_count)
        else {
            return TrimmedSurfaceResult::Rejected;
        };
        triangles
    };

    let mut staged_triangles = Vec::new();
    let mut staged_normals = Vec::new();
    let mut tri_count = 0;
    for tri in subdivided_triangles_2d {
        let area_uv = signed_area_2d(&tri);
        if area_uv == 0.0 {
            if max_sampled_deviation.is_some() {
                return TrimmedSurfaceResult::Rejected;
            }
            continue;
        }
        if max_sampled_deviation.is_none() && area_uv.abs() < 1e-12 {
            continue;
        }
        let Some(eval0) = eval_surface(tri[0][0], tri[0][1]) else {
            return TrimmedSurfaceResult::Rejected;
        };
        let Some(eval1) = eval_surface(tri[1][0], tri[1][1]) else {
            return TrimmedSurfaceResult::Rejected;
        };
        let Some(eval2) = eval_surface(tri[2][0], tri[2][1]) else {
            return TrimmedSurfaceResult::Rejected;
        };

        let (p0, p1, p2, mut n0, mut n1, mut n2) = if (area_uv > 0.0) == face_sense {
            (
                eval0.point,
                eval1.point,
                eval2.point,
                eval0.normal,
                eval1.normal,
                eval2.normal,
            )
        } else {
            (
                eval0.point,
                eval2.point,
                eval1.point,
                eval0.normal,
                eval2.normal,
                eval1.normal,
            )
        };

        if !face_sense {
            n0 = vec3_scale(n0, -1.0);
            n1 = vec3_scale(n1, -1.0);
            n2 = vec3_scale(n2, -1.0);
        }

        if !p0
            .iter()
            .chain(&p1)
            .chain(&p2)
            .chain(&n0)
            .chain(&n1)
            .chain(&n2)
            .all(|v| v.is_finite())
        {
            return TrimmedSurfaceResult::Rejected;
        }

        let d10 = vec3_sub(p1, p0);
        let d20 = vec3_sub(p2, p0);
        let cross = vec3_cross(d10, d20);
        let area_3d_sq = cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2];
        let edge_product = vec3_dot(d10, d10) * vec3_dot(d20, d20);
        let relative_degeneracy = if max_sampled_deviation.is_some() {
            vec3_cross_roundoff_bound_squared(d10, d20)
        } else {
            (256.0 * f64::EPSILON).powi(2) * edge_product
        };
        if !area_3d_sq.is_finite() || !edge_product.is_finite() {
            return TrimmedSurfaceResult::Rejected;
        }
        if area_3d_sq <= relative_degeneracy {
            if max_sampled_deviation.is_some() {
                return TrimmedSurfaceResult::Rejected;
            }
            continue;
        }

        staged_triangles.push([p0, p1, p2]);
        staged_normals.push([n0, n1, n2]);
        tri_count += 1;
    }

    if tri_count == 0 {
        return TrimmedSurfaceResult::Rejected;
    }
    result.triangles.extend(staged_triangles);
    result.normals.extend(staged_normals);
    TrimmedSurfaceResult::Tessellated
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Ring lengths above 2^53 may lose precision in the floating-point average."
)]
fn align_periodic_hole(
    hole: &[[f64; 2]],
    outer: &[[f64; 2]],
    periods: [Option<f64>; 2],
) -> Option<Vec<[f64; 2]>> {
    let center = |ring: &[[f64; 2]], axis: usize| {
        ring.iter().map(|p| p[axis]).sum::<f64>() / ring.len() as f64
    };
    let outer_u = center(outer, 0);
    let hole_u = center(hole, 0);
    let outer_v = center(outer, 1);
    let hole_v = center(hole, 1);
    if [outer_u, hole_u, outer_v, hole_v]
        .iter()
        .any(|value| value.abs() * f64::EPSILON > 1e-9)
    {
        return None;
    }
    let base = [0, 1].map(|axis| {
        periods[axis].map_or(0.0, |period| {
            (([outer_u, outer_v][axis] - [hole_u, hole_v][axis]) / period).round()
        })
    });
    for du in if periods[0].is_some() { -2..=2 } else { 0..=0 } {
        for dv in if periods[1].is_some() { -2..=2 } else { 0..=0 } {
            let shift = [
                (base[0] + f64::from(du)) * periods[0].unwrap_or(0.0),
                (base[1] + f64::from(dv)) * periods[1].unwrap_or(0.0),
            ];
            let shifted: Vec<_> = hole
                .iter()
                .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                .collect();
            let samples_inside = shifted.iter().enumerate().all(|(i, p)| {
                let next = shifted[(i + 1) % shifted.len()];
                point_inside_polygon(*p, outer)
                    && point_inside_polygon([(p[0] + next[0]) * 0.5, (p[1] + next[1]) * 0.5], outer)
            });
            if samples_inside {
                return Some(shifted);
            }
        }
    }
    None
}

fn periodic_delta_for(delta: f64, period: f64) -> Option<f64> {
    if !delta.is_finite()
        || !period.is_finite()
        || period <= 0.0
        || delta.abs() * f64::EPSILON > 1e-9
    {
        return None;
    }
    let wrapped = delta - period * (delta / period).round();
    (wrapped.is_finite() && wrapped.abs() <= period * 0.5 + 1e-9).then_some(wrapped)
}

#[cfg(test)]
fn surface_periodic_axes(surface: &SurfaceKind) -> (bool, bool) {
    let periods = surface_periodic_periods(surface).unwrap_or([None, None]);
    (periods[0].is_some(), periods[1].is_some())
}

fn surface_periodic_periods(surface: &SurfaceKind) -> Option<[Option<f64>; 2]> {
    let angle = Some(2.0 * std::f64::consts::PI);
    let no_period = [None, None];
    Some(match surface {
        SurfaceKind::Cylinder { .. }
        | SurfaceKind::Cone { .. }
        | SurfaceKind::XtCone { .. }
        | SurfaceKind::Sphere { .. } => [angle, None],
        SurfaceKind::Torus { .. } => [angle, angle],
        SurfaceKind::Spun { .. } => [None, angle],
        SurfaceKind::Nurbs(nurbs) => {
            let mut periods = no_period;
            if nurbs.u_periodic {
                let degree = usize::from(nurbs.u_degree);
                let count = usize::try_from(nurbs.u_control_vertex_count).ok()?;
                let expanded_count = count.checked_add(degree)?.checked_add(1)?;
                let mut expanded = Vec::with_capacity(expanded_count);
                for (&knot, &multiplicity) in nurbs.u_knots.iter().zip(&nurbs.u_knot_multiplicities)
                {
                    if !knot.is_finite()
                        || expanded.len().checked_add(usize::from(multiplicity))? > expanded_count
                    {
                        return None;
                    }
                    expanded.extend(std::iter::repeat_n(knot, usize::from(multiplicity)));
                }
                if expanded.len() != expanded_count || count >= expanded.len() || degree >= count {
                    return None;
                }
                let period = expanded[count] - expanded[degree];
                if !period.is_finite() || period <= 0.0 {
                    return None;
                }
                periods[0] = Some(period);
            }
            if nurbs.v_periodic {
                return None;
            }
            periods
        }
        _ => no_period,
    })
}

fn unwrap_periodic_to_nearest(value: f64, reference: f64, period: f64) -> Option<f64> {
    if !value.is_finite() || !reference.is_finite() {
        return None;
    }
    let unwrapped = reference + periodic_delta_for(value - reference, period)?;
    unwrapped.is_finite().then_some(unwrapped)
}

fn loops_have_crossing_constraints(loops: &[Vec<[f64; 2]>]) -> bool {
    for (loop_i, ring) in loops.iter().enumerate() {
        for edge_i in 0..ring.len() {
            let a = ring[edge_i];
            let b = ring[(edge_i + 1) % ring.len()];
            for (loop_j, other) in loops.iter().enumerate().skip(loop_i) {
                for edge_j in 0..other.len() {
                    if loop_i == loop_j
                        && (edge_i == edge_j
                            || (edge_i + 1) % ring.len() == edge_j
                            || (edge_j + 1) % ring.len() == edge_i)
                    {
                        continue;
                    }
                    if loop_i == loop_j && edge_j < edge_i {
                        continue;
                    }
                    if segments_intersect(a, b, other[edge_j], other[(edge_j + 1) % other.len()]) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    let eps = 1e-10;
    o1 * o2 <= eps
        && o3 * o4 <= eps
        && a[0].min(b[0]) - eps <= c[0].max(d[0])
        && c[0].min(d[0]) - eps <= a[0].max(b[0])
        && a[1].min(b[1]) - eps <= c[1].max(d[1])
        && c[1].min(d[1]) - eps <= a[1].max(b[1])
}

fn is_simple_periodic_band(
    loops: &[Vec<[f64; 2]>],
    wraps: &[[f64; 2]],
    period: f64,
) -> Option<usize> {
    if loops.len() != 2 || wraps.len() != 2 {
        return None;
    }
    let axis = if wraps.iter().all(|w| w[0].abs() > 1e-3 && w[1].abs() < 1e-3) {
        0
    } else if wraps.iter().all(|w| w[1].abs() > 1e-3 && w[0].abs() < 1e-3) {
        1
    } else {
        return None;
    };
    let other = 1 - axis;
    if wraps.iter().any(|w| (w[axis].abs() - period).abs() > 1e-3) {
        return None;
    }
    loops
        .iter()
        .all(|ring| {
            let min = ring.iter().map(|p| p[other]).fold(f64::INFINITY, f64::min);
            let max = ring
                .iter()
                .map(|p| p[other])
                .fold(f64::NEG_INFINITY, f64::max);
            max - min <= 1e-6
        })
        .then_some(axis)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn cone_periodic_band_polygon(
    loops: &[Vec<[f64; 2]>],
    wraps: &[[f64; 2]],
    period: f64,
) -> Option<Vec<[f64; 2]>> {
    const WRAP_TOLERANCE: f64 = 1e-3;
    const SEAM_TOLERANCE: f64 = 1e-9;

    if loops.len() != 2
        || wraps.len() != 2
        || !period.is_finite()
        || period <= 0.0
        || (period - 2.0 * std::f64::consts::PI).abs() > WRAP_TOLERANCE
        || (wraps[0][0] > 0.0) == (wraps[1][0] > 0.0)
        || wraps.iter().any(|wrap| {
            wrap.iter().any(|value| !value.is_finite())
                || (wrap[0].abs() - period).abs() > WRAP_TOLERANCE
                || wrap[1].abs() > WRAP_TOLERANCE
        })
        || loops.iter().map(Vec::len).sum::<usize>() > 2048
    {
        return None;
    }

    let mut boundaries = Vec::with_capacity(2);
    for (ring, wrap) in loops.iter().zip(wraps) {
        if ring.len() < 3 || ring.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let mut boundary = ring.clone();
        if wrap[0] < 0.0 {
            boundary.reverse();
        }
        let first = *boundary.first()?;
        let closure_u = first[0] + period;
        if !closure_u.is_finite() {
            return None;
        }
        if let Some(last) = boundary.last().copied() {
            let u_tolerance =
                64.0 * f64::EPSILON * closure_u.abs().max(first[0].abs()).max(period).max(1.0);
            let v_tolerance = 64.0 * f64::EPSILON * first[1].abs().max(last[1].abs()).max(1.0);
            if (last[0] - closure_u).abs() <= u_tolerance
                && (last[1] - first[1]).abs() <= v_tolerance
            {
                boundary.pop();
            }
        }
        if boundary.windows(2).any(|pair| pair[1][0] < pair[0][0])
            || boundary.len() < 3
            || boundary.last()?[0] > closure_u
        {
            return None;
        }
        boundaries.push(boundary);
    }

    let mut vertex_phases = boundaries
        .iter()
        .flatten()
        .map(|point| point[0].rem_euclid(period))
        .collect::<Vec<_>>();
    vertex_phases.sort_by(f64::total_cmp);
    let mut seam_phase = None;
    let mut largest_gap = 0.0;
    for index in 0..vertex_phases.len() {
        let left = vertex_phases[index];
        let right = if index + 1 == vertex_phases.len() {
            vertex_phases[0] + period
        } else {
            vertex_phases[index + 1]
        };
        let gap = right - left;
        if gap > largest_gap {
            largest_gap = gap;
            seam_phase = Some((left + gap * 0.5).rem_euclid(period));
        }
    }
    let seam_phase = seam_phase?;
    if largest_gap <= 2.0 * SEAM_TOLERANCE
        || vertex_phases.iter().any(|phase| {
            let delta = (*phase - seam_phase + period * 0.5).rem_euclid(period) - period * 0.5;
            delta.abs() <= SEAM_TOLERANCE
        })
    {
        return None;
    }

    let cut_boundary = |ring: &[[f64; 2]]| -> Option<Vec<[f64; 2]>> {
        let first = *ring.first()?;
        let turns = ((first[0] - seam_phase) / period).ceil();
        if !turns.is_finite() || turns.abs() > 1e12 {
            return None;
        }
        let seam_u = seam_phase + turns * period;
        if !(seam_u > first[0] && seam_u < first[0] + period) {
            return None;
        }

        let mut extended = ring.to_vec();
        extended.push([first[0] + period, first[1]]);
        extended.extend(
            ring.iter()
                .skip(1)
                .map(|point| [point[0] + period, point[1]]),
        );
        extended.push([first[0] + 2.0 * period, first[1]]);

        let interpolate = |target_u: f64| -> Option<[f64; 2]> {
            for pair in extended.windows(2) {
                if pair[0][0] < target_u && target_u < pair[1][0] {
                    let fraction = (target_u - pair[0][0]) / (pair[1][0] - pair[0][0]);
                    return Some([target_u, pair[0][1] + fraction * (pair[1][1] - pair[0][1])]);
                }
            }
            None
        };
        let end_u = seam_u + period;
        let start = interpolate(seam_u)?;
        let end = interpolate(end_u)?;
        let integer_shift = ((seam_phase - seam_u) / period).round();
        let shift = integer_shift * period;
        if !shift.is_finite() || (seam_u + shift - seam_phase).abs() > SEAM_TOLERANCE {
            return None;
        }
        let mut path = vec![start];
        path.extend(
            extended
                .into_iter()
                .filter(|point| point[0] > seam_u && point[0] < end_u),
        );
        path.push(end);
        for point in &mut path {
            point[0] += shift;
        }
        Some(path)
    };

    let boundary_a = cut_boundary(&boundaries[0])?;
    let boundary_b = cut_boundary(&boundaries[1])?;
    let mut polygon = boundary_a;
    polygon.extend(boundary_b.into_iter().rev());
    if polygon.len() < 3
        || polygon.len() > 2048
        || polygon
            .iter()
            .zip(polygon.iter().cycle().skip(1))
            .any(|(a, b)| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) <= 1e-24)
        || !signed_area_2d(&polygon).is_finite()
        || signed_area_2d(&polygon).abs() <= 1e-12
        || loops_have_crossing_constraints(&[polygon.clone()])
    {
        return None;
    }
    Some(polygon)
}

/// Splits a 2D UV polygon along a periodic seam line `axis = seam_coord`
/// using Sutherland-Hodgman polygon clipping into left (`<= seam`) and right (`>= seam`) parts.
#[allow(
    clippy::float_cmp,
    reason = "Floored periodic cell indices are compared exactly to preserve seam selection."
)]
pub fn split_polygon_at_periodic_seam(polygon: &[[f64; 2]], period: f64) -> Vec<Vec<[f64; 2]>> {
    if polygon.len() < 3 || !period.is_finite() || period <= 0.0 {
        return vec![polygon.to_vec()];
    }

    let min_u = polygon.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let max_u = polygon
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);

    // If the polygon spans less than period and doesn't cross any multiple of period:
    let k_min = (min_u / period).floor();
    let k_max = (max_u / period).floor();

    if k_min == k_max {
        return vec![polygon.to_vec()];
    }

    let seam = k_max * period;

    let clip_half = |poly: &[[f64; 2]], keep_left: bool| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        if poly.is_empty() {
            return out;
        }
        for i in 0..poly.len() {
            let curr = poly[i];
            let prev = poly[(i + poly.len() - 1) % poly.len()];

            let curr_in = if keep_left {
                curr[0] <= seam + 1e-9
            } else {
                curr[0] >= seam - 1e-9
            };
            let prev_in = if keep_left {
                prev[0] <= seam + 1e-9
            } else {
                prev[0] >= seam - 1e-9
            };

            if curr_in {
                if !prev_in {
                    // Entering
                    let t = (seam - prev[0]) / (curr[0] - prev[0]);
                    let v = prev[1] + t * (curr[1] - prev[1]);
                    out.push([seam, v]);
                }
                out.push(curr);
            } else if prev_in {
                // Leaving
                let t = (seam - prev[0]) / (curr[0] - prev[0]);
                let v = prev[1] + t * (curr[1] - prev[1]);
                out.push([seam, v]);
            }
        }
        out
    };

    let left = clip_half(polygon, true);
    let right = clip_half(polygon, false);

    let mut result = Vec::new();
    if left.len() >= 3 && signed_area_2d(&left).abs() > 1e-9 {
        result.push(left);
    }
    if right.len() >= 3 && signed_area_2d(&right).abs() > 1e-9 {
        result.push(right);
    }

    if result.is_empty() {
        vec![polygon.to_vec()]
    } else {
        result
    }
}

fn loops_have_nested_holes(loops: &[Vec<[f64; 2]>], outer_idx: usize) -> bool {
    let holes: Vec<_> = loops
        .iter()
        .enumerate()
        .filter_map(|(i, ring)| (i != outer_idx).then_some(ring))
        .collect();
    for i in 0..holes.len() {
        for j in i + 1..holes.len() {
            if holes[i].iter().any(|p| point_inside_polygon(*p, holes[j]))
                || holes[j].iter().any(|p| point_inside_polygon(*p, holes[i]))
            {
                return true;
            }
        }
    }
    false
}

fn split_triangle_uv(tri: [[f64; 2]; 3]) -> [[[f64; 2]; 3]; 4] {
    let [p0, p1, p2] = tri;
    let m01 = midpoint_uv(p0, p1);
    let m12 = midpoint_uv(p1, p2);
    let m20 = midpoint_uv(p2, p0);
    [
        [p0, m01, m20],
        [m01, p1, m12],
        [m20, m12, p2],
        [m01, m12, m20],
    ]
}

fn midpoint_uv(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [(left[0] + right[0]) * 0.5, (left[1] + right[1]) * 0.5]
}

fn triangle_sampled_chord_error<E>(tri: [[f64; 2]; 3], eval_surface: &E) -> Option<f64>
where
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    let points = tri.map(|[u, v]| eval_surface(u, v).map(|evaluation| evaluation.point));
    let [Some(p0), Some(p1), Some(p2)] = points else {
        return None;
    };
    let deviation = |uv: [f64; 2], expected: [f64; 3]| {
        let actual = eval_surface(uv[0], uv[1])?.point;
        let error = vec3_norm(vec3_sub(actual, expected));
        error.is_finite().then_some(error)
    };
    let center = [
        (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0,
        (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0,
    ];
    [
        deviation(
            midpoint_uv(tri[0], tri[1]),
            vec3_scale(vec3_add(p0, p1), 0.5),
        )?,
        deviation(
            midpoint_uv(tri[1], tri[2]),
            vec3_scale(vec3_add(p1, p2), 0.5),
        )?,
        deviation(
            midpoint_uv(tri[2], tri[0]),
            vec3_scale(vec3_add(p2, p0), 0.5),
        )?,
        deviation(
            center,
            vec3_scale(vec3_add(vec3_add(p0, p1), p2), 1.0 / 3.0),
        )?,
    ]
    .into_iter()
    .max_by(f64::total_cmp)
}

fn uniformly_refine_triangles_by_sampled_error<E>(
    triangles: &[[[f64; 2]; 3]],
    eval_surface: &E,
    max_deviation: f64,
    max_triangle_count: usize,
    max_depth: usize,
) -> Option<(Vec<[[f64; 2]; 3]>, usize)>
where
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    if triangles.is_empty()
        || !max_deviation.is_finite()
        || max_deviation <= 0.0
        || triangles.len() > max_triangle_count
    {
        return None;
    }
    let mut refined = triangles.to_vec();
    for depth in 0..=max_depth {
        let mut within_tolerance = true;
        for triangle in &refined {
            let error = triangle_sampled_chord_error(*triangle, eval_surface)?;
            if error > max_deviation {
                within_tolerance = false;
                break;
            }
        }
        if within_tolerance {
            return Some((refined, depth));
        }
        if depth == max_depth {
            return None;
        }
        let next_count = refined.len().checked_mul(4)?;
        if next_count > max_triangle_count {
            return None;
        }
        let mut next = Vec::with_capacity(next_count);
        for triangle in refined {
            next.extend(split_triangle_uv(triangle));
        }
        refined = next;
    }
    None
}

fn sampled_uv_wire_edges<F>(
    loops: &[Vec<[f64; 2]>],
    depth: usize,
    eval_point: F,
) -> Option<Vec<[[f64; 3]; 2]>>
where
    F: Fn(f64, f64) -> Option<[f64; 3]>,
{
    let segments = 1usize.checked_shl(u32::try_from(depth).ok()?)?;
    let mut edges = Vec::new();
    for loop_uv in loops {
        let mut cleaned = Vec::with_capacity(loop_uv.len());
        for point in loop_uv {
            if cleaned.last() != Some(point) {
                cleaned.push(*point);
            }
        }
        if cleaned.len() > 1 && cleaned.first() == cleaned.last() {
            cleaned.pop();
        }
        if cleaned.len() < 3 {
            return None;
        }
        edges.reserve(cleaned.len().checked_mul(segments)?);
        for index in 0..cleaned.len() {
            let start = cleaned[index];
            let end = cleaned[(index + 1) % cleaned.len()];
            let mut points = vec![start, end];
            for _ in 0..depth {
                let mut subdivided = Vec::with_capacity(points.len() * 2 - 1);
                for pair in points.windows(2) {
                    subdivided.push(pair[0]);
                    subdivided.push(midpoint_uv(pair[0], pair[1]));
                }
                subdivided.push(end);
                points = subdivided;
            }
            for pair in points.windows(2) {
                edges.push([
                    eval_point(pair[0][0], pair[0][1])?,
                    eval_point(pair[1][0], pair[1][1])?,
                ]);
            }
        }
    }
    Some(edges)
}

fn subdivide_triangles_uv(
    input: &[[[f64; 2]; 3]],
    delta_uv: [f64; 2],
    check_span_v: bool,
    max_triangle_count: usize,
) -> Option<Vec<[[f64; 2]; 3]>> {
    const MAX_REFINEMENT_ROUNDS: usize = 6;
    if input.is_empty() || input.len() > max_triangle_count {
        return None;
    }

    let mut triangles = input.to_vec();
    for depth in 0..=MAX_REFINEMENT_ROUNDS {
        let mut requested_edges = HashMap::new();
        for triangle in &triangles {
            if triangle_uv_exceeds_span(*triangle, delta_uv, check_span_v) {
                for edge in 0..3 {
                    let left = triangle[edge];
                    let right = triangle[(edge + 1) % 3];
                    requested_edges
                        .entry(uv_edge_key(left, right))
                        .or_insert_with(|| midpoint_uv(left, right));
                }
            }
        }
        if requested_edges.is_empty() {
            return Some(triangles);
        }
        if depth == MAX_REFINEMENT_ROUNDS {
            return None;
        }

        let mut next_count = 0usize;
        for triangle in &triangles {
            let mask = (0..3).fold(0u8, |mask, edge| {
                let key = uv_edge_key(triangle[edge], triangle[(edge + 1) % 3]);
                if requested_edges.contains_key(&key) {
                    mask | (1 << edge)
                } else {
                    mask
                }
            });
            let added = match mask.count_ones() {
                0 => 1,
                1 => 2,
                2 => 3,
                3 => 4,
                _ => unreachable!(),
            };
            next_count = next_count.checked_add(added)?;
            if next_count > max_triangle_count {
                return None;
            }
        }

        let mut next = Vec::with_capacity(next_count);
        for triangle in triangles {
            let midpoints = std::array::from_fn(|edge| {
                requested_edges
                    .get(&uv_edge_key(triangle[edge], triangle[(edge + 1) % 3]))
                    .copied()
            });
            split_triangle_uv_by_edge_mask(triangle, midpoints, &mut next);
        }
        triangles = next;
    }
    None
}

fn triangle_uv_exceeds_span(tri: [[f64; 2]; 3], delta_uv: [f64; 2], check_span_v: bool) -> bool {
    let span_u = tri[0][0].max(tri[1][0]).max(tri[2][0]) - tri[0][0].min(tri[1][0]).min(tri[2][0]);
    if check_span_v {
        let span_v =
            tri[0][1].max(tri[1][1]).max(tri[2][1]) - tri[0][1].min(tri[1][1]).min(tri[2][1]);
        span_u > delta_uv[0] || span_v > delta_uv[1]
    } else {
        span_u > delta_uv[0]
    }
}

fn uv_edge_key(left: [f64; 2], right: [f64; 2]) -> ([u64; 2], [u64; 2]) {
    fn point_key(point: [f64; 2]) -> [u64; 2] {
        point.map(|value| (if value == 0.0 { 0.0 } else { value }).to_bits())
    }
    let left_key = point_key(left);
    let right_key = point_key(right);
    if left_key > right_key {
        (right_key, left_key)
    } else {
        (left_key, right_key)
    }
}

fn split_triangle_uv_by_edge_mask(
    triangle: [[f64; 2]; 3],
    midpoints: [Option<[f64; 2]>; 3],
    out: &mut Vec<[[f64; 2]; 3]>,
) {
    let mask = midpoints
        .iter()
        .enumerate()
        .fold(0u8, |mask, (edge, midpoint)| {
            if midpoint.is_some() {
                mask | (1 << edge)
            } else {
                mask
            }
        });
    match mask.count_ones() {
        0 => out.push(triangle),
        1 => {
            let edge = mask.trailing_zeros() as usize;
            let a = triangle[edge];
            let b = triangle[(edge + 1) % 3];
            let c = triangle[(edge + 2) % 3];
            let Some(midpoint) = midpoints[edge] else {
                unreachable!("marked edge has a midpoint");
            };
            out.push([a, midpoint, c]);
            out.push([midpoint, b, c]);
        }
        2 => {
            let Some(vertex) = (0..3)
                .find(|&vertex| mask & (1 << vertex) != 0 && mask & (1 << ((vertex + 2) % 3)) != 0)
            else {
                unreachable!("two marked triangle edges share a vertex");
            };
            let previous = (vertex + 2) % 3;
            let next = (vertex + 1) % 3;
            let v0 = triangle[previous];
            let v1 = triangle[vertex];
            let v2 = triangle[next];
            let Some(midpoint_01) = midpoints[previous] else {
                unreachable!("marked edge has a midpoint");
            };
            let Some(midpoint_12) = midpoints[vertex] else {
                unreachable!("marked edge has a midpoint");
            };
            out.push([v1, midpoint_12, midpoint_01]);
            out.push([v0, midpoint_01, v2]);
            out.push([midpoint_01, midpoint_12, v2]);
        }
        3 => {
            let [Some(midpoint_01), Some(midpoint_12), Some(midpoint_20)] = midpoints else {
                unreachable!("three marked triangle edges have midpoints");
            };
            let [p0, p1, p2] = triangle;
            out.extend([
                [p0, midpoint_01, midpoint_20],
                [midpoint_01, p1, midpoint_12],
                [midpoint_20, midpoint_12, p2],
                [midpoint_01, midpoint_12, midpoint_20],
            ]);
        }
        _ => unreachable!(),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn tessellate_nurbs_face(
    surf_kind: &SurfaceKind,
    face_sense: bool,
    loops_3d: &[Vec<[f64; 3]>],
    direct_uv_loops: Option<&[Vec<[f64; 2]>]>,
    result: &mut TessellatedMesh,
) {
    let SurfaceKind::Nurbs(nurbs) = surf_kind else {
        return;
    };
    let Ok(prepared) = PreparedNurbsSurface::new(nurbs) else {
        return;
    };
    let evaluate_nurbs_point = |u, v| prepared.point(u, v).ok();
    let evaluate_nurbs_surface = |u, v| evaluate_prepared_nurbs_surface(nurbs, &prepared, u, v);
    let (first_parameter_min, first_parameter_max, second_parameter_min, second_parameter_max) =
        match surf_kind {
            SurfaceKind::Nurbs(nurbs) => (
                nurbs.u_knots.first().copied().unwrap_or(0.0),
                nurbs.u_knots.last().copied().unwrap_or(1.0),
                nurbs.v_knots.first().copied().unwrap_or(0.0),
                nurbs.v_knots.last().copied().unwrap_or(1.0),
            ),
            _ => (0.0, 1.0, 0.0, 1.0),
        };

    if !loops_3d.is_empty() {
        const GRID_STEPS: usize = 16;
        let mut coarse_samples: Vec<(f64, f64, [f64; 3])> =
            Vec::with_capacity((GRID_STEPS + 1) * (GRID_STEPS + 1));
        for j in 0_u32..=16 {
            let v = second_parameter_min
                + (second_parameter_max - second_parameter_min) * (f64::from(j) / 16.0);
            for i in 0_u32..=16 {
                let u = first_parameter_min
                    + (first_parameter_max - first_parameter_min) * (f64::from(i) / 16.0);
                if let Some(pt) = evaluate_nurbs_point(u, v) {
                    coarse_samples.push((u, v, pt));
                }
            }
        }

        let project_uv = |p: [f64; 3], _ring: &[[f64; 3]], _i: usize| -> Option<[f64; 2]> {
            let first_parameter_span = (first_parameter_max - first_parameter_min).max(1e-6);
            let second_parameter_span = (second_parameter_max - second_parameter_min).max(1e-6);
            let first_probe_step = (first_parameter_span * 1e-4).clamp(1e-7, 1e-3);
            let second_probe_step = (second_parameter_span * 1e-4).clamp(1e-7, 1e-3);
            // Nearby points on a folded surface can belong to different UV regions.
            let mut selected: Vec<(f64, f64)> = Vec::with_capacity(8);
            let mut best_projection = None;
            for _ in 0..8 {
                let closest_coarse_sample = coarse_samples
                    .iter()
                    .filter(|&&(u, v, _)| {
                        selected.iter().all(|&(selected_first, selected_second)| {
                            (u - selected_first).abs() >= first_parameter_span / 8.0
                                || (v - selected_second).abs() >= second_parameter_span / 8.0
                        })
                    })
                    .map(|&(u, v, point)| (dist_sq(&point, &p), u, v, point))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let Some((mut current_residual_squared, mut u, mut v, mut current_point)) =
                    closest_coarse_sample
                else {
                    break;
                };
                selected.push((u, v));

                for _ in 0..8 {
                    if current_residual_squared < 1e-14 {
                        break;
                    }
                    let residual_vector = vec3_sub(current_point, p);

                    let first_probe_upper = (u + first_probe_step).min(first_parameter_max);
                    let first_probe_lower = (u - first_probe_step).max(first_parameter_min);
                    let Some(u_upper_sample) = evaluate_nurbs_point(first_probe_upper, v) else {
                        break;
                    };
                    let Some(u_lower_sample) = evaluate_nurbs_point(first_probe_lower, v) else {
                        break;
                    };
                    let first_parameter_tangent = vec3_scale(
                        vec3_sub(u_upper_sample, u_lower_sample),
                        1.0 / (first_probe_upper - first_probe_lower).max(1e-12),
                    );

                    let second_probe_upper = (v + second_probe_step).min(second_parameter_max);
                    let second_probe_lower = (v - second_probe_step).max(second_parameter_min);
                    let Some(v_upper_sample) = evaluate_nurbs_point(u, second_probe_upper) else {
                        break;
                    };
                    let Some(v_lower_sample) = evaluate_nurbs_point(u, second_probe_lower) else {
                        break;
                    };
                    let second_parameter_tangent = vec3_scale(
                        vec3_sub(v_upper_sample, v_lower_sample),
                        1.0 / (second_probe_upper - second_probe_lower).max(1e-12),
                    );

                    let first_tangent_squared =
                        vec3_dot(first_parameter_tangent, first_parameter_tangent);
                    let tangent_dot_product =
                        vec3_dot(first_parameter_tangent, second_parameter_tangent);
                    let second_tangent_squared =
                        vec3_dot(second_parameter_tangent, second_parameter_tangent);
                    let first_tangent_residual_dot =
                        vec3_dot(first_parameter_tangent, residual_vector);
                    let second_tangent_residual_dot =
                        vec3_dot(second_parameter_tangent, residual_vector);
                    let det = first_tangent_squared * second_tangent_squared
                        - tangent_dot_product * tangent_dot_product;
                    if det.abs() < 1e-14 {
                        break;
                    }

                    let first_parameter_correction = -(second_tangent_squared
                        * first_tangent_residual_dot
                        - tangent_dot_product * second_tangent_residual_dot)
                        / det;
                    let second_parameter_correction = -(first_tangent_squared
                        * second_tangent_residual_dot
                        - tangent_dot_product * first_tangent_residual_dot)
                        / det;
                    let mut improved = false;
                    let mut scale = 1.0;
                    for _ in 0..7 {
                        let candidate_first_parameter = (u + scale * first_parameter_correction)
                            .clamp(first_parameter_min, first_parameter_max);
                        let candidate_second_parameter = (v + scale * second_parameter_correction)
                            .clamp(second_parameter_min, second_parameter_max);
                        if let Some(point) = evaluate_nurbs_point(
                            candidate_first_parameter,
                            candidate_second_parameter,
                        ) {
                            let candidate_residual_squared = dist_sq(&point, &p);
                            if candidate_residual_squared < current_residual_squared {
                                u = candidate_first_parameter;
                                v = candidate_second_parameter;
                                current_residual_squared = candidate_residual_squared;
                                current_point = point;
                                improved = true;
                                break;
                            }
                        }
                        scale *= 0.5;
                    }
                    if !improved
                        || (scale * first_parameter_correction).powi(2)
                            + (scale * second_parameter_correction).powi(2)
                            < 1e-14
                    {
                        break;
                    }
                }

                if best_projection.is_none_or(|(best_residual_squared, _, _)| {
                    current_residual_squared < best_residual_squared
                }) {
                    best_projection = Some((current_residual_squared, u, v));
                }
                if current_residual_squared < 1e-14 {
                    break;
                }
            }

            best_projection.map(|(_, u, v)| [u, v])
        };

        let first_parameter_cell_size = (first_parameter_max - first_parameter_min) / 16.0;
        let second_parameter_cell_size = (second_parameter_max - second_parameter_min) / 16.0;
        let max_parameter_cell_size = first_parameter_cell_size
            .max(second_parameter_cell_size)
            .max(1e-4);

        match try_tessellate_trimmed_surface_with_uv(
            surf_kind,
            project_uv,
            evaluate_nurbs_surface,
            loops_3d,
            direct_uv_loops,
            [max_parameter_cell_size; 2],
            false,
            face_sense,
            result,
        ) {
            TrimmedSurfaceResult::Tessellated
            | TrimmedSurfaceResult::Rejected
            | TrimmedSurfaceResult::NonContractibleBandU
            | TrimmedSurfaceResult::NonContractibleBandV => return,
        }
    }

    emit_isoparametric_grid_with(
        evaluate_nurbs_surface,
        first_parameter_min,
        first_parameter_max,
        16,
        second_parameter_min,
        second_parameter_max,
        16,
        face_sense,
        result,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn emit_isoparametric_grid(
    surf_kind: &SurfaceKind,
    u_min: f64,
    u_max: f64,
    n_u: usize,
    v_min: f64,
    v_max: f64,
    n_v: usize,
    face_sense: bool,
    result: &mut TessellatedMesh,
) {
    emit_isoparametric_grid_with(
        |u, v| evaluate_surface_kind(surf_kind, u, v),
        u_min,
        u_max,
        n_u,
        v_min,
        v_max,
        n_v,
        face_sense,
        result,
    );
}

#[allow(
    clippy::cast_precision_loss,
    reason = "All callers bound grid counts to at most 64 in U and 48 in V, so indices and denominators are exactly representable as f64."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn emit_isoparametric_grid_with<E>(
    eval_surface: E,
    u_min: f64,
    u_max: f64,
    n_u: usize,
    v_min: f64,
    v_max: f64,
    n_v: usize,
    face_sense: bool,
    result: &mut TessellatedMesh,
) where
    E: Fn(f64, f64) -> Option<SurfaceEvaluation>,
{
    if n_u == 0 || n_v == 0 {
        return;
    }

    let mut grid_pts = Vec::with_capacity((n_u + 1) * (n_v + 1));
    let mut grid_norms = Vec::with_capacity((n_u + 1) * (n_v + 1));

    for j in 0..=n_v {
        let v = v_min + (v_max - v_min) * (j as f64 / n_v as f64);
        for i in 0..=n_u {
            let u = u_min + (u_max - u_min) * (i as f64 / n_u as f64);
            let Some(eval) = eval_surface(u, v) else {
                return;
            };
            if !eval
                .point
                .iter()
                .chain(eval.normal.iter())
                .all(|value| value.is_finite())
            {
                return;
            }
            grid_pts.push(eval.point);
            grid_norms.push(eval.normal);
        }
    }

    for j in 0..n_v {
        for i in 0..n_u {
            let i0 = j * (n_u + 1) + i;
            let i1 = i0 + 1;
            let i2 = (j + 1) * (n_u + 1) + i;
            let i3 = i2 + 1;

            let (p0, p1, p2, p3) = (grid_pts[i0], grid_pts[i1], grid_pts[i2], grid_pts[i3]);
            let (n0, n1, n2, n3) = (
                grid_norms[i0],
                grid_norms[i1],
                grid_norms[i2],
                grid_norms[i3],
            );

            let (tri1, tri2, norm1, norm2) = if face_sense {
                ([p0, p1, p2], [p1, p3, p2], [n0, n1, n2], [n1, n3, n2])
            } else {
                (
                    [p0, p2, p1],
                    [p1, p2, p3],
                    [
                        vec3_scale(n0, -1.0),
                        vec3_scale(n2, -1.0),
                        vec3_scale(n1, -1.0),
                    ],
                    [
                        vec3_scale(n1, -1.0),
                        vec3_scale(n2, -1.0),
                        vec3_scale(n3, -1.0),
                    ],
                )
            };

            result.triangles.push(tri1);
            result.normals.push(norm1);

            result.triangles.push(tri2);
            result.normals.push(norm2);
        }
    }
}

/// Evaluates any supported surface kind within a B-Rep model, including
/// `Offset`, `Spun`, and mapped `Swept` surfaces.
#[must_use]
pub fn evaluate_surface(
    brep: &BrepModel,
    surface: &SurfaceKind,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    evaluate_surface_recursive(brep, surface, u, v, 0)
}

/// Evaluates a 3D point on any surface kind within a B-Rep model.
#[must_use]
pub fn evaluate_surface_point(
    brep: &BrepModel,
    surface: &SurfaceKind,
    u: f64,
    v: f64,
) -> Option<[f64; 3]> {
    evaluate_surface(brep, surface, u, v).map(|e| e.point)
}

/// Evaluates a mapped `X_T` offset surface whose basis is a mapped ruled sweep.
///
/// This explicit candidate checks the offset, swept-surface, and section source
/// links, requires a finite unit `X_T` sweep direction and known basis sense, and
/// uses the mapped section's bounded endpoint-extension profile. Generic
/// surface evaluation does not route through this helper.
#[must_use]
#[allow(
    clippy::float_cmp,
    reason = "Exact mapped direction equality is part of the source candidate match."
)]
pub fn evaluate_mapped_xt_swept_offset_surface_candidate(
    brep: &BrepModel,
    offset_surface: &SurfaceGeometry,
    swept: &SweptSurfaceData,
    section: &CurveGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    if brep.source_format != BrepSourceFormat::Text
        || offset_surface.source.node_type != 60
        || offset_surface.source.type_name != "OFFSET_SURF"
        || swept.source.node_type != 67
        || swept.source.type_name != "SWEPT_SURF"
        || swept.section != Some(section.source.node_index)
        || section.source.type_name != "B_CURVE"
        || !brep
            .surfaces
            .iter()
            .any(|surface| surface == offset_surface)
        || !u.is_finite()
        || !v.is_finite()
    {
        return None;
    }
    let SurfaceKind::Offset {
        basis_surface,
        offset,
    } = &offset_surface.kind
    else {
        return None;
    };
    if !offset.is_finite() {
        return None;
    }
    let basis = brep
        .surfaces
        .iter()
        .find(|surface| surface.id == *basis_surface && surface.source == swept.source)?;
    let [Some(dx), Some(dy), Some(dz)] = swept.sweep else {
        return None;
    };
    let direction = [dx, dy, dz];
    let basis_direction = match &basis.kind {
        SurfaceKind::Swept {
            section: mapped_section,
            direction,
            ..
        }
        | SurfaceKind::SweptWithSource {
            section: mapped_section,
            sweep: direction,
            ..
        } if *mapped_section == section.id => Some(direction.to_array()),
        SurfaceKind::Unsupported { type_name } if type_name == "SWEPT_SURF" => None,
        _ => return None,
    };
    if basis.source.node_index != swept.source.node_index
        || !brep.curves.iter().any(|mapped| mapped == section)
        || basis_direction.is_some_and(|basis_direction| basis_direction != direction)
        || direction.iter().any(|component| !component.is_finite())
    {
        return None;
    }
    let sense = basis.sense.multiplier()?;
    let CurveKind::Nurbs(_) = &section.kind else {
        return None;
    };
    let direction_length = direction[0].hypot(direction[1]).hypot(direction[2]);
    if !direction_length.is_finite() || (direction_length - 1.0).abs() > 64.0 * f64::EPSILON {
        return None;
    }
    let evaluation = evaluate_ruled_sweep_surface(&section.kind, direction, u, v)?;
    let point = vec3_add(
        evaluation.point,
        vec3_scale(evaluation.normal, sense * *offset),
    );
    point
        .iter()
        .all(|coordinate| coordinate.is_finite())
        .then_some(SurfaceEvaluation {
            point,
            normal: evaluation.normal,
        })
}

fn evaluate_curve_in_brep(
    brep: &BrepModel,
    curve_geom: &CurveGeometry,
    t: f64,
) -> Option<CurveEvaluation> {
    evaluate_curve_in_brep_inner(brep, curve_geom, t, 0)
}

fn evaluate_curve_in_brep_inner(
    brep: &BrepModel,
    curve_geom: &CurveGeometry,
    t: f64,
    depth: usize,
) -> Option<CurveEvaluation> {
    if depth > 8 {
        return None;
    }
    if let CurveKind::Trimmed {
        basis_curve,
        start_point,
        end_point,
        start_parameter,
        end_parameter,
    } = &curve_geom.kind
    {
        let mapped_t = if start_parameter.is_finite() && end_parameter.is_finite() {
            let min_p = start_parameter.min(*end_parameter);
            let max_p = start_parameter.max(*end_parameter);
            if (0.0..=1.0).contains(&t) && (t < min_p || t > max_p) {
                start_parameter + t * (end_parameter - start_parameter)
            } else {
                t
            }
        } else {
            t
        };
        let basis = brep.curves.iter().find(|c| c.id == *basis_curve);
        if basis.is_some_and(|basis| matches!(&basis.kind, CurveKind::Polyline(_))) {
            return None;
        }
        let eval = basis
            .and_then(|basis| evaluate_curve_in_brep_inner(brep, basis, mapped_t, depth + 1))
            .or_else(|| {
                let p0 = start_point.to_array();
                let p1 = end_point.to_array();
                let tangent = vec3_sub(p1, p0);
                let param = if (0.0..=1.0).contains(&t) { t } else { 0.5 };
                let point = vec3_add(p0, vec3_scale(tangent, param));
                Some(CurveEvaluation { point, tangent })
            });
        let mut eval = eval?;
        if curve_geom.sense == Sense::Negative {
            eval.tangent = vec3_scale(eval.tangent, -1.0);
        }
        Some(eval)
    } else {
        let mut eval = evaluate_curve(&curve_geom.kind, t)?;
        if curve_geom.sense == Sense::Negative {
            eval.tangent = vec3_scale(eval.tangent, -1.0);
        }
        Some(eval)
    }
}

fn rotate_point_around_axis(
    base: [f64; 3],
    axis: [f64; 3],
    point: [f64; 3],
    angle: f64,
) -> [f64; 3] {
    let a = vec3_normalize(axis);
    let r = vec3_sub(point, base);
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let cross_ar = vec3_cross(a, r);
    let dot_ar = vec3_dot(a, r);
    let r_rot = vec3_add(
        vec3_scale(r, cos_a),
        vec3_add(
            vec3_scale(cross_ar, sin_a),
            vec3_scale(a, dot_ar * (1.0 - cos_a)),
        ),
    );
    vec3_add(base, r_rot)
}

fn rotate_vector_around_axis(axis: [f64; 3], vec: [f64; 3], angle: f64) -> [f64; 3] {
    let a = vec3_normalize(axis);
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let cross_av = vec3_cross(a, vec);
    let dot_av = vec3_dot(a, vec);
    vec3_add(
        vec3_scale(vec, cos_a),
        vec3_add(
            vec3_scale(cross_av, sin_a),
            vec3_scale(a, dot_av * (1.0 - cos_a)),
        ),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn evaluate_surface_recursive(
    brep: &BrepModel,
    surface: &SurfaceKind,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<SurfaceEvaluation> {
    if depth > 8 || !u.is_finite() || !v.is_finite() {
        return None;
    }
    match surface {
        SurfaceKind::Offset {
            basis_surface,
            offset,
        } => {
            let basis = brep.surfaces.iter().find(|s| s.id == *basis_surface)?;
            let eval_basis = evaluate_surface_recursive(brep, &basis.kind, u, v, depth + 1)?;
            let displacement_normal = if basis.sense == Sense::Negative {
                vec3_scale(eval_basis.normal, -1.0)
            } else {
                eval_basis.normal
            };
            let pt = vec3_add(eval_basis.point, vec3_scale(displacement_normal, *offset));
            Some(SurfaceEvaluation {
                point: pt,
                normal: eval_basis.normal,
            })
        }
        SurfaceKind::Spun {
            profile,
            base,
            axis,
            start_parameter,
            end_parameter,
            ..
        } => {
            let profile_curve = brep.curves.iter().find(|c| c.id == *profile)?;
            let u_clamped = match (start_parameter, end_parameter) {
                (Some(p0), Some(p1)) => {
                    let min = p0.min(*p1);
                    let max = p0.max(*p1);
                    u.clamp(min, max)
                }
                (Some(p0), None) => u.max(*p0),
                (None, Some(p1)) => u.min(*p1),
                (None, None) => u,
            };

            let eval_pt = |param_u: f64, rot_v: f64| -> Option<[f64; 3]> {
                let curve_eval = evaluate_curve_in_brep(brep, profile_curve, param_u)?;
                Some(rotate_point_around_axis(
                    base.to_array(),
                    axis.to_array(),
                    curve_eval.point,
                    rot_v,
                ))
            };

            let curve_eval = evaluate_curve_in_brep(brep, profile_curve, u_clamped)?;
            let pt_rot =
                rotate_point_around_axis(base.to_array(), axis.to_array(), curve_eval.point, v);
            let t_rot = rotate_vector_around_axis(axis.to_array(), curve_eval.tangent, v);
            let v_tan = vec3_cross(axis.to_array(), vec3_sub(pt_rot, base.to_array()));
            let cross = vec3_cross(v_tan, t_rot);
            let cross_len = vec3_norm(cross);

            let normal = if cross_len > 1e-12 {
                vec3_scale(cross, 1.0 / cross_len)
            } else {
                // Fall back to finite difference or axis
                let eps_u = 1e-4;
                let eps_v = 1e-4;

                let u_plus = match (start_parameter, end_parameter) {
                    (Some(p0), Some(p1)) => (u_clamped + eps_u).min(p0.max(*p1)),
                    _ => u_clamped + eps_u,
                };
                let u_minus = match (start_parameter, end_parameter) {
                    (Some(p0), Some(p1)) => (u_clamped - eps_u).max(p0.min(*p1)),
                    _ => u_clamped - eps_u,
                };
                let du_step = u_plus - u_minus;
                let first_axis_forward_sample = eval_pt(u_plus, v).unwrap_or(pt_rot);
                let first_axis_backward_sample = eval_pt(u_minus, v).unwrap_or(pt_rot);
                let du = vec3_scale(
                    vec3_sub(first_axis_forward_sample, first_axis_backward_sample),
                    1.0 / du_step.max(1e-12),
                );

                let second_axis_forward_sample = eval_pt(u_clamped, v + eps_v).unwrap_or(pt_rot);
                let second_axis_backward_sample = eval_pt(u_clamped, v - eps_v).unwrap_or(pt_rot);
                let dv = vec3_scale(
                    vec3_sub(second_axis_forward_sample, second_axis_backward_sample),
                    1.0 / (2.0 * eps_v),
                );

                let fd_cross = vec3_cross(dv, du);
                let fd_len = vec3_norm(fd_cross);
                if fd_len > 1e-12 {
                    vec3_scale(fd_cross, 1.0 / fd_len)
                } else {
                    let axis_arr = axis.to_array();
                    let axis_len = vec3_norm(axis_arr);
                    if axis_len > 1e-12 {
                        vec3_scale(axis_arr, 1.0 / axis_len)
                    } else {
                        [0.0, 0.0, 1.0]
                    }
                }
            };

            Some(SurfaceEvaluation {
                point: pt_rot,
                normal,
            })
        }
        SurfaceKind::BlendBoundary {
            boundary_index,
            blend_surface,
        } => {
            let blend = brep.surfaces.iter().find(|s| s.id == *blend_surface)?;
            match &blend.kind {
                SurfaceKind::BlendedEdge {
                    supporting_surfaces,
                    ..
                } => {
                    let supp_idx = 1 - (*boundary_index as usize % 2);
                    let supp_id = supporting_surfaces[supp_idx];
                    let supp_surface = brep.surfaces.iter().find(|s| s.id == supp_id)?;
                    evaluate_surface_recursive(brep, &supp_surface.kind, u, v, depth + 1)
                }
                _ => None,
            }
        }
        SurfaceKind::Swept {
            section, direction, ..
        }
        | SurfaceKind::SweptWithSource {
            section,
            sweep: direction,
            ..
        } => {
            let mut section_geom = brep.curves.iter().find(|curve| curve.id == *section)?;
            let mut mapped_u = u;
            let mut depth_trim = 0;
            while let CurveKind::Trimmed {
                basis_curve,
                start_parameter,
                end_parameter,
                ..
            } = &section_geom.kind
            {
                if depth_trim > 8 {
                    return None;
                }
                depth_trim += 1;
                if start_parameter.is_finite() && end_parameter.is_finite() {
                    let min_p = start_parameter.min(*end_parameter);
                    let max_p = start_parameter.max(*end_parameter);
                    if (0.0..=1.0).contains(&mapped_u) && (mapped_u < min_p || mapped_u > max_p) {
                        mapped_u = start_parameter + mapped_u * (end_parameter - start_parameter);
                    }
                }
                section_geom = brep.curves.iter().find(|curve| curve.id == *basis_curve)?;
            }
            match &section_geom.kind {
                CurveKind::Nurbs(_)
                | CurveKind::Line { .. }
                | CurveKind::Circle { .. }
                | CurveKind::Ellipse { .. }
                | CurveKind::Parabola { .. }
                | CurveKind::Hyperbola { .. } => {}
                _ => return None,
            }
            let direction = direction.to_array();
            let direction_length = direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
                .sqrt();
            if !direction_length.is_finite() || (direction_length - 1.0).abs() > 64.0 * f64::EPSILON
            {
                return None;
            }
            evaluate_ruled_sweep_surface_in_brep(
                Some(brep),
                &section_geom.kind,
                direction,
                mapped_u,
                v,
            )
        }
        SurfaceKind::BlendedEdge {
            blend_type,
            supporting_surfaces,
            spine_curve,
            ranges,
            thumb_weights,
            boundary_surfaces,
            start,
            end,
        } => {
            if blend_type.as_str() != "rolling_ball" {
                return None;
            }
            evaluate_rolling_ball(
                brep,
                *supporting_surfaces,
                *spine_curve,
                *ranges,
                *thumb_weights,
                *boundary_surfaces,
                start.as_ref(),
                end.as_ref(),
                u,
                v,
            )
        }
        _ => evaluate_surface_kind(surface, u, v),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Projected-curve validation, endpoint matching, and triangle generation form one coordinated swept-face pipeline."
)]
fn tessellate_swept_face_projected(
    brep: &BrepModel,
    section_kind: &CurveKind,
    trim_range: Option<(f64, f64)>,
    direction: [f64; 3],
    face_sense: bool,
    loops_3d: &[Vec<[f64; 3]>],
    result: &mut TessellatedMesh,
) -> bool {
    let dir_len = vec3_norm(direction);
    if !dir_len.is_finite() || dir_len <= 1e-12 || loops_3d.is_empty() {
        return false;
    }
    let unit_d = vec3_scale(direction, 1.0 / dir_len);

    let mut uv_loops: Vec<Vec<[f64; 2]>> = Vec::with_capacity(loops_3d.len());

    if let CurveKind::Line {
        point,
        direction: line_dir,
    } = section_kind
    {
        let p0 = point.to_array();
        let t_vec = line_dir.to_array();
        let p0_perp = vec3_sub(p0, vec3_scale(unit_d, vec3_dot(p0, unit_d)));
        let t_perp = vec3_sub(t_vec, vec3_scale(unit_d, vec3_dot(t_vec, unit_d)));
        let t_perp_len_sq = vec3_dot(t_perp, t_perp);
        if !t_perp_len_sq.is_finite() || t_perp_len_sq <= 1e-14 {
            return false;
        }
        for loop_3d in loops_3d {
            let mut uv_ring = Vec::with_capacity(loop_3d.len());
            for &p in loop_3d {
                let loop_point_perpendicular = vec3_sub(p, vec3_scale(unit_d, vec3_dot(p, unit_d)));
                let u =
                    vec3_dot(vec3_sub(loop_point_perpendicular, p0_perp), t_perp) / t_perp_len_sq;
                let c_u = vec3_add(p0, vec3_scale(t_vec, u));
                let v = vec3_dot(vec3_sub(p, c_u), unit_d);
                if !u.is_finite() || !v.is_finite() {
                    return false;
                }
                uv_ring.push([u, v]);
            }
            uv_loops.push(uv_ring);
        }
    } else {
        let (u_min, u_max) = match trim_range {
            Some((p0, p1)) if p0.is_finite() && p1.is_finite() && (p1 - p0).abs() > 1e-12 => {
                (p0.min(p1), p0.max(p1))
            }
            _ => match section_kind {
                CurveKind::Circle { .. } | CurveKind::Ellipse { .. } => {
                    (0.0, 2.0 * std::f64::consts::PI)
                }
                CurveKind::Nurbs(nurbs) => (
                    nurbs.knots.first().copied().unwrap_or(0.0),
                    nurbs.knots.last().copied().unwrap_or(1.0),
                ),
                CurveKind::Parabola { .. } | CurveKind::Hyperbola { .. } => (-50.0, 50.0),
                _ => (0.0, 1.0),
            },
        };
        let u_span = u_max - u_min;
        if !u_span.is_finite() || u_span <= 0.0 {
            return false;
        }

        let num_coarse = 128;
        let mut coarse_samples: Vec<(f64, [f64; 3])> = Vec::with_capacity(num_coarse + 1);
        for i in 0_u32..=128 {
            let u = u_min + u_span * (f64::from(i) / 128.0);
            if let Some(pt) = crate::brep::evaluate_curve_point(section_kind, u) {
                if pt.iter().all(|c| c.is_finite()) {
                    let c_perp = vec3_sub(pt, vec3_scale(unit_d, vec3_dot(pt, unit_d)));
                    coarse_samples.push((u, c_perp));
                }
            }
        }
        if coarse_samples.is_empty() {
            return false;
        }

        for loop_3d in loops_3d {
            let mut uv_ring = Vec::with_capacity(loop_3d.len());
            for &p in loop_3d {
                let p_perp = vec3_sub(p, vec3_scale(unit_d, vec3_dot(p, unit_d)));
                let mut best_u = coarse_samples[0].0;
                let mut best_dist = dist_sq(&coarse_samples[0].1, &p_perp);
                for &(su, c_perp) in &coarse_samples[1..] {
                    let d = dist_sq(&c_perp, &p_perp);
                    if d < best_dist {
                        best_dist = d;
                        best_u = su;
                    }
                }

                // Local refinement around best_u
                let mut cur_u = best_u;
                let mut cur_dist = best_dist;
                let mut delta = (u_span / 128.0).max(1e-6);
                for _ in 0..3 {
                    let sub_min = (cur_u - delta).max(u_min);
                    let sub_max = (cur_u + delta).min(u_max);
                    let sub_steps = 16;
                    let sub_span = sub_max - sub_min;
                    for s in 0..=sub_steps {
                        let test_u = sub_min + sub_span * (f64::from(s) / f64::from(sub_steps));
                        if let Some(pt) = crate::brep::evaluate_curve_point(section_kind, test_u) {
                            let c_perp = vec3_sub(pt, vec3_scale(unit_d, vec3_dot(pt, unit_d)));
                            let d = dist_sq(&c_perp, &p_perp);
                            if d < cur_dist {
                                cur_dist = d;
                                cur_u = test_u;
                            }
                        }
                    }
                    delta = sub_span / f64::from(sub_steps);
                }

                let Some(c_u) = crate::brep::evaluate_curve_point(section_kind, cur_u) else {
                    return false;
                };
                let v = vec3_dot(vec3_sub(p, c_u), unit_d);
                if !cur_u.is_finite() || !v.is_finite() {
                    return false;
                }
                uv_ring.push([cur_u, v]);
            }
            uv_loops.push(uv_ring);
        }
    }

    if uv_loops.is_empty() {
        return false;
    }

    if uv_loops.len() == 1 {
        let poly_2d = &uv_loops[0];
        let tri_indices = triangulate_polygon_2d(poly_2d);
        let area_2d = signed_area_2d(poly_2d);
        for [i0, i1, i2] in tri_indices {
            let uv0 = poly_2d[i0];
            let uv1 = poly_2d[i1];
            let uv2 = poly_2d[i2];
            let e0 = evaluate_ruled_sweep_surface_in_brep(
                Some(brep),
                section_kind,
                direction,
                uv0[0],
                uv0[1],
            );
            let e1 = evaluate_ruled_sweep_surface_in_brep(
                Some(brep),
                section_kind,
                direction,
                uv1[0],
                uv1[1],
            );
            let e2 = evaluate_ruled_sweep_surface_in_brep(
                Some(brep),
                section_kind,
                direction,
                uv2[0],
                uv2[1],
            );
            if let (Some(e0), Some(e1), Some(e2)) = (e0, e1, e2) {
                let (p0, p1, p2, n0, n1, n2) = if (area_2d > 0.0) == face_sense {
                    (
                        e0.point, e1.point, e2.point, e0.normal, e1.normal, e2.normal,
                    )
                } else {
                    (
                        e0.point, e2.point, e1.point, e0.normal, e2.normal, e1.normal,
                    )
                };
                result.triangles.push([p0, p1, p2]);
                result.normals.push([n0, n1, n2]);
            }
        }
        true
    } else {
        let mut outer_idx = 0;
        let mut max_area = 0.0;
        for (i, ring) in uv_loops.iter().enumerate() {
            let a = signed_area_2d(ring).abs();
            if a > max_area {
                max_area = a;
                outer_idx = i;
            }
        }
        let outer_2d = &uv_loops[outer_idx];
        let mut holes_2d: Vec<&[[f64; 2]]> = Vec::new();
        for (i, ring) in uv_loops.iter().enumerate() {
            if i != outer_idx {
                holes_2d.push(ring.as_slice());
            }
        }

        let [min_x, max_x, min_y, max_y] = uv_loops.iter().flatten().fold(
            [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ],
            |[min_x, max_x, min_y, max_y], [x, y]| {
                [min_x.min(*x), max_x.max(*x), min_y.min(*y), max_y.max(*y)]
            },
        );
        let center = [min_x / 2.0 + max_x / 2.0, min_y / 2.0 + max_y / 2.0];
        let scale = (max_x - min_x).max(max_y - min_y);
        if !center.iter().all(|value| value.is_finite()) || !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        let to_spade = |[x, y]: [f64; 2]| [(x - center[0]) / scale, (y - center[1]) / scale];
        let from_spade = |[x, y]: [f64; 2]| [x * scale + center[0], y * scale + center[1]];

        let mut points = Vec::new();
        let mut edges = Vec::new();

        for ring_2d in &uv_loops {
            let start = points.len();
            for &p2 in ring_2d {
                let [x, y] = to_spade(p2);
                points.push(spade::mitigate_underflow(Point2::new(x, y)));
            }
            for i in 0..ring_2d.len() {
                edges.push([start + i, start + (i + 1) % ring_2d.len()]);
            }
        }

        let mut conflict = false;
        let mut triangulated = false;
        if let Ok(cdt) = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
            points,
            edges,
            |_| conflict = true,
        ) {
            if !conflict {
                for inner_face in cdt.inner_faces() {
                    let verts = inner_face.vertices();
                    let uv_tri = [
                        from_spade([verts[0].position().x, verts[0].position().y]),
                        from_spade([verts[1].position().x, verts[1].position().y]),
                        from_spade([verts[2].position().x, verts[2].position().y]),
                    ];
                    let centroid = [
                        (uv_tri[0][0] + uv_tri[1][0] + uv_tri[2][0]) / 3.0,
                        (uv_tri[0][1] + uv_tri[1][1] + uv_tri[2][1]) / 3.0,
                    ];
                    if point_inside_polygon(centroid, outer_2d)
                        && !holes_2d.iter().any(|h| point_inside_polygon(centroid, h))
                    {
                        let e0 = evaluate_ruled_sweep_surface_in_brep(
                            Some(brep),
                            section_kind,
                            direction,
                            uv_tri[0][0],
                            uv_tri[0][1],
                        );
                        let e1 = evaluate_ruled_sweep_surface_in_brep(
                            Some(brep),
                            section_kind,
                            direction,
                            uv_tri[1][0],
                            uv_tri[1][1],
                        );
                        let e2 = evaluate_ruled_sweep_surface_in_brep(
                            Some(brep),
                            section_kind,
                            direction,
                            uv_tri[2][0],
                            uv_tri[2][1],
                        );
                        if let (Some(e0), Some(e1), Some(e2)) = (e0, e1, e2) {
                            let tri_area = signed_area_2d(&uv_tri);
                            let (p0, p1, p2, n0, n1, n2) = if (tri_area > 0.0) == face_sense {
                                (
                                    e0.point, e1.point, e2.point, e0.normal, e1.normal, e2.normal,
                                )
                            } else {
                                (
                                    e0.point, e2.point, e1.point, e0.normal, e2.normal, e1.normal,
                                )
                            };
                            result.triangles.push([p0, p1, p2]);
                            result.normals.push([n0, n1, n2]);
                            triangulated = true;
                        }
                    }
                }
            }
        }
        triangulated
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Surface parameterization, trim handling, and normal orientation are coupled in this spun-face tessellator."
)]
fn tessellate_spun_face(
    brep: &BrepModel,
    surf_kind: &SurfaceKind,
    face_sense: bool,
    loops_3d: &[Vec<[f64; 3]>],
    result: &mut TessellatedMesh,
) -> bool {
    const GRID_STEPS_U: u32 = 16;
    const GRID_STEPS_V: u32 = 32;

    let (u_min, u_max, v_min, v_max) = match surf_kind {
        SurfaceKind::Spun {
            profile,
            start_parameter,
            end_parameter,
            ..
        } => {
            let (u0, u1) = match (start_parameter, end_parameter) {
                (Some(s0), Some(s1))
                    if s0.is_finite() && s1.is_finite() && (s1 - s0).abs() > 1e-12 =>
                {
                    (s0.min(*s1), s0.max(*s1))
                }
                (Some(_), Some(_)) => return false,
                _ => {
                    let Some(profile_curve) = brep.curves.iter().find(|c| c.id == *profile) else {
                        return false;
                    };
                    let domain = match &profile_curve.kind {
                        CurveKind::Nurbs(nurbs) => {
                            match (nurbs.knots.first(), nurbs.knots.last()) {
                                (Some(first), Some(last)) => Some((*first, *last)),
                                _ => None,
                            }
                        }
                        CurveKind::Circle { .. } => Some((0.0, 2.0 * std::f64::consts::PI)),
                        CurveKind::Trimmed {
                            start_parameter,
                            end_parameter,
                            ..
                        } if start_parameter.is_finite() && end_parameter.is_finite() => {
                            Some((*start_parameter, *end_parameter))
                        }
                        _ => None,
                    };
                    let Some(domain) = domain else {
                        return false;
                    };
                    if !domain.0.is_finite()
                        || !domain.1.is_finite()
                        || (domain.1 - domain.0).abs() <= 1e-12
                    {
                        return false;
                    }
                    (domain.0.min(domain.1), domain.0.max(domain.1))
                }
            };
            (u0, u1, 0.0, 2.0 * std::f64::consts::PI)
        }
        _ => (0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI),
    };

    if !u_min.is_finite() || !u_max.is_finite() || u_max - u_min <= 1e-12 {
        return false;
    }

    if loops_3d.is_empty() {
        for j in 0..=GRID_STEPS_V {
            let v = v_min + (v_max - v_min) * (f64::from(j) / f64::from(GRID_STEPS_V));
            for i in 0..=GRID_STEPS_U {
                let u = u_min + (u_max - u_min) * (f64::from(i) / f64::from(GRID_STEPS_U));
                let Some(eval) = evaluate_surface(brep, surf_kind, u, v) else {
                    return false;
                };
                if !eval
                    .point
                    .iter()
                    .chain(eval.normal.iter())
                    .all(|value| value.is_finite())
                {
                    return false;
                }
            }
        }
        emit_isoparametric_grid_with(
            |u, v| evaluate_surface(brep, surf_kind, u, v),
            u_min,
            u_max,
            16,
            v_min,
            v_max,
            32,
            face_sense,
            result,
        );
        return true;
    }
    {
        let grid_steps_u = 16_u32;
        let grid_steps_v = 16_u32;
        let mut coarse_samples: Vec<(f64, f64, [f64; 3])> = Vec::with_capacity(17 * 17);
        for j in 0..=grid_steps_v {
            let v = v_min + (v_max - v_min) * (f64::from(j) / f64::from(grid_steps_v));
            for i in 0..=grid_steps_u {
                let u = u_min + (u_max - u_min) * (f64::from(i) / f64::from(grid_steps_u));
                if let Some(eval) = evaluate_surface(brep, surf_kind, u, v)
                    .filter(|eval| eval.point.iter().all(|value| value.is_finite()))
                {
                    coarse_samples.push((u, v, eval.point));
                }
            }
        }

        if !coarse_samples.is_empty() {
            let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
            for (_, _, point) in &coarse_samples {
                for axis in 0..3 {
                    bounds[axis][0] = bounds[axis][0].min(point[axis]);
                    bounds[axis][1] = bounds[axis][1].max(point[axis]);
                }
            }
            let support_size = bounds
                .iter()
                .map(|range| (range[1] - range[0]).powi(2))
                .sum::<f64>()
                .sqrt();
            if !support_size.is_finite() || support_size <= 1e-12 {
                return false;
            }
            let projection_tolerance = support_size * 1e-7;
            let project_uv = |p: [f64; 3], _ring: &[[f64; 3]], _i: usize| -> Option<[f64; 2]> {
                if !p.iter().all(|value| value.is_finite()) {
                    return None;
                }
                let mut best_u = coarse_samples[0].0;
                let mut best_v = coarse_samples[0].1;
                let mut best_dist = dist_sq(&coarse_samples[0].2, &p);
                for &(su, sv, spt) in &coarse_samples[1..] {
                    let d = dist_sq(&spt, &p);
                    if d < best_dist {
                        best_dist = d;
                        best_u = su;
                        best_v = sv;
                    }
                }

                let mut u = best_u;
                let mut v = best_v;
                let u_span = (u_max - u_min).max(1e-6);
                let v_span = (v_max - v_min).max(1e-6);
                let eps_u = (u_span * 1e-4).clamp(1e-7, 1e-3);
                let eps_v = (v_span * 1e-4).clamp(1e-7, 1e-3);

                for _ in 0..5 {
                    let Some(eval) = evaluate_surface(brep, surf_kind, u, v) else {
                        break;
                    };
                    let pt = eval.point;
                    let r = vec3_sub(pt, p);
                    if dist_sq(&pt, &p) < 1e-14 {
                        break;
                    }

                    let u_plus = (u + eps_u).min(u_max);
                    let u_minus = (u - eps_u).max(u_min);
                    let first_parameter_step = u_plus - u_minus;
                    let Some(first_axis_upper_evaluation) =
                        evaluate_surface(brep, surf_kind, u_plus, v)
                    else {
                        break;
                    };
                    let Some(first_axis_lower_evaluation) =
                        evaluate_surface(brep, surf_kind, u_minus, v)
                    else {
                        break;
                    };
                    let du = vec3_scale(
                        vec3_sub(
                            first_axis_upper_evaluation.point,
                            first_axis_lower_evaluation.point,
                        ),
                        1.0 / first_parameter_step.max(1e-12),
                    );

                    let v_plus = v + eps_v;
                    let v_minus = v - eps_v;
                    let second_parameter_step = v_plus - v_minus;
                    let Some(second_axis_upper_evaluation) =
                        evaluate_surface(brep, surf_kind, u, v_plus)
                    else {
                        break;
                    };
                    let Some(second_axis_lower_evaluation) =
                        evaluate_surface(brep, surf_kind, u, v_minus)
                    else {
                        break;
                    };
                    let dv = vec3_scale(
                        vec3_sub(
                            second_axis_upper_evaluation.point,
                            second_axis_lower_evaluation.point,
                        ),
                        1.0 / second_parameter_step.max(1e-12),
                    );

                    let j00 = vec3_dot(du, du);
                    let j01 = vec3_dot(du, dv);
                    let j11 = vec3_dot(dv, dv);
                    let first_axis_residual_dot = vec3_dot(du, r);
                    let second_axis_residual_dot = vec3_dot(dv, r);

                    let det = j00 * j11 - j01 * j01;
                    if !det.is_finite() || det.abs() < 1e-14 {
                        break;
                    }

                    let delta_u =
                        -(j11 * first_axis_residual_dot - j01 * second_axis_residual_dot) / det;
                    let delta_v =
                        -(j00 * second_axis_residual_dot - j01 * first_axis_residual_dot) / det;
                    if !delta_u.is_finite() || !delta_v.is_finite() {
                        break;
                    }

                    u = (u + delta_u).clamp(u_min, u_max);
                    v += delta_v;

                    if delta_u * delta_u + delta_v * delta_v < 1e-14 {
                        break;
                    }
                }

                let eval = evaluate_surface(brep, surf_kind, u, v)?;
                let residual_sq = dist_sq(&eval.point, &p);
                (eval.point.iter().all(|value| value.is_finite())
                    && residual_sq <= projection_tolerance.powi(2))
                .then_some([u, v])
            };

            let delta_u = (u_max - u_min) / f64::from(GRID_STEPS_U);
            let delta_v = (v_max - v_min) / f64::from(GRID_STEPS_V);
            let maximum_parameter_step = delta_u.max(delta_v).max(1e-4);

            if try_tessellate_trimmed_surface(
                surf_kind,
                project_uv,
                |u, v| evaluate_surface(brep, surf_kind, u, v),
                loops_3d,
                maximum_parameter_step,
                true,
                face_sense,
                result,
            ) == TrimmedSurfaceResult::Tessellated
            {
                return true;
            }
        }
    }

    false
}

/// Computes the outward polygon normal using Newell's method.
#[must_use]
pub fn compute_newell_normal(poly: &[[f64; 3]]) -> [f64; 3] {
    let mut nx = 0.0;
    let mut ny = 0.0;
    let mut nz = 0.0;
    let n = poly.len();
    for i in 0..n {
        let p_curr = poly[i];
        let p_next = poly[(i + 1) % n];
        nx += (p_curr[1] - p_next[1]) * (p_curr[2] + p_next[2]);
        ny += (p_curr[2] - p_next[2]) * (p_curr[0] + p_next[0]);
        nz += (p_curr[0] - p_next[0]) * (p_curr[1] + p_next[1]);
    }
    let norm = [nx, ny, nz];
    if vec3_norm(norm) > 1e-10 {
        vec3_normalize(norm)
    } else {
        [0.0, 0.0, 1.0]
    }
}

pub fn tessellate_generic_polygon(
    loops_3d: &[Vec<[f64; 3]>],
    effective_sense: bool,
    result: &mut TessellatedMesh,
) {
    let Some(poly) = loops_3d.first() else {
        return;
    };
    if poly.len() < 3 {
        return;
    }
    let normal = compute_newell_normal(poly);
    if loops_3d.len() > 1 {
        tessellate_planar_face(loops_3d[0][0], normal, effective_sense, loops_3d, result);
        return;
    }

    tessellate_planar_face(poly[0], normal, effective_sense, loops_3d, result);
}

fn point_inside_polygon(pt: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
        if (a[1] > pt[1]) != (b[1] > pt[1])
            && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

#[allow(
    clippy::too_many_lines,
    reason = "Linked topology validation and UV projection must preserve ordered loop continuity and shared rejection paths."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn surface_parametric_uv_loop(
    brep: &BrepModel,
    surface_id: u32,
    surface_kind: &SurfaceKind,
    loop_: &crate::brep::Loop,
    half_edges: &HashMap<u32, &crate::brep::HalfEdge>,
    edges: &HashMap<u32, &crate::brep::Edge>,
    curves: &HashMap<u32, &CurveGeometry>,
    vertices: &HashMap<u32, &crate::brep::Vertex>,
    points: &HashMap<u32, [f64; 3]>,
    chord_error: f64,
    swept_direction: Option<[f64; 3]>,
) -> Result<Option<Vec<[f64; 2]>>, ()> {
    if loop_.half_edges.is_empty() {
        return Err(());
    }
    let mut forward_samples_by_edge = Vec::with_capacity(loop_.half_edges.len());
    let mut forward_endpoints_by_edge = Vec::with_capacity(loop_.half_edges.len());
    let mut edge_tolerances = Vec::with_capacity(loop_.half_edges.len());
    let mut halfedge_vertices = Vec::with_capacity(loop_.half_edges.len());
    let mut halfedge_senses = Vec::with_capacity(loop_.half_edges.len());
    for half_edge_id in &loop_.half_edges {
        let Some(half_edge) = half_edges.get(half_edge_id) else {
            return Err(());
        };
        let curve_id = half_edge.curve.or_else(|| {
            half_edge
                .edge
                .and_then(|edge_id| edges.get(&edge_id))
                .and_then(|edge| edge.curve)
        });
        let Some(curve) = curve_id.and_then(|curve_id| curves.get(&curve_id).copied()) else {
            return Ok(None);
        };
        let (parameter_curve, start, end, trim_endpoints) = match &curve.kind {
            CurveKind::Trimmed {
                basis_curve,
                start_point,
                end_point,
                start_parameter,
                end_parameter,
            } => {
                let Some(basis) = curves.get(basis_curve) else {
                    return Err(());
                };
                let CurveKind::SurfaceParametric {
                    surface,
                    parameter_curve,
                    ..
                } = &basis.kind
                else {
                    return Ok(None);
                };
                let surface_matches = *surface == surface_id
                    || match surface_kind {
                        SurfaceKind::Offset { basis_surface, .. } => *surface == *basis_surface,
                        _ => false,
                    };
                if !surface_matches {
                    return Err(());
                }
                (
                    *parameter_curve,
                    *start_parameter,
                    *end_parameter,
                    Some([start_point.to_array(), end_point.to_array()]),
                )
            }
            CurveKind::SurfaceParametric {
                surface,
                parameter_curve,
                ..
            } => {
                let surface_matches = *surface == surface_id
                    || match surface_kind {
                        SurfaceKind::Offset { basis_surface, .. } => *surface == *basis_surface,
                        _ => false,
                    };
                if !surface_matches {
                    return Err(());
                }
                let parameter_curve_id = *parameter_curve;
                let Some(parameter_curve) = curves.get(&parameter_curve_id) else {
                    return Err(());
                };
                let CurveKind::Nurbs(nurbs) = &parameter_curve.kind else {
                    return Err(());
                };
                let (Some(start), Some(end)) = (nurbs.knots.first(), nurbs.knots.last()) else {
                    return Err(());
                };
                (parameter_curve_id, *start, *end, None)
            }
            _ => return Ok(None),
        };
        let Some(parameter_curve) = curves.get(&parameter_curve) else {
            return Err(());
        };
        let CurveKind::Nurbs(nurbs) = &parameter_curve.kind else {
            return Err(());
        };
        let forward_samples = sample_nurbs_trim_2d(nurbs, start, end, chord_error, |uv| {
            surface_point_in_brep(brep, surface_kind, uv, swept_direction)
        })
        .ok_or(())?;
        let forward_start = surface_point_from_uv(
            brep,
            surface_kind,
            *forward_samples.first().ok_or(())?,
            swept_direction,
        )?;
        let forward_end = surface_point_from_uv(
            brep,
            surface_kind,
            *forward_samples.last().ok_or(())?,
            swept_direction,
        )?;
        let edge_tolerance = source_edge_tolerance(half_edge, edges)?;
        if let Some([trim_start, trim_end]) = trim_endpoints {
            let endpoints_match_forward =
                within_source_endpoint_tolerance(forward_start, trim_start, edge_tolerance)
                    && within_source_endpoint_tolerance(forward_end, trim_end, edge_tolerance);
            let endpoints_match_reverse =
                within_source_endpoint_tolerance(forward_start, trim_end, edge_tolerance)
                    && within_source_endpoint_tolerance(forward_end, trim_start, edge_tolerance);
            if !endpoints_match_forward && !endpoints_match_reverse {
                return Err(());
            }
        }
        forward_samples_by_edge.push(forward_samples);
        forward_endpoints_by_edge.push([forward_start, forward_end]);
        edge_tolerances.push(edge_tolerance);
        halfedge_vertices.push(half_edge.vertex);
        halfedge_senses.push(half_edge.sense);
    }

    // The retained Glassspot B3 face anchors each current halfedge vertex at
    // that edge's forward trim end. Some existing mapped synthetic callers
    // use a current-as-start convention; accept either only when one convention
    // matches the entire loop. This does not establish a universal B3 rule.
    let choose_loop_convention = |vertex_is_start: bool| {
        let mut samples_by_edge = Vec::with_capacity(loop_.half_edges.len());
        let mut endpoints_by_edge = Vec::with_capacity(loop_.half_edges.len());
        let mut forward_by_edge = Vec::with_capacity(loop_.half_edges.len());
        for index in 0..loop_.half_edges.len() {
            let previous = (index + loop_.half_edges.len() - 1) % loop_.half_edges.len();
            let next = (index + 1) % loop_.half_edges.len();
            let (start_index, end_index) = if vertex_is_start {
                (index, next)
            } else {
                (previous, index)
            };
            let start_vertex =
                linked_vertex_point(halfedge_vertices[start_index], vertices, points).ok()?;
            let end_vertex =
                linked_vertex_point(halfedge_vertices[end_index], vertices, points).ok()?;
            let edge_tolerance = edge_tolerances[index];
            let [forward_start, forward_end] = forward_endpoints_by_edge[index];
            let matches_anchors = |start_point, end_point| {
                start_vertex.is_none_or(|(position, vertex_tolerance)| {
                    within_source_endpoint_tolerance(
                        start_point,
                        position,
                        vertex_tolerance.unwrap_or(edge_tolerance),
                    )
                }) && end_vertex.is_none_or(|(position, vertex_tolerance)| {
                    within_source_endpoint_tolerance(
                        end_point,
                        position,
                        vertex_tolerance.unwrap_or(edge_tolerance),
                    )
                })
            };
            let forward_matches = matches_anchors(forward_start, forward_end);
            let reverse_matches = matches_anchors(forward_end, forward_start);
            let forward = match (forward_matches, reverse_matches) {
                (true, false) => true,
                (false, true) => false,
                (true, true) if start_vertex.is_none() && end_vertex.is_none() => {
                    halfedge_senses[index] == Sense::Positive
                }
                _ => return None,
            };
            let mut samples = forward_samples_by_edge[index].clone();
            let endpoints = if forward {
                forward_endpoints_by_edge[index]
            } else {
                samples.reverse();
                [forward_end, forward_start]
            };
            samples_by_edge.push(samples);
            endpoints_by_edge.push(endpoints);
            forward_by_edge.push(forward);
        }
        Some((samples_by_edge, endpoints_by_edge, forward_by_edge))
    };
    let start_convention = choose_loop_convention(true);
    let end_convention = choose_loop_convention(false);
    let (edge_samples, lifted_endpoints, _) = match (start_convention, end_convention) {
        (Some(start), None) => start,
        (None, Some(end)) => end,
        (Some(start), Some(end)) if start.2 == end.2 => start,
        _ => return Err(()),
    };
    if edge_samples.len() != loop_.half_edges.len() {
        return Err(());
    }

    let extent = edge_samples
        .iter()
        .flatten()
        .flatten()
        .fold(1.0_f64, |extent, value| extent.max(value.abs()));
    let closure_tolerance = 1e-8 * extent;
    for index in 0..edge_samples.len() {
        let end = edge_samples[index].last().ok_or(())?;
        let next_start = edge_samples[(index + 1) % edge_samples.len()]
            .first()
            .ok_or(())?;
        if (end[0] - next_start[0]).hypot(end[1] - next_start[1]) > closure_tolerance {
            return Err(());
        }
        let lifted_end = lifted_endpoints[index][1];
        let lifted_next_start = lifted_endpoints[(index + 1) % lifted_endpoints.len()][0];
        let tolerance =
            source_edge_tolerance(half_edges.get(&loop_.half_edges[index]).ok_or(())?, edges)?.max(
                source_edge_tolerance(
                    half_edges
                        .get(&loop_.half_edges[(index + 1) % loop_.half_edges.len()])
                        .ok_or(())?,
                    edges,
                )?,
            );
        if !within_source_endpoint_tolerance(lifted_end, lifted_next_start, tolerance) {
            return Err(());
        }
    }

    let mut ring = Vec::new();
    for samples in edge_samples {
        let count = samples.len().saturating_sub(1);
        ring.extend(samples.into_iter().take(count));
    }
    if ring.len() < 3 || ring.iter().flatten().any(|value| !value.is_finite()) {
        return Err(());
    }
    Ok(Some(ring))
}

fn surface_point_in_brep(
    brep: &BrepModel,
    surface: &SurfaceKind,
    uv: [f64; 2],
    swept_direction: Option<[f64; 3]>,
) -> Option<[f64; 3]> {
    if let Some((section, direction)) = surface.swept_section_direction() {
        let direction = swept_direction.unwrap_or_else(|| direction.to_array());
        let section_geom = brep.curves.iter().find(|curve| curve.id == section)?;
        evaluate_ruled_sweep_surface_in_brep(
            Some(brep),
            &section_geom.kind,
            direction,
            uv[0],
            uv[1],
        )
        .map(|eval| eval.point)
    } else if matches!(
        surface,
        SurfaceKind::BlendedEdge { .. }
            | SurfaceKind::BlendBoundary { .. }
            | SurfaceKind::Offset { .. }
            | SurfaceKind::Spun { .. }
    ) {
        evaluate_surface_point(brep, surface, uv[0], uv[1])
    } else {
        evaluate_surface_point(brep, surface, uv[0], uv[1])
            .or_else(|| evaluate_surface_point_kind(surface, uv[0], uv[1]))
    }
}

fn surface_point_from_uv(
    brep: &BrepModel,
    surface: &SurfaceKind,
    uv: [f64; 2],
    swept_direction: Option<[f64; 3]>,
) -> Result<[f64; 3], ()> {
    surface_point_in_brep(brep, surface, uv, swept_direction).ok_or(())
}

fn evaluate_ruled_sweep_surface(
    section: &CurveKind,
    direction: [f64; 3],
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    evaluate_ruled_sweep_surface_in_brep(None, section, direction, u, v)
}

fn evaluate_ruled_sweep_surface_in_brep(
    brep: Option<&BrepModel>,
    section: &CurveKind,
    direction: [f64; 3],
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    if !u.is_finite() || !v.is_finite() || direction.iter().any(|c| !c.is_finite()) {
        return None;
    }
    let (point, tangent) = match section {
        CurveKind::Nurbs(nurbs) => {
            let start = *nurbs.knots.first()?;
            let end = *nurbs.knots.last()?;
            let step = (end - start).abs() * 1e-6;
            if !step.is_finite() || step <= 0.0 {
                return None;
            }
            let point = evaluate_ruled_sweep_point_candidate(nurbs, direction, u, v).ok()?;
            let before =
                evaluate_ruled_sweep_point_candidate(nurbs, direction, u - step, v).ok()?;
            let after = evaluate_ruled_sweep_point_candidate(nurbs, direction, u + step, v).ok()?;
            let tangent = vec3_scale(vec3_sub(after, before), 0.5 / step);
            (point, tangent)
        }
        CurveKind::Line { .. }
        | CurveKind::Circle { .. }
        | CurveKind::Ellipse { .. }
        | CurveKind::Parabola { .. }
        | CurveKind::Hyperbola { .. } => {
            let eval = crate::brep::evaluate_curve(section, u)?;
            let point = [
                eval.point[0] + v * direction[0],
                eval.point[1] + v * direction[1],
                eval.point[2] + v * direction[2],
            ];
            if !point.iter().all(|c| c.is_finite()) {
                return None;
            }
            (point, eval.tangent)
        }
        CurveKind::Trimmed {
            basis_curve,
            start_point,
            end_point,
            start_parameter,
            end_parameter,
        } => {
            let mapped_u = if start_parameter.is_finite() && end_parameter.is_finite() {
                let min_p = start_parameter.min(*end_parameter);
                let max_p = start_parameter.max(*end_parameter);
                if (0.0..=1.0).contains(&u) && (u < min_p || u > max_p) {
                    start_parameter + u * (end_parameter - start_parameter)
                } else {
                    u
                }
            } else {
                u
            };
            if let Some(brep) = brep {
                if let Some(basis) = brep.curves.iter().find(|c| c.id == *basis_curve) {
                    return evaluate_ruled_sweep_surface_in_brep(
                        Some(brep),
                        &basis.kind,
                        direction,
                        mapped_u,
                        v,
                    );
                }
            }
            let p0 = start_point.to_array();
            let p1 = end_point.to_array();
            let tangent = vec3_sub(p1, p0);
            let param = if (0.0..=1.0).contains(&u) { u } else { 0.5 };
            let curve_pt = vec3_add(p0, vec3_scale(tangent, param));
            let point = [
                curve_pt[0] + v * direction[0],
                curve_pt[1] + v * direction[1],
                curve_pt[2] + v * direction[2],
            ];
            if !point.iter().all(|c| c.is_finite()) {
                return None;
            }
            (point, tangent)
        }
        _ => return None,
    };

    let tangent_length = tangent[0].hypot(tangent[1]).hypot(tangent[2]);
    let direction_length = direction[0].hypot(direction[1]).hypot(direction[2]);
    if !tangent_length.is_finite()
        || tangent_length <= 0.0
        || !direction_length.is_finite()
        || direction_length <= 0.0
    {
        return None;
    }
    let unit_tangent = vec3_scale(tangent, 1.0 / tangent_length);
    let unit_direction = vec3_scale(direction, 1.0 / direction_length);
    let normal = vec3_cross(unit_tangent, unit_direction);
    let normal_length = normal[0].hypot(normal[1]).hypot(normal[2]);

    (normal_length.is_finite() && normal_length > 1e-12).then(|| SurfaceEvaluation {
        point,
        normal: vec3_scale(normal, 1.0 / normal_length),
    })
}

fn source_edge_tolerance(
    half_edge: &crate::brep::HalfEdge,
    edges: &HashMap<u32, &crate::brep::Edge>,
) -> Result<f64, ()> {
    let Some(edge_id) = half_edge.edge else {
        return Ok(0.0);
    };
    let Some(edge) = edges.get(&edge_id) else {
        return Err(());
    };
    Ok(valid_source_tolerance(edge.tolerance)?.unwrap_or(0.0))
}

fn intersection_edge_tolerance(
    half_edge: &crate::brep::HalfEdge,
    edges: &HashMap<u32, &crate::brep::Edge>,
    brep: &BrepModel,
) -> Result<f64, ()> {
    let Some(edge_id) = half_edge.edge else {
        return Ok(0.0);
    };
    let Some(edge) = edges.get(&edge_id) else {
        return Err(());
    };
    match valid_source_tolerance(edge.tolerance)? {
        Some(tolerance) => Ok(tolerance),
        None if brep.source_format == BrepSourceFormat::Text => {
            xt_null_entity_tolerance(&edge.owner, brep)
        }
        None => Ok(0.0),
    }
}

fn xt_null_entity_tolerance(
    owner: &crate::brep::SourceNodeRef,
    brep: &BrepModel,
) -> Result<f64, ()> {
    if brep.source_format != BrepSourceFormat::Text {
        return Err(());
    }
    let body = match owner.type_name.as_str() {
        "BODY" => {
            let mut matches = brep
                .bodies
                .iter()
                .filter(|body| body.source.node_index == owner.node_index);
            let body = matches.next().ok_or(())?;
            if matches.next().is_some() {
                return Err(());
            }
            body
        }
        "SHELL" => {
            let mut shells = brep
                .shells
                .iter()
                .filter(|shell| shell.source.node_index == owner.node_index);
            let shell = shells.next().ok_or(())?;
            if shells.next().is_some() {
                return Err(());
            }
            let mut regions = brep
                .regions
                .iter()
                .filter(|region| region.id == shell.region);
            let region = regions.next().ok_or(())?;
            if regions.next().is_some() {
                return Err(());
            }
            let mut bodies = brep.bodies.iter().filter(|body| body.id == region.body);
            let body = bodies.next().ok_or(())?;
            if bodies.next().is_some() {
                return Err(());
            }
            body
        }
        _ => return Err(()),
    };
    let tolerance = body.linear_resolution * 0.5;
    (tolerance.is_finite() && tolerance > 0.0)
        .then_some(tolerance)
        .ok_or(())
}

#[allow(
    clippy::type_complexity,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn linked_vertex_point(
    vertex_id: Option<u32>,
    vertices: &HashMap<u32, &crate::brep::Vertex>,
    points: &HashMap<u32, [f64; 3]>,
) -> Result<Option<([f64; 3], Option<f64>)>, ()> {
    let Some(vertex_id) = vertex_id else {
        return Ok(None);
    };
    let vertex = vertices.get(&vertex_id).ok_or(())?;
    let point = *points.get(&vertex.point).ok_or(())?;
    if point.iter().any(|coordinate| !coordinate.is_finite()) {
        return Err(());
    }
    Ok(Some((point, valid_source_tolerance(vertex.tolerance)?)))
}

fn valid_source_tolerance(tolerance: Option<f64>) -> Result<Option<f64>, ()> {
    match tolerance {
        Some(value) if value.is_finite() && value >= 0.0 => Ok(Some(value)),
        Some(_) => Err(()),
        None => Ok(None),
    }
}

fn within_source_endpoint_tolerance(a: [f64; 3], b: [f64; 3], source_tolerance: f64) -> bool {
    if !source_tolerance.is_finite()
        || source_tolerance < 0.0
        || a.iter()
            .chain(b.iter())
            .any(|coordinate| !coordinate.is_finite())
    {
        return false;
    }
    let scale = a
        .iter()
        .chain(b.iter())
        .fold(1.0_f64, |scale, coordinate| scale.max(coordinate.abs()));
    let numeric_floor = 256.0 * f64::EPSILON * scale;
    let tolerance = source_tolerance.max(numeric_floor);
    vec3_norm(vec3_sub(a, b)) <= tolerance
}

#[allow(
    clippy::float_cmp,
    reason = "Exact knot and endpoint equality preserves NURBS trim boundary identity."
)]
fn sample_nurbs_trim_2d<F>(
    curve: &crate::brep::NurbsCurve,
    t_start: f64,
    t_end: f64,
    chord_error: f64,
    lift: F,
) -> Option<Vec<[f64; 2]>>
where
    F: Fn([f64; 2]) -> Option<[f64; 3]>,
{
    if !t_start.is_finite()
        || !t_end.is_finite()
        || t_start == t_end
        || !chord_error.is_finite()
        || chord_error <= 0.0
        || curve.knots.len() > MAX_NURBS_TRIM_KNOT_VALUES
        || curve.knots.iter().any(|knot| !knot.is_finite())
    {
        return None;
    }
    let (lo, hi) = (t_start.min(t_end), t_start.max(t_end));
    let mut knots: Vec<_> = curve
        .knots
        .iter()
        .copied()
        .filter(|knot| *knot > lo && *knot < hi)
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| *a == *b);
    if knots.len() + 1 > MAX_NURBS_TRIM_SPANS {
        return None;
    }
    if t_end < t_start {
        knots.reverse();
    }
    let mut spans = Vec::with_capacity(knots.len() + 2);
    spans.push(t_start);
    spans.extend(knots);
    spans.push(t_end);
    let mut budget = MAX_NURBS_TRIM_SAMPLE_EVALUATIONS;
    let mut samples = Vec::new();
    for span in spans.windows(2) {
        let a = evaluate_nurbs_trim_2d(curve, span[0], &mut budget)?;
        let b = evaluate_nurbs_trim_2d(curve, span[1], &mut budget)?;
        if samples.last().is_some_and(|last| *last != a) {
            return None;
        }
        if samples.is_empty() {
            samples.push(a);
        }
        subdivide_nurbs_trim_2d(
            curve,
            span[0],
            a,
            span[1],
            b,
            chord_error,
            0,
            &mut budget,
            &mut samples,
            &lift,
        )?;
    }
    Some(samples)
}

fn evaluate_nurbs_trim_2d(
    curve: &crate::brep::NurbsCurve,
    parameter: f64,
    budget: &mut usize,
) -> Option<[f64; 2]> {
    *budget = budget.checked_sub(1)?;
    evaluate_mapped_nurbs_curve_candidate_2d(curve, parameter).ok()
}

#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn subdivide_nurbs_trim_2d<F>(
    curve: &crate::brep::NurbsCurve,
    t0: f64,
    p0: [f64; 2],
    t1: f64,
    p1: [f64; 2],
    chord_error: f64,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<[f64; 2]>,
    lift: &F,
) -> Option<()>
where
    F: Fn([f64; 2]) -> Option<[f64; 3]>,
{
    let p0_3d = lift(p0)?;
    let p1_3d = lift(p1)?;
    let mut deviation = 0.0_f64;
    for fraction in [0.25, 0.5, 0.75] {
        let parameter = t0 * (1.0 - fraction) + t1 * fraction;
        let point = evaluate_nurbs_trim_2d(curve, parameter, budget)?;
        let point_3d = lift(point)?;
        let chord_3d = vec3_add(
            vec3_scale(p0_3d, 1.0 - fraction),
            vec3_scale(p1_3d, fraction),
        );
        deviation = deviation.max(vec3_norm(vec3_sub(point_3d, chord_3d)));
    }
    if !deviation.is_finite() {
        return None;
    }
    if deviation <= chord_error {
        out.push(p1);
        return Some(());
    }
    if depth >= MAX_NURBS_TRIM_SUBDIVISION_DEPTH {
        return None;
    }
    let tm = (t0 + t1) * 0.5;
    let pm = evaluate_nurbs_trim_2d(curve, tm, budget)?;
    subdivide_nurbs_trim_2d(
        curve,
        t0,
        p0,
        tm,
        pm,
        chord_error,
        depth + 1,
        budget,
        out,
        lift,
    )?;
    subdivide_nurbs_trim_2d(
        curve,
        tm,
        pm,
        t1,
        p1,
        chord_error,
        depth + 1,
        budget,
        out,
        lift,
    )
}

const MAX_NURBS_TRIM_SUBDIVISION_DEPTH: usize = 12;
const MAX_NURBS_TRIM_SPANS: usize = 4096;
const MAX_NURBS_TRIM_KNOT_VALUES: usize = MAX_NURBS_TRIM_SPANS + 1;
const MAX_NURBS_TRIM_SAMPLE_EVALUATIONS: usize = 16_384;
// shortcut: caps repeated chart traversal at 4M points per edge; raise with a reviewed work budget.
const MAX_INTERSECTION_CHART_SAMPLE_WORK: usize = 4_194_304;

fn consume_intersection_chart_work(
    chart_points: usize,
    solver_visits: usize,
    remaining: &mut usize,
) -> Option<()> {
    *remaining = remaining.checked_sub(chart_points.checked_mul(solver_visits)?)?;
    Some(())
}

fn consume_intersection_inverse_work(
    chart_points: usize,
    solver_visits: usize,
    remaining: &mut usize,
) -> Option<()> {
    // Two endpoints each scan both axis conventions and can evaluate every chart segment.
    let charges = chart_points.checked_add(1)?.checked_mul(2)?;
    for _ in 0..charges {
        consume_intersection_chart_work(chart_points, solver_visits, remaining)?;
    }
    Some(())
}

fn consume_cone_cylinder_inverse_work(chart_points: usize, remaining: &mut usize) -> Option<()> {
    // Conservative charge for endpoint inversion and per-sample axis/root selection.
    let full_evaluation_work = chart_points.checked_mul(4)?.checked_add(1)?;
    let station_evaluations = chart_points
        .checked_mul(2)?
        .checked_sub(3)?
        .checked_mul(2)?;
    let station_scans = station_evaluations.checked_mul(full_evaluation_work)?;
    let segment_evaluations = chart_points.checked_sub(1)?;
    let newton_work_per_segment = chart_points
        .checked_mul(2)?
        .checked_add(2)?
        .checked_mul(40)?;
    let segment_work = full_evaluation_work.checked_add(newton_work_per_segment)?;
    let interval_work = segment_evaluations.checked_mul(segment_work)?;
    let both_endpoints = station_scans.checked_add(interval_work)?.checked_mul(2)?;
    *remaining = remaining.checked_sub(both_endpoints)?;
    Some(())
}

fn intersection_chart_solver_visits(brep: &BrepModel, curve: &CurveKind) -> usize {
    let CurveKind::Intersection { surfaces, .. } = curve else {
        return 1;
    };
    let Some(a) = brep
        .surfaces
        .iter()
        .find(|surface| surface.id == surfaces[0])
    else {
        return 1;
    };
    let Some(b) = brep
        .surfaces
        .iter()
        .find(|surface| surface.id == surfaces[1])
    else {
        return 1;
    };
    if matches!(
        (&a.kind, &b.kind),
        (
            SurfaceKind::XtCone { .. },
            SurfaceKind::Cylinder { .. } | SurfaceKind::Plane { .. },
        ) | (
            SurfaceKind::Cylinder { .. } | SurfaceKind::Plane { .. },
            SurfaceKind::XtCone { .. },
        ) | (SurfaceKind::Plane { .. }, SurfaceKind::Cone { .. })
            | (SurfaceKind::Cone { .. }, SurfaceKind::Plane { .. })
    ) {
        2
    } else {
        1
    }
}

#[allow(
    clippy::float_cmp,
    reason = "Exact knot and endpoint equality preserves NURBS trim boundary identity."
)]
fn sample_nurbs_trim(
    curve: &CurveKind,
    t_start: f64,
    t_end: f64,
    chord_error: f64,
) -> Option<Vec<[f64; 3]>> {
    if !t_start.is_finite()
        || !t_end.is_finite()
        || !chord_error.is_finite()
        || chord_error <= 0.0
        || t_start == t_end
    {
        return None;
    }
    let CurveKind::Nurbs(nurbs) = curve else {
        return None;
    };
    if nurbs.knots.len() > MAX_NURBS_TRIM_KNOT_VALUES
        || nurbs.knots.iter().any(|knot| !knot.is_finite())
    {
        return None;
    }
    let (lo, hi) = (t_start.min(t_end), t_start.max(t_end));
    let mut internal_knots: Vec<f64> = nurbs
        .knots
        .iter()
        .copied()
        .filter(|knot| *knot > lo && *knot < hi)
        .collect();
    internal_knots.sort_by(f64::total_cmp);
    internal_knots.dedup_by(|a, b| *a == *b);
    if internal_knots.len() + 1 > MAX_NURBS_TRIM_SPANS {
        return None;
    }
    if t_end < t_start {
        internal_knots.reverse();
    }
    let mut spans = Vec::with_capacity(internal_knots.len() + 2);
    spans.push(t_start);
    spans.extend(internal_knots);
    spans.push(t_end);

    let mut samples = Vec::new();
    let mut evaluation_budget = MAX_NURBS_TRIM_SAMPLE_EVALUATIONS;
    for interval in spans.windows(2) {
        let (t0, t1) = (interval[0], interval[1]);
        let p0 = evaluate_nurbs_trim_sample(curve, t0, &mut evaluation_budget)?;
        let p1 = evaluate_nurbs_trim_sample(curve, t1, &mut evaluation_budget)?;
        if samples.is_empty() {
            if p0.iter().any(|coordinate| !coordinate.is_finite()) {
                return None;
            }
            samples.push(p0);
        } else if samples.last() != Some(&p0) {
            return None;
        }
        let mut evaluate =
            |parameter, _| evaluate_nurbs_trim_sample(curve, parameter, &mut evaluation_budget);
        subdivide_sampled_trim(t0, p0, t1, p1, chord_error, 0, &mut evaluate, &mut samples)?;
    }
    Some(samples)
}

#[allow(
    clippy::float_cmp,
    reason = "Exact midpoint equality preserves the deterministic sampled trim split."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn subdivide_sampled_trim<E>(
    t0: f64,
    p0: [f64; 3],
    t1: f64,
    p1: [f64; 3],
    chord_error: f64,
    depth: usize,
    evaluate: &mut E,
    out: &mut Vec<[f64; 3]>,
) -> Option<()>
where
    E: FnMut(f64, [f64; 3]) -> Option<[f64; 3]>,
{
    if p0
        .iter()
        .chain(&p1)
        .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    // Quarter-point deviation is a sampled criterion, not a conservative curve bound.
    let mut max_sampled_deviation: f64 = 0.0;
    let mut midpoint = None;
    for fraction in [0.25, 0.5, 0.75] {
        let t = t0 * (1.0 - fraction) + t1 * fraction;
        let chord_point = vec3_add(vec3_scale(p0, 1.0 - fraction), vec3_scale(p1, fraction));
        let point = evaluate(t, chord_point)?;
        if point.iter().any(|coordinate| !coordinate.is_finite()) {
            return None;
        }
        let deviation = vec3_norm(vec3_sub(point, chord_point));
        if !deviation.is_finite() {
            return None;
        }
        max_sampled_deviation = max_sampled_deviation.max(deviation);
        if fraction == 0.5 {
            midpoint = Some(point);
        }
    }

    if max_sampled_deviation <= chord_error {
        out.push(p1);
        return Some(());
    }
    if depth >= MAX_NURBS_TRIM_SUBDIVISION_DEPTH {
        return None;
    }

    let tm = t0 * 0.5 + t1 * 0.5;
    let pm = midpoint?;
    subdivide_sampled_trim(t0, p0, tm, pm, chord_error, depth + 1, evaluate, out)?;
    subdivide_sampled_trim(tm, pm, t1, p1, chord_error, depth + 1, evaluate, out)
}

fn evaluate_nurbs_trim_sample(
    curve: &CurveKind,
    parameter: f64,
    evaluation_budget: &mut usize,
) -> Option<[f64; 3]> {
    *evaluation_budget = evaluation_budget.checked_sub(1)?;
    let point = evaluate_curve_point(curve, parameter)?;
    point
        .iter()
        .all(|coordinate| coordinate.is_finite())
        .then_some(point)
}

#[allow(
    clippy::float_cmp,
    reason = "Exact parameter equality detects collapsed intersection trim intervals."
)]
fn sample_intersection_trim(
    brep: &BrepModel,
    curve: &CurveKind,
    t_start: f64,
    t_end: f64,
    chord_error: f64,
    endpoint_candidates: Option<([f64; 3], [f64; 3])>,
    chart_work_budget: &mut usize,
) -> Option<Vec<[f64; 3]>> {
    let CurveKind::Intersection { chart_points, .. } = curve else {
        return None;
    };
    if !t_start.is_finite()
        || !t_end.is_finite()
        || t_start == t_end
        || !chord_error.is_finite()
        || chord_error <= 0.0
    {
        return None;
    }
    let mut evaluation_budget = MAX_NURBS_TRIM_SAMPLE_EVALUATIONS;
    let solver_visits = intersection_chart_solver_visits(brep, curve);
    let cone_cylinder_sampling = endpoint_candidates.is_some_and(|_| {
        let CurveKind::Intersection { surfaces, .. } = curve else {
            return false;
        };
        let kinds = surfaces.map(|id| {
            brep.surfaces
                .iter()
                .find(|surface| surface.id == id)
                .map(|surface| &surface.kind)
        });
        matches!(
            kinds,
            [
                Some(SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. }),
                Some(SurfaceKind::Cylinder { .. }),
            ] | [
                Some(SurfaceKind::Cylinder { .. }),
                Some(SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. }),
            ]
        )
    });
    let mut evaluate = |parameter, trim_chord_point| {
        evaluation_budget = evaluation_budget.checked_sub(1)?;
        if cone_cylinder_sampling {
            consume_cone_cylinder_inverse_work(chart_points.len(), chart_work_budget)?;
        } else {
            consume_intersection_chart_work(chart_points.len(), solver_visits, chart_work_budget)?;
        }
        let evaluation = if cone_cylinder_sampling {
            super::intersection_eval::evaluate_cone_cylinder_intersection_sample_candidate(
                brep,
                curve,
                parameter,
                trim_chord_point,
            )
        } else {
            crate::brep::evaluate_intersection_candidate(brep, curve, parameter)
        };
        let point = evaluation.ok()?.point;
        point
            .iter()
            .all(|coordinate| coordinate.is_finite())
            .then_some(point)
    };
    let (p0, p1) = match endpoint_candidates {
        Some((p0, p1)) if p0.into_iter().chain(p1).all(f64::is_finite) => (p0, p1),
        Some(_) => return None,
        None => (
            evaluate(t_start, chart_points.first()?.to_array())?,
            evaluate(t_end, chart_points.last()?.to_array())?,
        ),
    };
    let mut samples = vec![p0];
    subdivide_sampled_trim(
        t_start,
        p0,
        t_end,
        p1,
        chord_error,
        0,
        &mut evaluate,
        &mut samples,
    )?;
    Some(samples)
}

fn dist_sq(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

fn full_polyline_points(polyline: &PolylineData, edge_sense: Sense) -> Option<Vec<[f64; 3]>> {
    let forward = match (polyline.sense, edge_sense) {
        (Sense::Positive, Sense::Positive) | (Sense::Negative, Sense::Negative) => true,
        (Sense::Positive, Sense::Negative) | (Sense::Negative, Sense::Positive) => false,
        (_, Sense::Unknown) | (Sense::Unknown, _) => return None,
    };
    if polyline.points.len() < 2 {
        return None;
    }
    let mut points = polyline
        .points
        .iter()
        .map(|point| point.to_array())
        .collect::<Vec<_>>();
    if !forward {
        points.reverse();
    }
    if polyline.closed && points.len() > 1 && points.first() == points.last() {
        points.pop();
    } else if !polyline.closed {
        // The next half-edge contributes this edge's terminal vertex.
        points.pop();
    }
    Some(points)
}

// shortcut: trims scan at most 16,384 segments; raise with a reviewed parser resource limit if needed.
const MAX_TRIMMED_POLYLINE_SEGMENTS: usize = 16_384;

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn trimmed_polyline_points(
    polyline: &PolylineData,
    edge_sense: Sense,
    start_point: [f64; 3],
    end_point: [f64; 3],
) -> Option<Vec<[f64; 3]>> {
    if polyline.points.len() < 2 {
        return None;
    }
    let mut vertices = polyline
        .points
        .iter()
        .map(|point| point.to_array())
        .collect::<Vec<_>>();
    if polyline.closed && vertices.first() == vertices.last() {
        vertices.pop();
    }
    let segment_count = if polyline.closed {
        vertices.len()
    } else {
        vertices.len().saturating_sub(1)
    };
    if vertices.len() < 2 || segment_count > MAX_TRIMMED_POLYLINE_SEGMENTS {
        return None;
    }
    let start = locate_polyline_trim(&vertices, start_point, polyline.closed)?;
    let end = locate_polyline_trim(&vertices, end_point, polyline.closed)?;
    let parameter_forward = match polyline.sense {
        Sense::Positive => true,
        Sense::Negative => false,
        Sense::Unknown => return None,
    };
    let edge_forward = match edge_sense {
        Sense::Positive => true,
        Sense::Negative => false,
        Sense::Unknown => return None,
    };
    let position_order = compare_polyline_positions(start, end, &vertices, polyline.closed)?;
    if position_order == 0 {
        return None;
    }
    if polyline.closed {
        // shortcut: closed trims follow stored cyclic order; revise if real XT evidence contradicts it.
        let mut interior_vertices = Vec::new();
        for index in 0..vertices.len() {
            let vertex = PolylinePosition::Vertex(index);
            let after_start = compare_polyline_positions(start, vertex, &vertices, true)?;
            let before_end = compare_polyline_positions(vertex, end, &vertices, true)?;
            let include = match (parameter_forward, position_order) {
                (true, -1) => after_start < 0 && before_end < 0,
                (true, 1) => after_start < 0 || before_end < 0,
                (false, 1) => after_start > 0 && before_end > 0,
                (false, -1) => after_start > 0 || before_end > 0,
                _ => false,
            };
            if include {
                interior_vertices.push(index);
            }
        }
        if parameter_forward {
            interior_vertices.sort_unstable();
        } else {
            interior_vertices.sort_unstable_by(|left, right| right.cmp(left));
        }
        if parameter_forward && position_order > 0 {
            let after_start = interior_vertices
                .iter()
                .filter(|index| {
                    compare_polyline_positions(
                        start,
                        PolylinePosition::Vertex(**index),
                        &vertices,
                        true,
                    ) == Some(-1)
                })
                .count();
            let before_start = interior_vertices.len() - after_start;
            interior_vertices.rotate_left(before_start);
        } else if !parameter_forward && position_order < 0 {
            let after_start = interior_vertices
                .iter()
                .filter(|index| {
                    compare_polyline_positions(
                        start,
                        PolylinePosition::Vertex(**index),
                        &vertices,
                        true,
                    ) == Some(-1)
                })
                .count();
            interior_vertices.rotate_left(after_start);
        }
        let mut points = vec![start_point];
        points.extend(interior_vertices.into_iter().map(|index| vertices[index]));
        points.push(end_point);
        if !edge_forward {
            points.reverse();
        }
        points.pop();
        return Some(points);
    }
    if (position_order < 0) != parameter_forward {
        return None;
    }

    let mut points = vec![start_point];
    for (index, point) in vertices.iter().enumerate() {
        if compare_polyline_positions(start, PolylinePosition::Vertex(index), &vertices, false)?
            == position_order
            && compare_polyline_positions(PolylinePosition::Vertex(index), end, &vertices, false)?
                == position_order
        {
            points.push(*point);
        }
    }
    if position_order > 0 {
        points[1..].reverse();
    }
    points.push(end_point);
    if !edge_forward {
        points.reverse();
    }
    // Match full open polylines: the following half-edge contributes the terminal point.
    points.pop();
    Some(points)
}

#[derive(Clone, Copy)]
enum PolylinePosition {
    Vertex(usize),
    Segment(usize, [f64; 3]),
}

#[allow(
    clippy::float_cmp,
    reason = "Exact coordinate equality preserves source polyline vertex identity."
)]
fn locate_polyline_trim(
    points: &[[f64; 3]],
    point: [f64; 3],
    closed: bool,
) -> Option<PolylinePosition> {
    let segment_count = if closed {
        points.len()
    } else {
        points.len().checked_sub(1)?
    };
    let vertices = points
        .iter()
        .enumerate()
        .filter(|(_, candidate)| **candidate == point)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match vertices.as_slice() {
        [index] => {
            for segment_index in 0..segment_count {
                let start = points[segment_index];
                let end = points[(segment_index + 1) % points.len()];
                if exact_point_on_segment(point, start, end)? {
                    return None;
                }
            }
            return Some(PolylinePosition::Vertex(*index));
        }
        [] => {}
        _ => return None,
    }

    let mut matches = Vec::new();
    for index in 0..segment_count {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        if exact_point_on_segment(point, start, end)? {
            matches.push(index);
        }
    }
    let [index] = matches.as_slice() else {
        return None;
    };
    Some(PolylinePosition::Segment(*index, point))
}

#[allow(
    clippy::float_cmp,
    reason = "Exact coordinate equality preserves source polyline position ordering."
)]
fn compare_polyline_positions(
    left: PolylinePosition,
    right: PolylinePosition,
    points: &[[f64; 3]],
    closed: bool,
) -> Option<i8> {
    match (left, right) {
        (PolylinePosition::Vertex(left), PolylinePosition::Vertex(right)) => {
            Some(match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        (PolylinePosition::Vertex(vertex), PolylinePosition::Segment(segment, _)) => {
            Some(if vertex <= segment { -1 } else { 1 })
        }
        (PolylinePosition::Segment(segment, _), PolylinePosition::Vertex(vertex)) => {
            Some(if segment < vertex { -1 } else { 1 })
        }
        (
            PolylinePosition::Segment(left_segment, left),
            PolylinePosition::Segment(right_segment, right),
        ) => {
            if left_segment != right_segment {
                return Some(if left_segment < right_segment { -1 } else { 1 });
            }
            let segment_end = *points
                .get(left_segment + 1)
                .or_else(|| closed.then(|| points.first()).flatten())?;
            let segment_start = *points.get(left_segment)?;
            let axis = (0..3).find(|axis| segment_start[*axis] != segment_end[*axis])?;
            let ordering = left[axis].partial_cmp(&right[axis])?;
            let forward = segment_start[axis] < segment_end[axis];
            Some(match (ordering, forward) {
                (std::cmp::Ordering::Less, true) | (std::cmp::Ordering::Greater, false) => -1,
                (std::cmp::Ordering::Equal, _) => 0,
                _ => 1,
            })
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "This helper deliberately tests exact endpoint and coordinate equality."
)]
#[allow(
    clippy::many_single_char_names,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn exact_point_on_segment(point: [f64; 3], start: [f64; 3], end: [f64; 3]) -> Option<bool> {
    if !point
        .iter()
        .chain(&start)
        .chain(&end)
        .all(|value| value.is_finite())
        || start == end
    {
        return Some(false);
    }
    let mut p = [0_i128; 3];
    let mut a = [0_i128; 3];
    let mut b = [0_i128; 3];
    for axis in 0..3 {
        let scaled = exact_scaled_axis([start[axis], end[axis], point[axis]])?;
        a[axis] = scaled[0];
        b[axis] = scaled[1];
        p[axis] = scaled[2];
    }
    let u = [
        b[0].checked_sub(a[0])?,
        b[1].checked_sub(a[1])?,
        b[2].checked_sub(a[2])?,
    ];
    let v = [
        p[0].checked_sub(a[0])?,
        p[1].checked_sub(a[1])?,
        p[2].checked_sub(a[2])?,
    ];
    let cross = [
        u[1].checked_mul(v[2])?
            .checked_sub(u[2].checked_mul(v[1])?)?,
        u[2].checked_mul(v[0])?
            .checked_sub(u[0].checked_mul(v[2])?)?,
        u[0].checked_mul(v[1])?
            .checked_sub(u[1].checked_mul(v[0])?)?,
    ];
    Some(
        cross == [0; 3]
            && (0..3)
                .all(|axis| p[axis] >= a[axis].min(b[axis]) && p[axis] <= a[axis].max(b[axis]))
            && point != start
            && point != end,
    )
}

fn exact_scaled_axis(values: [f64; 3]) -> Option<[i128; 3]> {
    let parts = values.map(|value| {
        let bits = value.to_bits();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1_u64 << 52) - 1);
        let (mantissa, exponent) = if exponent_bits == 0 {
            (fraction, -1074)
        } else {
            ((1_u64 << 52) | fraction, exponent_bits - 1023 - 52)
        };
        (bits >> 63 != 0, mantissa, exponent)
    });
    let min_exponent = parts
        .iter()
        .filter(|(_, mantissa, _)| *mantissa != 0)
        .map(|(_, _, exponent)| *exponent)
        .min()
        .unwrap_or(-1074);
    parts
        .map(|(negative, mantissa, exponent)| {
            if mantissa == 0 {
                return Some(0);
            }
            let shift = u32::try_from(exponent - min_exponent).ok()?;
            if shift >= u128::BITS || u128::from(mantissa) > (u128::MAX >> shift) {
                return None;
            }
            let magnitude = u128::from(mantissa).checked_shl(shift)?;
            let limit = if negative {
                1_u128 << 127
            } else {
                i128::MAX as u128
            };
            if magnitude > limit {
                return None;
            }
            Some(if negative {
                if magnitude == 1_u128 << 127 {
                    i128::MIN
                } else {
                    -i128::try_from(magnitude).ok()?
                }
            } else {
                i128::try_from(magnitude).ok()?
            })
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn fin_endpoint_vertices(
    source_format: crate::brep::BrepSourceFormat,
    half_edge: &crate::brep::HalfEdge,
    previous: Option<&crate::brep::HalfEdge>,
    next: Option<&crate::brep::HalfEdge>,
) -> (Option<u32>, Option<u32>) {
    if source_format == crate::brep::BrepSourceFormat::Text {
        (previous.and_then(|fin| fin.vertex), half_edge.vertex)
    } else {
        (half_edge.vertex, next.and_then(|fin| fin.vertex))
    }
}

#[cfg(test)]
#[allow(
    clippy::similar_names,
    clippy::cast_precision_loss,
    reason = "Fixture indices and values are bounded and kept explicit for regression checks."
)]
#[allow(
    clippy::panic,
    clippy::too_many_lines,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::brep::{
        Body, BodyKind, BrepMetrics, BrepModel, BrepModelWithResolvedBodyHierarchy,
        BrepSourceFormat, CurveGeometry, CurveKind, Edge, Face, HalfEdge,
        IntersectionChartParameters, IntersectionLimitKind, Loop, MappedBodyChildren, NurbsCurve,
        NurbsSurface, PointGeometry, RawIntegerPool, RawMeshData, RawVectorPool, Region,
        RegionKind, Sense, Shell, SourceNodeRef, SurfaceGeometry, SurfaceKind, SweptSurfaceData,
        TopologyValidation, TransformNode, Vector3, Vertex,
    };

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "The fixed mesh fixture must be valid for the tessellation assertions."
    )]
    fn tessellate_brep_face_uses_embedded_mesh_triangles() {
        let source = SourceNodeRef {
            node_index: 1,
            node_type: 0,
            type_name: "MESH".into(),
            node_id: None,
            byte_range: 0..0,
        };
        let face = Face {
            id: 1,
            back_shell: 0,
            front_shell: 0,
            loops: vec![],
            surface: Some(2),
            sense: Sense::Negative,
            attributes: vec![],
            source: source.clone(),
        };
        let surface = SurfaceGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Mesh {
                source_mesh_index: 3,
                embedded: Some(RawMeshData {
                    mesh_index: 3,
                    transform_index: Some(7),
                    surface_sense: Sense::Positive,
                    psm_mesh_index: 4,
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
                transform_index: Some(7),
                transform: Some(TransformNode {
                    index: 7,
                    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    translation: [10.0, 0.0, 0.0],
                    scale: 2.0,
                    flags: 0,
                    perspective: [None; 3],
                    precision: None,
                    byte_range: 0..0,
                }),
                unembedded_source_fields: None,
            },
            source,
        };
        let mut brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: String::new(),
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
            surfaces: vec![surface],
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
        };

        let mesh = tessellate_brep_face(&brep, 1, 0.1).unwrap();
        assert_eq!(
            mesh.triangles,
            vec![[[20.0, 0.0, 0.0], [20.0, 2.0, 0.0], [22.0, 0.0, 0.0]]]
        );
        assert_eq!(mesh.normals, vec![[[0.0, 0.0, -1.0]; 3]]);
        let whole_brep_mesh = tessellate_brep_to_mesh(&brep, 0.1);
        assert_eq!(whole_brep_mesh.triangles, mesh.triangles);
        assert_eq!(whole_brep_mesh.normals, mesh.normals);

        if let SurfaceKind::Mesh {
            embedded: Some(mesh),
            ..
        } = &mut brep.surfaces[0].kind
        {
            mesh.position_indices.values[..3].copy_from_slice(&[1, 2, 2]);
        }
        assert!(tessellate_brep_face(&brep, 1, 0.1).is_none());
        if let SurfaceKind::Mesh {
            embedded: Some(mesh),
            ..
        } = &mut brep.surfaces[0].kind
        {
            mesh.position_indices.values[..3].copy_from_slice(&[0, 1, 3]);
        }
        assert!(tessellate_brep_face(&brep, 1, 0.1).is_none());
        let SurfaceKind::Mesh { embedded, .. } = &mut brep.surfaces[0].kind else {
            unreachable!();
        };
        *embedded = None;
        assert!(tessellate_brep_face(&brep, 1, 0.1).is_none());
    }

    fn intersection_trim_face(reverse: bool) -> BrepModel {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let source = brep.curves[0].source.clone();
        let point_at = |theta: f64| [theta.cos(), theta.sin(), 0.5 * theta.cos()];
        let angles = [0.15, 0.8, 1.9];
        let chart_points = angles.map(point_at);
        let chord0 = vec3_sub(chart_points[1], chart_points[0]);
        let chord1 = vec3_sub(chart_points[2], chart_points[1]);
        let length0 = vec3_norm(chord0);
        let length1 = vec3_norm(chord1);
        let radial = [chart_points[1][0], chart_points[1][1], 0.0];
        let tangent = vec3_normalize(vec3_cross([-0.5, 0.0, 1.0], radial));
        let base_parameter = 3.0;
        let base_scale = 0.7;
        let middle_parameter = base_parameter + length0 * base_scale;
        let middle_scale = base_scale * (vec3_dot(tangent, chord0) / length0)
            / (vec3_dot(tangent, chord1) / length1);
        let end_parameter = middle_parameter + length1 * middle_scale;
        let (a, b, c) = (chart_points[0], chart_points[2], [0.0, 0.0, 0.0]);
        let intersection = CurveGeometry {
            id: 50,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Intersection {
                surfaces: [10, 11],
                chart: source.clone(),
                chart_parameters: IntersectionChartParameters {
                    base_parameter: Some(base_parameter),
                    base_scale: Some(base_scale),
                    chordal_error: Some(0.4),
                    angular_error: None,
                },
                chart_points: chart_points.map(Vector3::from_array).into(),
                start: source.clone(),
                start_limit_kind: Some(IntersectionLimitKind::Limit),
                start_term_use: None,
                start_points: vec![Vector3::from_array(a)],
                end: source.clone(),
                end_limit_kind: Some(IntersectionLimitKind::Limit),
                end_term_use: None,
                end_points: vec![Vector3::from_array(b)],
                intersection_uv: None,
                intersection_data: None,
            },
            source: source.clone(),
        };
        let trims = [
            CurveGeometry {
                id: 51,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 50,
                    start_point: Vector3::from_array(a),
                    end_point: Vector3::from_array(b),
                    start_parameter: base_parameter,
                    end_parameter,
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 52,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 54,
                    start_point: Vector3::from_array(b),
                    end_point: Vector3::from_array(c),
                    start_parameter: 0.0,
                    end_parameter: 1.0,
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 53,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 55,
                    start_point: Vector3::from_array(c),
                    end_point: Vector3::from_array(a),
                    start_parameter: 0.0,
                    end_parameter: 1.0,
                },
                source: source.clone(),
            },
        ];
        let lines = [
            CurveGeometry {
                id: 54,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array(b),
                    direction: Vector3::from_array(vec3_sub(c, b)),
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 55,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array(c),
                    direction: Vector3::from_array(vec3_sub(a, c)),
                },
                source: source.clone(),
            },
        ];
        brep.curves = vec![intersection];
        brep.curves.extend(trims);
        brep.curves.extend(lines);
        brep.surfaces[0].kind = SurfaceKind::Plane {
            point: Vector3::from_array([0.0; 3]),
            normal: Vector3::from_array([-0.5, 0.0, 1.0]),
            x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        brep.surfaces.push(SurfaceGeometry {
            id: 11,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Cylinder {
                point: Vector3::from_array([0.0; 3]),
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                radius: 1.0,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source.clone(),
        });
        if reverse {
            brep.loops[0].half_edges = vec![5, 4, 3];
            for half_edge in &mut brep.half_edges {
                half_edge.sense = Sense::Negative;
            }
        } else {
            brep.loops[0].half_edges = vec![3, 4, 5];
        }
        brep.half_edges = [3, 4, 5]
            .into_iter()
            .enumerate()
            .map(|(index, id)| HalfEdge {
                id,
                loop_id: Some(2),
                forward: Some([4, 5, 3][index]),
                backward: Some([5, 3, 4][index]),
                vertex: None,
                other: None,
                edge: None,
                curve: Some([51, 52, 53][index]),
                sense: if reverse {
                    Sense::Negative
                } else {
                    Sense::Positive
                },
                dummy: false,
                source: source.clone(),
            })
            .collect();
        brep
    }

    #[allow(
        clippy::expect_used,
        reason = "This helper constructs fixed valid intersection fixtures; missing geometry is a setup failure."
    )]
    fn intersection_face_with_tolerances(
        edge_tolerance: Option<f64>,
        vertex_tolerance: Option<f64>,
        vertex_offset: f64,
        trim_offset: f64,
        duplicate_owner: bool,
    ) -> BrepModel {
        let mut brep = intersection_trim_face(false);
        let source = SourceNodeRef {
            node_index: 90,
            node_type: 1,
            type_name: "BODY".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        let body = Body {
            id: 90,
            kind: BodyKind::Solid,
            size_resolution: 1000.0,
            linear_resolution: 0.1,
            regions: vec![],
            edges: vec![60],
            vertices: vec![70, 71],
            attributes: vec![],
            source: source.clone(),
        };
        let start = [0.15_f64.cos(), 0.15_f64.sin(), 0.5 * 0.15_f64.cos()];
        let end = [1.9_f64.cos(), 1.9_f64.sin(), 0.5 * 1.9_f64.cos()];
        let mut shifted_start = start;
        shifted_start[0] += vertex_offset;
        if trim_offset != 0.0 {
            if let CurveKind::Trimmed { start_point, .. } = &mut brep.curves[1].kind {
                let mut point = start;
                point[0] += trim_offset;
                *start_point = Vector3::from_array(point);
            }
        }
        for (id, point_id, position) in [(70, 80, shifted_start), (71, 81, end)] {
            brep.vertices.push(Vertex {
                id,
                point: point_id,
                tolerance: vertex_tolerance,
                owner: source.clone(),
                source: SourceNodeRef {
                    node_index: id,
                    node_type: 12,
                    type_name: "VERTEX".to_owned(),
                    node_id: None,
                    byte_range: 0..1,
                },
            });
            brep.points.push(PointGeometry {
                id: point_id,
                position: Vector3::from_array(position),
                owner: Some(source.clone()),
                source: SourceNodeRef {
                    node_index: point_id,
                    node_type: 13,
                    type_name: "POINT".to_owned(),
                    node_id: None,
                    byte_range: 0..1,
                },
            });
        }
        brep.half_edges
            .iter_mut()
            .find(|half_edge| half_edge.id == 3)
            .expect("intersection fin")
            .vertex = Some(71);
        brep.half_edges
            .iter_mut()
            .find(|half_edge| half_edge.id == 5)
            .expect("preceding fin")
            .vertex = Some(70);
        brep.half_edges
            .iter_mut()
            .find(|half_edge| half_edge.id == 3)
            .expect("intersection fin")
            .edge = Some(60);
        brep.edges.push(Edge {
            id: 60,
            owner: source.clone(),
            half_edges: vec![3],
            start_vertex: Some(70),
            end_vertex: Some(71),
            curve: Some(50),
            tolerance: edge_tolerance,
            source: SourceNodeRef {
                node_index: 60,
                node_type: 11,
                type_name: "EDGE".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        });
        brep.bodies.push(body.clone());
        if duplicate_owner {
            let mut duplicate = body;
            duplicate.id = 91;
            brep.bodies.push(duplicate);
        }
        brep
    }

    #[test]
    fn sampled_intersection_keeps_near_distinct_boundary_points() {
        let start = [0.0, 0.0, 0.0];
        let near = [9e-6, 0.0, 0.0];
        let mut ring = vec![start];
        append_intersection_samples(&mut ring, [near, near, [1.0, 0.0, 0.0]]);
        assert_eq!(ring, vec![start, near, [1.0, 0.0, 0.0]]);

        ring.push([0.0, 9e-6, 0.0]);
        remove_ring_closure_duplicate(&mut ring, true);
        assert_eq!(ring.last(), Some(&[0.0, 9e-6, 0.0]));

        ring.push(start);
        remove_ring_closure_duplicate(&mut ring, true);
        assert_eq!(ring.last(), Some(&[0.0, 9e-6, 0.0]));

        let mut legacy_ring = vec![start, near];
        remove_ring_closure_duplicate(&mut legacy_ring, false);
        assert_eq!(legacy_ring, vec![start]);
    }

    fn two_body_face_fixture() -> BrepModel {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let source = |node_index| SourceNodeRef {
            node_index,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let mut second_face = brep.faces[0].clone();
        second_face.id = 2;
        second_face.back_shell = 31;
        second_face.front_shell = 33;
        second_face.source = source(8);
        brep.faces[0].back_shell = 30;
        brep.faces[0].front_shell = 32;
        brep.bodies = vec![
            Body {
                id: 10,
                kind: BodyKind::Solid,
                size_resolution: 1.0,
                linear_resolution: 0.1,
                regions: vec![20],
                edges: vec![],
                vertices: vec![],
                attributes: vec![],
                source: source(100),
            },
            Body {
                id: 11,
                kind: BodyKind::Solid,
                size_resolution: 1.0,
                linear_resolution: 0.1,
                regions: vec![21],
                edges: vec![],
                vertices: vec![],
                attributes: vec![],
                source: source(101),
            },
        ];
        brep.regions = vec![
            Region {
                id: 20,
                kind: RegionKind::Solid,
                body: 10,
                shells: vec![30, 32],
                source: source(200),
            },
            Region {
                id: 21,
                kind: RegionKind::Solid,
                body: 11,
                shells: vec![31, 33],
                source: source(201),
            },
        ];
        brep.shells = vec![
            Shell {
                id: 30,
                region: 20,
                back_faces: vec![1],
                front_faces: vec![],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source(300),
            },
            Shell {
                id: 31,
                region: 21,
                back_faces: vec![2],
                front_faces: vec![],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source(301),
            },
            Shell {
                id: 32,
                region: 20,
                back_faces: vec![],
                front_faces: vec![1],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source(302),
            },
            Shell {
                id: 33,
                region: 21,
                back_faces: vec![],
                front_faces: vec![2],
                wire_edges: vec![],
                isolated_vertex: None,
                source: source(303),
            },
        ];
        brep.faces.push(second_face);
        brep
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "This test's fixed BODY fixture must be valid for its ownership assertions."
    )]
    fn body_tessellation_follows_owned_face_links_and_fails_closed() {
        let brep = two_body_face_fixture();
        let body_10 = tessellate_brep_body_to_mesh(&brep, 10, 0.1).unwrap();
        let body_11 = tessellate_brep_body_to_mesh(&brep, 11, 0.1).unwrap();
        let face_1 = tessellate_brep_face(&brep, 1, 0.1).unwrap();
        let face_2 = tessellate_brep_face(&brep, 2, 0.1).unwrap();
        assert!(!face_1.triangles.is_empty());
        assert!(!face_2.triangles.is_empty());
        assert_eq!(body_10.triangles, face_1.triangles);
        assert_eq!(body_11.triangles, face_2.triangles);
        assert_eq!(
            tessellate_brep_to_mesh(&brep, 0.1).triangles,
            [face_1.triangles, face_2.triangles].concat()
        );

        let mut wrong_region_owner = brep.clone();
        wrong_region_owner.regions[0].body = 11;
        assert!(tessellate_brep_body_to_mesh(&wrong_region_owner, 10, 0.1).is_none());

        let mut unsupported_region = brep.clone();
        unsupported_region.regions[0].kind = RegionKind::Unsupported(0x3f);
        assert!(tessellate_brep_body_to_mesh(&unsupported_region, 10, 0.1).is_none());

        let mut wrong_shell_face = brep;
        wrong_shell_face.shells[0].back_faces[0] = 2;
        assert!(tessellate_brep_body_to_mesh(&wrong_shell_face, 10, 0.1).is_none());

        let mut cross_body_face = two_body_face_fixture();
        cross_body_face.faces[0].front_shell = 33;
        cross_body_face.shells[3].front_faces.push(1);
        assert!(tessellate_brep_body_to_mesh(&cross_body_face, 10, 0.1).is_none());

        let mut partial_mesh = two_body_face_fixture();
        partial_mesh.faces[1].loops = vec![999];
        assert!(tessellate_brep_body_to_mesh(&partial_mesh, 11, 0.1).is_none());

        let mut duplicate_face_id = two_body_face_fixture();
        duplicate_face_id
            .faces
            .push(duplicate_face_id.faces[0].clone());
        assert!(tessellate_brep_body_to_mesh(&duplicate_face_id, 10, 0.1).is_none());
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "This test's fixed compound fixtures must be valid before malformed cases are checked."
    )]
    fn resolved_compound_body_tessellation_flattens_children_and_rejects_bad_hierarchy() {
        let mut brep = two_body_face_fixture();
        let mut parent = brep.bodies[0].clone();
        parent.id = 12;
        parent.regions.clear();
        brep.bodies.push(parent);
        let first_child = tessellate_brep_body_to_mesh(&brep, 11, 0.1).unwrap();
        let second_child = tessellate_brep_body_to_mesh(&brep, 10, 0.1).unwrap();
        let resolved = BrepModelWithResolvedBodyHierarchy {
            model: brep,
            body_hierarchies: vec![],
            body_children: vec![MappedBodyChildren {
                parent_brep_body_id: 12,
                parent_source_node_index: 102,
                child_brep_body_ids: vec![11, 10],
            }],
        };
        let flattened = tessellate_resolved_brep_body_to_mesh(&resolved, 12, 0.1).unwrap();
        assert_eq!(
            flattened.triangles,
            [first_child.triangles, second_child.triangles].concat()
        );
        assert_eq!(
            flattened.normals,
            [first_child.normals, second_child.normals].concat()
        );
        assert_eq!(
            flattened.wire_edges,
            [first_child.wire_edges, second_child.wire_edges].concat()
        );
        let leaf_mesh = tessellate_resolved_brep_body_to_mesh(&resolved, 11, 0.1).unwrap();
        let original_leaf_mesh = tessellate_brep_body_to_mesh(&resolved.model, 11, 0.1).unwrap();
        assert_eq!(leaf_mesh.triangles, original_leaf_mesh.triangles);
        assert_eq!(leaf_mesh.normals, original_leaf_mesh.normals);
        assert_eq!(leaf_mesh.wire_edges, original_leaf_mesh.wire_edges);

        let mut duplicate_parent = resolved.clone();
        duplicate_parent
            .body_children
            .push(duplicate_parent.body_children[0].clone());
        assert!(tessellate_resolved_brep_body_to_mesh(&duplicate_parent, 12, 0.1).is_none());

        let mut missing_child = resolved.clone();
        missing_child.body_children[0].child_brep_body_ids = vec![999];
        assert!(tessellate_resolved_brep_body_to_mesh(&missing_child, 12, 0.1).is_none());

        let mut duplicate_child = resolved.clone();
        duplicate_child.body_children[0].child_brep_body_ids = vec![11, 11];
        assert!(tessellate_resolved_brep_body_to_mesh(&duplicate_child, 12, 0.1).is_none());

        let mut shared_child = resolved.clone();
        shared_child.body_children.push(MappedBodyChildren {
            parent_brep_body_id: 10,
            parent_source_node_index: 100,
            child_brep_body_ids: vec![11],
        });
        assert!(tessellate_resolved_brep_body_to_mesh(&shared_child, 12, 0.1).is_none());

        let mut cycle = resolved;
        cycle.body_children.push(MappedBodyChildren {
            parent_brep_body_id: 11,
            parent_source_node_index: 101,
            child_brep_body_ids: vec![12],
        });
        assert!(tessellate_resolved_brep_body_to_mesh(&cycle, 12, 0.1).is_none());
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed intersection fixtures must evaluate successfully for their assertions."
    )]
    fn intersection_face_boundary_is_sampled_and_rejects_unsupported_pairs() {
        let mut work_budget = 10;
        consume_intersection_chart_work(3, 1, &mut work_budget).expect("charge one chart call");
        assert_eq!(work_budget, 7);
        assert!(consume_intersection_chart_work(8, 1, &mut work_budget).is_none());
        assert_eq!(work_budget, 7);
        let mut max_work_budget = MAX_INTERSECTION_CHART_SAMPLE_WORK;
        assert!(
            consume_intersection_chart_work(
                MAX_INTERSECTION_CHART_SAMPLE_WORK + 1,
                1,
                &mut max_work_budget
            )
            .is_none()
        );
        let mut inverse_budget = 1_000;
        consume_intersection_inverse_work(3, 2, &mut inverse_budget)
            .expect("precharge both endpoint inversions");
        assert_eq!(inverse_budget, 952);
        assert!(consume_intersection_chart_work(3, 2, &mut inverse_budget).is_some());
        assert_eq!(inverse_budget, 946);
        let mut insufficient_budget = 47;
        assert!(consume_intersection_inverse_work(3, 2, &mut insufficient_budget).is_none());
        assert_eq!(insufficient_budget, 5);
        let mut cone_cylinder_budget = MAX_INTERSECTION_CHART_SAMPLE_WORK;
        let chart_points = 32;
        let full_evaluation_work = 4 * chart_points + 1;
        let expected_inverse_work = (2 * (2 * chart_points - 3) * full_evaluation_work
            + (chart_points - 1) * (full_evaluation_work + (2 * chart_points + 2) * 40))
            * 2;
        consume_cone_cylinder_inverse_work(32, &mut cone_cylinder_budget)
            .expect("precharge both bounded cone/cylinder endpoint inversions");
        assert_eq!(
            cone_cylinder_budget,
            MAX_INTERSECTION_CHART_SAMPLE_WORK - expected_inverse_work
        );
        let mut insufficient_cone_cylinder_budget = expected_inverse_work - 1;
        assert!(
            consume_cone_cylinder_inverse_work(32, &mut insufficient_cone_cylinder_budget)
                .is_none()
        );
        assert_eq!(insufficient_cone_cylinder_budget, expected_inverse_work - 1);
        let mut overflow_cone_cylinder_budget = usize::MAX;
        assert!(
            consume_cone_cylinder_inverse_work(usize::MAX, &mut overflow_cone_cylinder_budget)
                .is_none()
        );
        assert_eq!(overflow_cone_cylinder_budget, usize::MAX);
        let mut cone_cylinder = intersection_trim_face(false);
        cone_cylinder.surfaces[0].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.5,
            cos_half_angle: 0.866_025_403_784_438_6,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let intersection = &cone_cylinder.curves[0].kind;
        let CurveKind::Intersection { chart_points, .. } = intersection else {
            unreachable!();
        };
        let visits = intersection_chart_solver_visits(&cone_cylinder, intersection);
        assert_eq!(visits, 2);
        let mut budget = 10;
        consume_intersection_chart_work(chart_points.len(), visits, &mut budget)
            .expect("charge both cone-axis solver visits");
        assert_eq!(budget, 4);

        let mut overflow_budget = usize::MAX;
        assert!(consume_intersection_chart_work(usize::MAX, 2, &mut overflow_budget).is_none());
        assert_eq!(overflow_budget, usize::MAX);

        let plane_cylinder = intersection_trim_face(false);
        let intersection = &plane_cylinder.curves[0].kind;
        let CurveKind::Intersection { chart_points, .. } = intersection else {
            unreachable!();
        };
        let visits = intersection_chart_solver_visits(&plane_cylinder, intersection);
        assert_eq!(visits, 1);
        let mut plane_cylinder_budget = 10;
        consume_intersection_chart_work(chart_points.len(), visits, &mut plane_cylinder_budget)
            .expect("charge one plane-cylinder solver visit");
        assert_eq!(plane_cylinder_budget, 7);

        let mut plane_cone = plane_cylinder.clone();
        plane_cone.surfaces[1].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.5,
            cos_half_angle: 0.866_025_403_784_438_6,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        for surfaces in [[10, 11], [11, 10]] {
            let CurveKind::Intersection { surfaces: pair, .. } = &mut plane_cone.curves[0].kind
            else {
                unreachable!();
            };
            *pair = surfaces;
            let intersection = &plane_cone.curves[0].kind;
            let CurveKind::Intersection { chart_points, .. } = intersection else {
                unreachable!();
            };
            let visits = intersection_chart_solver_visits(&plane_cone, intersection);
            assert_eq!(visits, 2);
            let mut budget = 10;
            consume_intersection_chart_work(chart_points.len(), visits, &mut budget)
                .expect("charge both plane-cone solver visits");
            assert_eq!(budget, 4);
        }

        let tiny_interval = intersection_trim_face(false);
        let intersection = tiny_interval
            .curves
            .iter()
            .find(|curve| curve.id == 50)
            .expect("intersection basis curve");
        let base: f64 = 3.0;
        let next = f64::from_bits(base.to_bits() + 1);
        let mut chart_work_budget = MAX_INTERSECTION_CHART_SAMPLE_WORK;
        assert!(
            sample_intersection_trim(
                &tiny_interval,
                &intersection.kind,
                base,
                next,
                0.005,
                None,
                &mut chart_work_budget,
            )
            .is_some()
        );
        let start_candidate =
            crate::brep::evaluate_intersection_candidate(&tiny_interval, &intersection.kind, base)
                .expect("start candidate")
                .point;
        let end_candidate =
            crate::brep::evaluate_intersection_candidate(&tiny_interval, &intersection.kind, next)
                .expect("end candidate")
                .point;
        let mut chart_work_budget = MAX_INTERSECTION_CHART_SAMPLE_WORK;
        let samples = sample_intersection_trim(
            &tiny_interval,
            &intersection.kind,
            base,
            next,
            0.005,
            Some((start_candidate, end_candidate)),
            &mut chart_work_budget,
        )
        .expect("validated endpoints can seed intersection sampling");
        assert_eq!(samples.first(), Some(&start_candidate));
        assert_eq!(samples.last(), Some(&end_candidate));

        for reverse in [false, true] {
            let brep = intersection_trim_face(reverse);
            let default_mesh = tessellate_brep_to_mesh(&brep, 0.005);
            assert!(!default_mesh.triangles.is_empty());
            assert_eq!(default_mesh.wire_edges.len(), 3);
            let mesh = tessellate_brep_face_with_intersection_candidates(&brep, 1, 0.005)
                .expect("opt-in face tessellation");
            let boundary_points: Vec<_> = mesh
                .wire_edges
                .iter()
                .flat_map(|edge| edge.iter().copied())
                .filter(|point| (point[0].hypot(point[1]) - 1.0).abs() < 1e-10)
                .collect();
            assert!(
                boundary_points.len() > 4,
                "intersection must have interior samples"
            );
            let [start, end] = [
                [0.15_f64.cos(), 0.15_f64.sin(), 0.5 * 0.15_f64.cos()],
                [1.9_f64.cos(), 1.9_f64.sin(), 0.5 * 1.9_f64.cos()],
            ];
            assert!(boundary_points.iter().any(|point| {
                let chord = vec3_sub(end, start);
                let offset = vec3_sub(*point, start);
                vec3_norm(vec3_cross(chord, offset)) / vec3_norm(chord) > 0.01
            }));
        }

        let mut unsupported = intersection_trim_face(false);
        unsupported.surfaces[1].kind = SurfaceKind::Sphere {
            center: Vector3::from_array([0.0; 3]),
            radius: 1.0,
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let default_mesh = tessellate_brep_to_mesh(&unsupported, 0.005);
        assert!(!default_mesh.triangles.is_empty());
        assert_eq!(default_mesh.wire_edges.len(), 3);
        assert!(
            tessellate_brep_face_with_intersection_candidates(&unsupported, 1, 0.005).is_none()
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixture's uniquely owned body is required for this tolerance assertion."
    )]
    fn xt_null_entity_tolerance_requires_one_valid_body_owner() {
        let mut brep = intersection_face_with_tolerances(None, None, 0.0, 0.0, false);
        let body_owner = brep.bodies[0].source.clone();
        assert_eq!(xt_null_entity_tolerance(&body_owner, &brep), Ok(0.05));

        let shell_owner = SourceNodeRef {
            node_index: 120,
            node_type: 2,
            type_name: "SHELL".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        brep.regions.push(Region {
            id: 110,
            kind: RegionKind::Solid,
            body: brep.bodies[0].id,
            shells: vec![111],
            source: SourceNodeRef {
                node_index: 119,
                node_type: 3,
                type_name: "REGION".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        });
        brep.shells.push(Shell {
            id: 111,
            region: 110,
            back_faces: vec![],
            front_faces: vec![],
            wire_edges: vec![],
            isolated_vertex: None,
            source: shell_owner.clone(),
        });
        assert_eq!(xt_null_entity_tolerance(&shell_owner, &brep), Ok(0.05));

        let mut ambiguous = brep.clone();
        ambiguous.bodies.push(ambiguous.bodies[0].clone());
        assert!(xt_null_entity_tolerance(&body_owner, &ambiguous).is_err());
        assert!(xt_null_entity_tolerance(&shell_owner, &ambiguous).is_err());

        let mut invalid = brep.clone();
        invalid.bodies[0].linear_resolution = f64::INFINITY;
        assert!(xt_null_entity_tolerance(&body_owner, &invalid).is_err());
        let missing = SourceNodeRef {
            node_index: 999,
            ..body_owner
        };
        assert!(xt_null_entity_tolerance(&missing, &brep).is_err());

        brep.source_format = BrepSourceFormat::Binary;
        assert!(xt_null_entity_tolerance(&shell_owner, &brep).is_err());
        let edge_map = brep.edges.iter().map(|edge| (edge.id, edge)).collect();
        let half_edge = brep
            .half_edges
            .iter()
            .find(|half_edge| half_edge.id == 3)
            .expect("intersection fin");
        assert_eq!(
            intersection_edge_tolerance(half_edge, &edge_map, &brep),
            Ok(0.0)
        );
    }

    #[test]
    fn opt_in_intersection_sampling_uses_xt_null_tolerance_but_keeps_explicit_values() {
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(None, Some(0.001), 0.0, 0.01, false),
                1,
                0.005,
            )
            .is_none()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(Some(0.001), None, 0.01, 0.0, false),
                1,
                0.005,
            )
            .is_some()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(Some(0.001), Some(0.001), 0.01, 0.0, false),
                1,
                0.005,
            )
            .is_none()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(Some(0.1), Some(0.001), 0.0, 0.01, false),
                1,
                0.005,
            )
            .is_none()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(Some(0.1), Some(0.001), 0.01, 0.0, false),
                1,
                0.005,
            )
            .is_none()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(Some(0.1), Some(0.001), 0.0005, 0.0, false),
                1,
                0.005,
            )
            .is_some()
        );
        assert!(
            tessellate_brep_face_with_intersection_candidates(
                &intersection_face_with_tolerances(None, Some(0.001), 0.0, 0.01, true),
                1,
                0.005,
            )
            .is_none()
        );
    }

    #[test]
    fn full_polyline_tessellation_uses_source_vertices_and_closure_flag() {
        let source = SourceNodeRef {
            node_index: 1,
            node_type: 119,
            type_name: "POLYLINE".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        let open = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 1.0, 0.0]),
            ]
            .into(),
            closed: false,
            sense: Sense::Positive,
            base_parameter: 17.0,
            source: source.clone(),
        };
        assert_eq!(
            full_polyline_points(&open, Sense::Positive),
            Some(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
        );
        assert_eq!(
            full_polyline_points(&open, Sense::Negative),
            Some(vec![[1.0, 1.0, 0.0], [1.0, 0.0, 0.0]])
        );

        let closed = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([0.0, 1.0, 0.0]),
                Vector3::from_array([0.0, 0.0, 0.0]),
            ]
            .into(),
            closed: true,
            sense: Sense::Positive,
            base_parameter: 17.0,
            source,
        };
        assert_eq!(
            full_polyline_points(&closed, Sense::Positive),
            Some(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
        );
        assert_eq!(full_polyline_points(&closed, Sense::Unknown), None);

        let close_but_distinct = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([0.0, 1.0, 0.0]),
                Vector3::from_array([1e-6, 0.0, 0.0]),
            ]
            .into(),
            ..closed.clone()
        };
        assert_eq!(
            full_polyline_points(&close_but_distinct, Sense::Positive),
            Some(vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1e-6, 0.0, 0.0],
            ])
        );

        let unknown_polyline_sense = PolylineData {
            sense: Sense::Unknown,
            ..closed
        };
        assert_eq!(
            full_polyline_points(&unknown_polyline_sense, Sense::Positive),
            None
        );
    }

    #[test]
    fn trimmed_open_polyline_clips_at_unique_vertices_and_orients_to_edge() {
        let polyline = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 1.0, 0.0]),
                Vector3::from_array([2.0, 1.0, 0.0]),
            ]
            .into(),
            closed: false,
            sense: Sense::Positive,
            base_parameter: 17.0,
            source: SourceNodeRef {
                node_index: 1,
                node_type: 119,
                type_name: "POLYLINE".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        };
        let start = [1.0, 0.0, 0.0];
        let end = [2.0, 1.0, 0.0];
        assert_eq!(
            trimmed_polyline_points(&polyline, Sense::Positive, start, end),
            Some(vec![start, [1.0, 1.0, 0.0]])
        );
        assert_eq!(
            trimmed_polyline_points(&polyline, Sense::Negative, start, end),
            Some(vec![end, [1.0, 1.0, 0.0]])
        );
        let negative_sense = PolylineData {
            sense: Sense::Negative,
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(&negative_sense, Sense::Positive, end, start),
            Some(vec![end, [1.0, 1.0, 0.0]])
        );
        assert_eq!(
            trimmed_polyline_points(&negative_sense, Sense::Negative, end, start),
            Some(vec![start, [1.0, 1.0, 0.0]])
        );
    }

    #[test]
    fn trimmed_polyline_rejects_ambiguous_closed_and_unknown_sense_inputs() {
        let open = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
            ]
            .into(),
            closed: false,
            sense: Sense::Positive,
            base_parameter: 0.0,
            source: SourceNodeRef {
                node_index: 1,
                node_type: 119,
                type_name: "POLYLINE".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        };
        assert_eq!(
            trimmed_polyline_points(&open, Sense::Positive, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            None
        );
        assert_eq!(
            trimmed_polyline_points(&open, Sense::Unknown, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            None
        );
        let closed = PolylineData {
            closed: true,
            ..open
        };
        assert_eq!(
            trimmed_polyline_points(&closed, Sense::Positive, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn trimmed_closed_polyline_wraps_with_edge_sense_and_rejects_ambiguous_locator() {
        let closed = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
                Vector3::from_array([0.0, 2.0, 0.0]),
                Vector3::from_array([0.0, 0.0, 0.0]),
            ]
            .into(),
            closed: true,
            sense: Sense::Positive,
            base_parameter: 0.0,
            source: SourceNodeRef {
                node_index: 1,
                node_type: 119,
                type_name: "POLYLINE".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        };
        let start = [0.0, 1.0, 0.0];
        let end = [1.0, 0.0, 0.0];
        assert_eq!(
            trimmed_polyline_points(&closed, Sense::Positive, start, end),
            Some(vec![start, [0.0, 0.0, 0.0]])
        );
        assert_eq!(
            trimmed_polyline_points(&closed, Sense::Negative, start, end),
            Some(vec![end, [0.0, 0.0, 0.0]])
        );
        let forward_start = [1.0, 0.0, 0.0];
        let forward_end = [0.0, 1.0, 0.0];
        assert_eq!(
            trimmed_polyline_points(&closed, Sense::Positive, forward_start, forward_end),
            Some(vec![
                forward_start,
                [2.0, 0.0, 0.0],
                [2.0, 2.0, 0.0],
                [0.0, 2.0, 0.0]
            ])
        );
        let reverse_sense = PolylineData {
            sense: Sense::Negative,
            ..closed.clone()
        };
        assert_eq!(
            trimmed_polyline_points(&reverse_sense, Sense::Positive, start, end),
            Some(vec![
                start,
                [0.0, 2.0, 0.0],
                [2.0, 2.0, 0.0],
                [2.0, 0.0, 0.0]
            ])
        );
        assert_eq!(
            trimmed_polyline_points(&reverse_sense, Sense::Positive, forward_start, forward_end),
            Some(vec![forward_start, [0.0, 0.0, 0.0]])
        );

        let crossing = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
                Vector3::from_array([0.0, 2.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
            ]
            .into(),
            ..closed
        };
        assert_eq!(
            trimmed_polyline_points(&crossing, Sense::Positive, [1.0, 1.0, 0.0], [0.0, 2.0, 0.0]),
            None
        );
    }

    #[test]
    fn trimmed_open_polyline_accepts_exact_interior_points_and_rejects_ambiguity_or_overflow() {
        let polyline = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
            ]
            .into(),
            closed: false,
            sense: Sense::Positive,
            base_parameter: 0.0,
            source: SourceNodeRef {
                node_index: 1,
                node_type: 119,
                type_name: "POLYLINE".to_owned(),
                node_id: None,
                byte_range: 0..1,
            },
        };
        let start = [0.5, 0.0, 0.0];
        let end = [2.0, 1.5, 0.0];
        assert_eq!(
            trimmed_polyline_points(&polyline, Sense::Positive, start, end),
            Some(vec![start, [2.0, 0.0, 0.0]])
        );
        assert_eq!(
            trimmed_polyline_points(&polyline, Sense::Negative, start, end),
            Some(vec![end, [2.0, 0.0, 0.0]])
        );
        let reverse_curve = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
                Vector3::from_array([4.0, 2.0, 0.0]),
                Vector3::from_array([4.0, 4.0, 0.0]),
            ]
            .into(),
            sense: Sense::Negative,
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(
                &reverse_curve,
                Sense::Positive,
                [4.0, 3.0, 0.0],
                [1.0, 0.0, 0.0]
            ),
            Some(vec![
                [4.0, 3.0, 0.0],
                [4.0, 2.0, 0.0],
                [2.0, 2.0, 0.0],
                [2.0, 0.0, 0.0]
            ])
        );
        assert_eq!(
            exact_point_on_segment([0.5, 0.5, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            Some(true)
        );
        assert_eq!(
            exact_point_on_segment(
                [0.5, f64::from_bits(0.5_f64.to_bits() + 1), 0.0],
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0]
            ),
            Some(false)
        );
        let crossing = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
                Vector3::from_array([0.5, 2.0, 0.0]),
                Vector3::from_array([0.5, -1.0, 0.0]),
            ]
            .into(),
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(&crossing, Sense::Positive, start, end),
            None
        );
        let overlapping = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([3.0, 0.0, 0.0]),
            ]
            .into(),
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(
                &overlapping,
                Sense::Positive,
                [0.5, 0.0, 0.0],
                [2.5, 0.0, 0.0]
            ),
            Some(vec![[0.5, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
        );
        assert_eq!(
            trimmed_polyline_points(
                &overlapping,
                Sense::Positive,
                [1.5, 0.0, 0.0],
                [2.5, 0.0, 0.0]
            ),
            None
        );
        let crossing_vertex = PolylineData {
            points: [
                Vector3::from_array([0.0, 0.0, 0.0]),
                Vector3::from_array([1.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 0.0, 0.0]),
                Vector3::from_array([2.0, 2.0, 0.0]),
                Vector3::from_array([0.0, 1.0, 0.0]),
                Vector3::from_array([2.0, -1.0, 0.0]),
            ]
            .into(),
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(
                &crossing_vertex,
                Sense::Positive,
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0]
            ),
            None
        );
        let over_budget = PolylineData {
            points: (0..=MAX_TRIMMED_POLYLINE_SEGMENTS + 1)
                .map(|index| Vector3::from_array([index as f64, 0.0, 0.0]))
                .collect(),
            ..polyline.clone()
        };
        assert_eq!(
            trimmed_polyline_points(
                &over_budget,
                Sense::Positive,
                [0.5, 0.0, 0.0],
                [MAX_TRIMMED_POLYLINE_SEGMENTS as f64 + 0.5, 0.0, 0.0]
            ),
            None
        );
        assert_eq!(
            exact_point_on_segment([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1e308, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn face_tessellation_uses_only_the_trimmed_open_polyline_segments() {
        let source = SourceNodeRef {
            node_index: 1,
            node_type: 119,
            type_name: "POLYLINE".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [1.0, 1.0, 0.0];
        let d = [0.0, 1.0, 0.0];
        let trim_start = [0.5, 0.0, 0.0];
        let trim_end = [1.0, 0.5, 0.0];
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        brep.faces[0].surface = Some(10);
        brep.surfaces[0].kind = SurfaceKind::Plane {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        brep.loops[0].half_edges = vec![3, 4, 5];
        brep.half_edges = [3, 4, 5]
            .into_iter()
            .enumerate()
            .map(|(index, id)| HalfEdge {
                id,
                loop_id: Some(2),
                forward: Some([4, 5, 3][index]),
                backward: Some([5, 3, 4][index]),
                vertex: None,
                other: None,
                edge: None,
                curve: Some([14, 15, 16][index]),
                sense: Sense::Positive,
                dummy: false,
                source: source.clone(),
            })
            .collect();
        brep.curves = vec![
            CurveGeometry {
                id: 11,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Polyline(PolylineData {
                    points: [
                        Vector3::from_array([-1.0, 0.0, 0.0]),
                        Vector3::from_array(a),
                        Vector3::from_array(b),
                        Vector3::from_array(c),
                        Vector3::from_array([2.0, 1.0, 0.0]),
                    ]
                    .into(),
                    closed: false,
                    sense: Sense::Positive,
                    base_parameter: 17.0,
                    source: source.clone(),
                }),
                source: source.clone(),
            },
            CurveGeometry {
                id: 12,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array(trim_end),
                    direction: Vector3::from_array([-1.0, 0.5, 0.0]),
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 13,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array(d),
                    direction: Vector3::from_array([0.5, -1.0, 0.0]),
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 14,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 11,
                    start_point: Vector3::from_array(trim_start),
                    end_point: Vector3::from_array(trim_end),
                    start_parameter: 18.0,
                    end_parameter: 20.0,
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 15,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 12,
                    start_point: Vector3::from_array(trim_end),
                    end_point: Vector3::from_array(d),
                    start_parameter: 0.0,
                    end_parameter: 1.0,
                },
                source: source.clone(),
            },
            CurveGeometry {
                id: 16,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Trimmed {
                    basis_curve: 13,
                    start_point: Vector3::from_array(d),
                    end_point: Vector3::from_array(trim_start),
                    start_parameter: 0.0,
                    end_parameter: 1.0,
                },
                source,
            },
        ];

        let mesh = tessellate_brep_to_mesh(&brep, 0.01);
        assert!(!mesh.triangles.is_empty());
        assert_eq!(
            mesh.wire_edges,
            vec![
                [trim_start, b],
                [b, trim_end],
                [trim_end, d],
                [d, trim_start]
            ]
        );
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .all(|point| point[0] >= -1e-9
                    && point[0] <= 1.0 + 1e-9
                    && point[1] >= -1e-9
                    && point[1] <= 1.0 + 1e-9
                    && point[2].abs() <= 1e-9)
        );
        let area = mesh
            .triangles
            .iter()
            .map(|triangle| {
                0.5 * vec3_norm(vec3_cross(
                    vec3_sub(triangle[1], triangle[0]),
                    vec3_sub(triangle[2], triangle[0]),
                ))
            })
            .sum::<f64>();
        assert!((area - 0.5).abs() < 1e-9, "clipped face area was {area}");

        let mut reversed = brep.clone();
        if let CurveKind::Polyline(polyline) = &mut reversed.curves[0].kind {
            polyline.sense = Sense::Negative;
        }
        if let CurveKind::Trimmed {
            start_point,
            end_point,
            ..
        } = &mut reversed.curves[3].kind
        {
            std::mem::swap(start_point, end_point);
        }
        reversed.half_edges[0].sense = Sense::Negative;
        assert!(
            !tessellate_brep_to_mesh(&reversed, 0.01)
                .triangles
                .is_empty(),
            "reversed open-polyline trim should tessellate"
        );

        let mut ambiguous = brep;
        if let CurveKind::Polyline(polyline) = &mut ambiguous.curves[0].kind {
            polyline.points.push(Vector3::from_array([0.5, 1.0, 0.0]));
            polyline.points.push(Vector3::from_array([0.5, -1.0, 0.0]));
        }
        assert!(
            tessellate_brep_to_mesh(&ambiguous, 0.01)
                .triangles
                .is_empty(),
            "an interior trim point at a segment intersection must fail"
        );
    }

    #[test]
    fn trimmed_polyline_curve_evaluation_does_not_fall_back_to_endpoint_chord() {
        let source = SourceNodeRef {
            node_index: 1,
            node_type: 119,
            type_name: "POLYLINE".to_owned(),
            node_id: None,
            byte_range: 0..1,
        };
        let basis = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Polyline(PolylineData {
                points: [
                    Vector3::from_array([0.0, 0.0, 0.0]),
                    Vector3::from_array([1.0, 0.0, 0.0]),
                    Vector3::from_array([1.0, 1.0, 0.0]),
                ]
                .into(),
                closed: false,
                sense: Sense::Positive,
                base_parameter: 17.0,
                source: source.clone(),
            }),
            source: source.clone(),
        };
        let trimmed = CurveGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_point: Vector3::from_array([0.0, 0.0, 0.0]),
                end_point: Vector3::from_array([1.0, 1.0, 0.0]),
                start_parameter: 17.0,
                end_parameter: 19.0,
            },
            source,
        };
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        brep.curves = vec![basis, trimmed];

        assert!(evaluate_curve_in_brep(&brep, &brep.curves[1], 0.5).is_none());
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed refinement fixture must produce its expected interface."
    )]
    fn uv_refinement_keeps_adjacent_triangle_interface_conforming() {
        let adjacent = [
            [[0.0, 0.0], [1.0, 0.0], [2.0, 1.0]],
            [[1.0, 0.0], [0.0, 0.0], [-3.0, -1.0]],
        ];
        let triangles = subdivide_triangles_uv(&adjacent, [0.25, 1.0], false, 500_000)
            .expect("adjacent UV triangles refine within the budget");
        let mut interface_partitions = [Vec::new(), Vec::new()];
        let mut output_areas = [0.0, 0.0];

        for subtriangle in triangles {
            let centroid_v = (subtriangle[0][1] + subtriangle[1][1] + subtriangle[2][1]) / 3.0;
            let source_index = usize::from(centroid_v < 0.0);
            let area = signed_area_2d(&subtriangle);
            assert!(
                area * signed_area_2d(&adjacent[source_index]) > 0.0,
                "refinement changed triangle orientation"
            );
            output_areas[source_index] += area;
            for index in 0..3 {
                let a = subtriangle[index];
                let b = subtriangle[(index + 1) % 3];
                if a[1] == 0.0 && b[1] == 0.0 {
                    interface_partitions[source_index].push(if a[0] < b[0] {
                        (a[0], b[0])
                    } else {
                        (b[0], a[0])
                    });
                }
            }
        }

        for source_index in 0..2 {
            assert!(
                (output_areas[source_index] - signed_area_2d(&adjacent[source_index])).abs()
                    < 1e-10
            );
            interface_partitions[source_index].sort_by(|left, right| left.0.total_cmp(&right.0));
        }
        assert_eq!(
            interface_partitions[0], interface_partitions[1],
            "shared UV edge [0, 0]-[1, 0] has hanging refinement vertices"
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed signed-zero fixture must produce its expected interface."
    )]
    fn uv_refinement_canonicalizes_mixed_signed_zero_on_vertical_interfaces() {
        let adjacent = [
            [[-0.0, 0.0], [0.0, 1.0], [-2.0, 0.5]],
            [[0.0, 1.0], [-0.0, 0.0], [2.0, 0.5]],
        ];
        let triangles = subdivide_triangles_uv(&adjacent, [0.25, 1.0], false, 500_000)
            .expect("signed-zero adjacent triangles refine within the budget");
        let mut interface_partitions = [Vec::new(), Vec::new()];
        let mut output_areas = [0.0, 0.0];

        for subtriangle in triangles {
            let centroid_u = (subtriangle[0][0] + subtriangle[1][0] + subtriangle[2][0]) / 3.0;
            let source_index = usize::from(centroid_u > 0.0);
            let area = signed_area_2d(&subtriangle);
            assert!(
                area * signed_area_2d(&adjacent[source_index]) > 0.0,
                "refinement changed triangle orientation"
            );
            output_areas[source_index] += area;
            for index in 0..3 {
                let a = subtriangle[index];
                let b = subtriangle[(index + 1) % 3];
                if a[0] == 0.0 && b[0] == 0.0 {
                    interface_partitions[source_index].push(if a[1] < b[1] {
                        (a[1], b[1])
                    } else {
                        (b[1], a[1])
                    });
                }
            }
        }

        for source_index in 0..2 {
            assert!(
                (output_areas[source_index] - signed_area_2d(&adjacent[source_index])).abs()
                    < 1e-10
            );
            interface_partitions[source_index].sort_by(|left, right| left.0.total_cmp(&right.0));
        }
        assert_eq!(
            interface_partitions[0], interface_partitions[1],
            "vertical shared edge x=0 has hanging refinement vertices"
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed reversed-winding fixture must refine for its output assertions."
    )]
    fn uv_refinement_preserves_reversed_winding_and_rejects_limits() {
        let clockwise = [[0.0, 0.0], [0.0, 1.0], [4.0, 0.0]];
        let input_area = signed_area_2d(&clockwise);
        let refined = subdivide_triangles_uv(&[clockwise], [0.5, 0.5], true, 500_000)
            .expect("clockwise triangle refines within limits");
        let mut output_area = 0.0;
        for triangle in &refined {
            let area = signed_area_2d(triangle);
            assert!(
                area * input_area > 0.0,
                "refinement changed clockwise winding"
            );
            output_area += area;
        }
        assert!((output_area - input_area).abs() < 1e-10);

        let over_budget = subdivide_triangles_uv(&[clockwise], [0.5, 0.5], true, 2);
        assert!(
            over_budget.is_none(),
            "refinement must reject an exceeded face budget"
        );

        let beyond_depth = [[[0.0, 0.0], [100.0, 0.0], [0.0, 1.0]]];
        let unresolved = subdivide_triangles_uv(&beyond_depth, [1e-9, 1.0], false, 500_000);
        assert!(
            unresolved.is_none(),
            "refinement must reject spans left after six rounds"
        );
    }

    #[test]
    fn text_fin_uses_forward_vertex_and_binary_keeps_neighbor_pairing() {
        let fin = |vertex: Option<u32>| HalfEdge {
            id: vertex.unwrap_or_default(),
            loop_id: None,
            forward: None,
            backward: None,
            vertex,
            other: None,
            edge: None,
            curve: None,
            sense: Sense::Negative,
            dummy: false,
            source: SourceNodeRef {
                node_index: 0,
                node_type: 17,
                type_name: "HALFEDGE".into(),
                node_id: None,
                byte_range: 0..0,
            },
        };
        let previous = fin(Some(10));
        let current = fin(Some(20));
        let next = fin(Some(30));

        assert_eq!(
            fin_endpoint_vertices(
                BrepSourceFormat::Text,
                &current,
                Some(&previous),
                Some(&next)
            ),
            (Some(10), Some(20))
        );
        assert_eq!(
            fin_endpoint_vertices(
                BrepSourceFormat::Binary,
                &current,
                Some(&previous),
                Some(&next)
            ),
            (Some(20), Some(30))
        );
    }

    fn ruled_sweep_face_with_explicit_uv_trim() -> BrepModel {
        let source = || SourceNodeRef {
            node_index: 7,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let section = NurbsCurve {
            degree: 1,
            control_vertex_count: 2,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![vec![0.0, 0.0, 0.0], vec![1.0, 0.0, 0.0]],
            knots: vec![0.0, 1.0],
            knot_multiplicities: vec![2, 2],
            sources: vec![],
        };
        let uv_trim = NurbsCurve {
            degree: 1,
            control_vertex_count: 5,
            vertex_dimension: 2,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![
                vec![-0.5, 0.0],
                vec![1.5, 0.0],
                vec![1.5, 1.0],
                vec![-0.5, 1.0],
                vec![-0.5, 0.0],
            ],
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            knot_multiplicities: vec![2, 1, 1, 1, 2],
            sources: vec![],
        };
        BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 1,
                back_shell: 0,
                front_shell: 0,
                loops: vec![2],
                surface: Some(10),
                sense: Sense::Positive,
                attributes: vec![],
                source: source(),
            }],
            loops: vec![Loop {
                id: 2,
                face: 1,
                half_edges: vec![3],
                source: source(),
            }],
            half_edges: vec![HalfEdge {
                id: 3,
                loop_id: Some(2),
                forward: Some(3),
                backward: Some(3),
                vertex: None,
                other: None,
                edge: None,
                curve: Some(13),
                sense: Sense::Positive,
                dummy: false,
                source: source(),
            }],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![
                CurveGeometry {
                    id: 11,
                    sense: Sense::Positive,
                    owner: None,
                    kind: CurveKind::Nurbs(section),
                    source: source(),
                },
                CurveGeometry {
                    id: 12,
                    sense: Sense::Positive,
                    owner: None,
                    kind: CurveKind::Nurbs(uv_trim),
                    source: source(),
                },
                CurveGeometry {
                    id: 13,
                    sense: Sense::Positive,
                    owner: None,
                    kind: CurveKind::SurfaceParametric {
                        surface: 10,
                        parameter_curve: 12,
                        original_curve: None,
                        tolerance_to_original: None,
                    },
                    source: source(),
                },
            ],
            surfaces: vec![SurfaceGeometry {
                id: 10,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Swept {
                    section: 11,
                    direction: Vector3::from_array([0.0, 0.0, 1.0]),
                },
                source: source(),
            }],
            topology: TopologyValidation {
                valid: true,
                closed_loop_count: 1,
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
        clippy::expect_used,
        reason = "The fixed ruled-sweep fixture must evaluate for its direction assertions."
    )]
    fn generic_surface_evaluation_maps_ruled_sweep_direction() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: brep.curves[0].id,
            direction: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        let evaluated = evaluate_surface(&brep, &brep.surfaces[0].kind, 0.5, 0.25)
            .expect("evaluate mapped ruled sweep");
        assert!((evaluated.point[0] - 0.5).abs() < 1e-10);
        assert!(evaluated.point[1].abs() < 1e-10);
        assert!((evaluated.point[2] - 0.25).abs() < 1e-10);
        assert!(evaluated.normal[0].abs() < 1e-10);
        assert!((evaluated.normal[1] + 1.0).abs() < 1e-10);
        assert!(evaluated.normal[2].abs() < 1e-10);

        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: 999,
            direction: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        assert!(evaluate_surface(&brep, &brep.surfaces[0].kind, 0.5, 0.25).is_none());
        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: brep.curves[0].id,
            direction: Vector3::from_array([0.0, 0.0, 2.0]),
        };
        assert!(evaluate_surface(&brep, &brep.surfaces[0].kind, 0.5, 0.25).is_none());
        assert!(evaluate_surface(&brep, &brep.surfaces[0].kind, f64::NAN, 0.25).is_none());
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed ruled-sweep fixture must evaluate and tessellate for source-metadata parity assertions."
    )]
    fn swept_with_source_retains_scale_and_matches_swept_geometry() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let section = brep.curves[0].id;
        let direction = Vector3::from_array([0.0, 0.0, 1.0]);
        let original_kind = SurfaceKind::Swept { section, direction };
        let source_kind = SurfaceKind::SweptWithSource {
            section,
            sweep: direction,
            scale: Some(2.5),
        };
        let original_eval = evaluate_surface(&brep, &original_kind, 0.5, 0.25)
            .expect("evaluate existing swept representation");
        let source_eval = evaluate_surface(&brep, &source_kind, 0.5, 0.25)
            .expect("evaluate source-retaining swept representation");
        assert_eq!(source_eval, original_eval);
        let SurfaceKind::SweptWithSource { scale, .. } = &source_kind else {
            unreachable!();
        };
        assert_eq!(*scale, Some(2.5));

        brep.surfaces[0].kind = source_kind;
        assert!(tessellate_brep_face_with_explicit_uv(&brep, 7, 0.01).is_some());
        brep.surfaces[0].kind = SurfaceKind::SweptWithSource {
            section,
            sweep: Vector3::from_array([0.0, 0.0, 2.0]),
            scale: Some(2.5),
        };
        assert!(tessellate_brep_face_with_explicit_uv(&brep, 7, 0.01).is_none());
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed analytic-curve fixtures must evaluate for their exact support assertions."
    )]
    fn generic_surface_evaluation_and_point_support_analytic_line_circle_ellipse() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        // Line section
        brep.curves[0].kind = CurveKind::Line {
            point: Vector3::from_array([1.0, 2.0, 3.0]),
            direction: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: brep.curves[0].id,
            direction: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        // C(u) = [1.0 + 2.0, 2.0, 3.0] = [3.0, 2.0, 3.0]
        // v = 4.0, D = [0, 0, 1] -> R(2.0, 4.0) = [3.0, 2.0, 7.0]
        // Tangent = [1, 0, 0], Direction = [0, 0, 1] -> Normal = cross([1, 0, 0], [0, 0, 1]) = [0, -1, 0]
        let eval_line = evaluate_surface(&brep, &brep.surfaces[0].kind, 2.0, 4.0)
            .expect("evaluate line swept surface");
        assert!((eval_line.point[0] - 3.0).abs() < 1e-10);
        assert!((eval_line.point[1] - 2.0).abs() < 1e-10);
        assert!((eval_line.point[2] - 7.0).abs() < 1e-10);
        assert!((eval_line.normal[0]).abs() < 1e-10);
        assert!((eval_line.normal[1] + 1.0).abs() < 1e-10);
        assert!((eval_line.normal[2]).abs() < 1e-10);

        let pt_line = surface_point_in_brep(
            &brep,
            &brep.surfaces[0].kind,
            [2.0, 4.0],
            Some([0.0, 0.0, 1.0]),
        )
        .expect("surface point in brep for line section");
        assert!((pt_line[0] - 3.0).abs() < 1e-10);
        assert!((pt_line[1] - 2.0).abs() < 1e-10);
        assert!((pt_line[2] - 7.0).abs() < 1e-10);

        // Circle section
        brep.curves[0].kind = CurveKind::Circle {
            center: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 5.0,
        };
        // u = 0 -> C(0) = [5, 0, 0], tangent = [0, 5, 0]
        // v = 2 -> point = [5, 0, 2]
        // cross(tangent/|tangent|, direction) = cross([0, 1, 0], [0, 0, 1]) = [1, 0, 0]
        let eval_circle = evaluate_surface(&brep, &brep.surfaces[0].kind, 0.0, 2.0)
            .expect("evaluate circle swept surface");
        assert!((eval_circle.point[0] - 5.0).abs() < 1e-10);
        assert!((eval_circle.point[1]).abs() < 1e-10);
        assert!((eval_circle.point[2] - 2.0).abs() < 1e-10);
        assert!((eval_circle.normal[0] - 1.0).abs() < 1e-10);
        assert!((eval_circle.normal[1]).abs() < 1e-10);
        assert!((eval_circle.normal[2]).abs() < 1e-10);

        let pt_circle = surface_point_in_brep(
            &brep,
            &brep.surfaces[0].kind,
            [0.0, 2.0],
            Some([0.0, 0.0, 1.0]),
        )
        .expect("surface point in brep for circle section");
        assert!((pt_circle[0] - 5.0).abs() < 1e-10);
        assert!((pt_circle[1]).abs() < 1e-10);
        assert!((pt_circle[2] - 2.0).abs() < 1e-10);

        // Ellipse section
        brep.curves[0].kind = CurveKind::Ellipse {
            center: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
        // u = 0 -> C(0) = [4, 0, 0], tangent = [0, 2, 0]
        // v = 3 -> point = [4, 0, 3]
        // cross([0, 1, 0], [0, 0, 1]) = [1, 0, 0]
        let eval_ellipse = evaluate_surface(&brep, &brep.surfaces[0].kind, 0.0, 3.0)
            .expect("evaluate ellipse swept surface");
        assert!((eval_ellipse.point[0] - 4.0).abs() < 1e-10);
        assert!((eval_ellipse.point[1]).abs() < 1e-10);
        assert!((eval_ellipse.point[2] - 3.0).abs() < 1e-10);
        assert!((eval_ellipse.normal[0] - 1.0).abs() < 1e-10);
        assert!((eval_ellipse.normal[1]).abs() < 1e-10);
        assert!((eval_ellipse.normal[2]).abs() < 1e-10);
    }

    fn mapped_xt_swept_offset_fixture(
        basis_sense: Sense,
        direction: [f64; 3],
    ) -> (BrepModel, CurveGeometry, SweptSurfaceData) {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let section = &mut brep.curves[0];
        section.source.node_index = 11;
        section.source.node_type = 134;
        section.source.type_name = "B_CURVE".into();
        let basis_source = SourceNodeRef {
            node_index: 10,
            node_type: 67,
            type_name: "SWEPT_SURF".into(),
            node_id: Some(10),
            byte_range: 0..1,
        };
        let basis = &mut brep.surfaces[0];
        basis.sense = basis_sense;
        basis.source = basis_source.clone();
        basis.kind = SurfaceKind::Swept {
            section: section.id,
            direction: Vector3::from_array(direction),
        };
        let offset_surface = SurfaceGeometry {
            id: 14,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Offset {
                basis_surface: basis.id,
                offset: 0.25,
            },
            source: SourceNodeRef {
                node_index: 12,
                node_type: 60,
                type_name: "OFFSET_SURF".into(),
                node_id: Some(12),
                byte_range: 1..2,
            },
        };
        let swept = SweptSurfaceData {
            source: basis_source,
            section: Some(section.source.node_index),
            sweep: direction.map(Some),
            scale: Some(1.0),
        };
        let section = section.clone();
        brep.surfaces.push(offset_surface);
        (brep, section, swept)
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions verify exact source direction matching and preserve IEEE equality semantics."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed mapped-sweep fixture must be valid for candidate checks."
    )]
    fn mapped_xt_swept_offset_candidate_checks_links_senses_and_degeneracy() {
        for (sense, expected_y) in [(Sense::Positive, -0.25), (Sense::Negative, 0.25)] {
            let (brep, section, swept) = mapped_xt_swept_offset_fixture(sense, [0.0, 0.0, 1.0]);
            let evaluation = evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &brep.surfaces[1],
                &swept,
                &section,
                -0.25,
                0.5,
            )
            .expect("linked endpoint-continued offset evaluation");
            assert!((evaluation.point[0] + 0.25).abs() < 1e-12);
            assert!((evaluation.point[1] - expected_y).abs() < 1e-12);
            assert!((evaluation.point[2] - 0.5).abs() < 1e-12);
            assert_eq!(evaluation.normal, [0.0, -1.0, 0.0]);
        }

        let (brep, section, swept) =
            mapped_xt_swept_offset_fixture(Sense::Positive, [0.0, 0.0, 1.0]);
        let mut unsupported_basis_brep = brep.clone();
        unsupported_basis_brep.surfaces[0].kind = SurfaceKind::Unsupported {
            type_name: "SWEPT_SURF".into(),
        };
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &unsupported_basis_brep,
                &unsupported_basis_brep.surfaces[1],
                &swept,
                &section,
                -0.25,
                0.5,
            )
            .is_some()
        );
        unsupported_basis_brep.surfaces[0].kind = SurfaceKind::Unsupported {
            type_name: "NURBS_SURF".into(),
        };
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &unsupported_basis_brep,
                &unsupported_basis_brep.surfaces[1],
                &swept,
                &section,
                -0.25,
                0.5,
            )
            .is_none()
        );
        let mut invalid_basis_brep = brep.clone();
        let SurfaceKind::Offset { basis_surface, .. } = &mut invalid_basis_brep.surfaces[1].kind
        else {
            panic!("synthetic offset surface");
        };
        *basis_surface = 999;
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &invalid_basis_brep,
                &invalid_basis_brep.surfaces[1],
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let mut unlinked_offset = brep.surfaces[1].clone();
        unlinked_offset.source.node_index += 1;
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &unlinked_offset,
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let mut invalid_source = swept.clone();
        invalid_source.source.node_index += 1;
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &brep.surfaces[1],
                &invalid_source,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let mut invalid_section = swept.clone();
        invalid_section.section = Some(section.source.node_index + 1);
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &brep.surfaces[1],
                &invalid_section,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let mut invalid_direction = swept.clone();
        invalid_direction.sweep = [Some(0.0), Some(0.0), Some(2.0)];
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &brep.surfaces[1],
                &invalid_direction,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let mut nan_offset_brep = brep.clone();
        let SurfaceKind::Offset { offset, .. } = &mut nan_offset_brep.surfaces[1].kind else {
            panic!("synthetic offset surface");
        };
        *offset = f64::NAN;
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &nan_offset_brep,
                &nan_offset_brep.surfaces[1],
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &brep,
                &brep.surfaces[1],
                &swept,
                &section,
                f64::NAN,
                0.5,
            )
            .is_none()
        );
        let (degenerate_brep, section, swept) =
            mapped_xt_swept_offset_fixture(Sense::Positive, [1.0, 0.0, 0.0]);
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &degenerate_brep,
                &degenerate_brep.surfaces[1],
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        let (mut unknown_brep, section, swept) =
            mapped_xt_swept_offset_fixture(Sense::Unknown, [0.0, 0.0, 1.0]);
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &unknown_brep,
                &unknown_brep.surfaces[1],
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
        unknown_brep.source_format = BrepSourceFormat::Binary;
        assert!(
            evaluate_mapped_xt_swept_offset_surface_candidate(
                &unknown_brep,
                &unknown_brep.surfaces[1],
                &swept,
                &section,
                0.5,
                0.5,
            )
            .is_none()
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed refinement fixture must produce its expected sample budget result."
    )]
    fn sampled_refinement_is_uniform_and_fails_closed_at_budget() {
        let curved = |u: f64, v: f64| {
            Some(SurfaceEvaluation {
                point: [u, v, u * u],
                normal: [0.0, 0.0, 1.0],
            })
        };
        let triangle = [[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]];
        let (refined, depth) =
            uniformly_refine_triangles_by_sampled_error(&triangle, &curved, 0.07, 16, 3)
                .expect("one uniform split meets the sampled tolerance");
        assert_eq!(depth, 1);
        assert_eq!(refined.len(), 4);
        let area = refined
            .iter()
            .map(|triangle| signed_area_2d(triangle).abs())
            .sum::<f64>();
        assert!((area - 0.5).abs() < 1e-12);
        assert!(
            uniformly_refine_triangles_by_sampled_error(&triangle, &curved, 1e-12, 4, 3,).is_none()
        );
        assert!(
            uniformly_refine_triangles_by_sampled_error(&triangle, &curved, f64::NAN, 16, 3,)
                .is_none()
        );
        assert!(
            uniformly_refine_triangles_by_sampled_error(&triangle, &|_, _| None, 0.1, 16, 3,)
                .is_none()
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions verify exact deterministic subdivision coordinates."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed wire fixture must produce its expected recursive midpoint samples."
    )]
    fn sampled_wire_uses_recursive_triangle_midpoints() {
        let start = [1e16, 0.0];
        let end = [1.0, 0.0];
        let loops = vec![vec![start, end, [1.0, 1.0], [1e16, 1.0]]];
        let edges =
            sampled_uv_wire_edges(&loops, 3, |u, v| Some([u, v, 0.0])).expect("finite wire points");

        let mut expected = vec![start, end];
        for _ in 0..3 {
            let mut subdivided = Vec::with_capacity(expected.len() * 2 - 1);
            for pair in expected.windows(2) {
                subdivided.push(pair[0]);
                subdivided.push([
                    (pair[0][0] + pair[1][0]) * 0.5,
                    (pair[0][1] + pair[1][1]) * 0.5,
                ]);
            }
            subdivided.push(end);
            expected = subdivided;
        }

        assert_eq!(edges.len(), loops[0].len() * 8);
        for (index, pair) in edges[..8].iter().enumerate() {
            assert_eq!(pair[0], [expected[index][0], 0.0, 0.0]);
            assert_eq!(pair[1], [expected[index + 1][0], 0.0, 0.0]);
        }
        assert_eq!(expected[8], end);
        assert_ne!(expected[6][0], start[0] + (end[0] - start[0]) * 0.75);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed wire fixture must produce its expected closure handling."
    )]
    fn sampled_wire_drops_duplicate_closure_and_consecutive_vertices() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let CurveKind::Nurbs(section_curve) = &mut brep.curves[0].kind else {
            panic!("swept section is NURBS")
        };
        *section_curve = NurbsCurve {
            degree: 2,
            control_vertex_count: 3,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![
                vec![0.0, 0.0, 0.0],
                vec![0.5, 0.0, 2.0],
                vec![1.0, 0.0, 0.0],
            ],
            knots: vec![0.0, 1.0],
            knot_multiplicities: vec![3, 3],
            sources: vec![],
        };
        brep.curves[0].source.type_name = "B_CURVE".to_owned();
        brep.surfaces[0].source.node_index = 9;
        brep.surfaces[0].source.node_type = 67;
        brep.surfaces[0].source.type_name = "SWEPT_SURF".to_owned();
        let section = brep.curves[0].clone();
        let swept = crate::brep::SweptSurfaceData {
            source: brep.surfaces[0].source.clone(),
            section: Some(section.source.node_index),
            sweep: [Some(0.0), Some(1.0), Some(0.0)],
            scale: None,
        };
        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: section.id,
            direction: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        let uv_loop = vec![
            [0.0, 0.0],
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.0, 0.0],
        ];
        let mesh = crate::brep::tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &brep,
            &swept,
            &section,
            7,
            &[uv_loop],
            0.01,
        )
        .expect("curved closed trim refines successfully");

        assert!(
            mesh.wire_edges.len() > 4,
            "the curved sweep forced refinement"
        );
        let (mut expected_triangles, mut edge_segments) = (2, 1);
        while expected_triangles < mesh.triangles.len() {
            expected_triangles *= 4;
            edge_segments *= 2;
        }
        assert_eq!(mesh.triangles.len(), expected_triangles);
        assert_eq!(mesh.wire_edges.len(), 4 * edge_segments);
        assert!(
            mesh.wire_edges
                .iter()
                .all(|[start, end]| vec3_norm(vec3_sub(*start, *end)) > 0.0)
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed tiny-edge fixture must produce its expected sampled wire."
    )]
    fn sampled_wire_preserves_tiny_edges_and_rejects_collapsed_loops() {
        let tiny = 1e-20;
        let loop_uv = vec![[0.0, 0.0], [tiny, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let edges = sampled_uv_wire_edges(&[loop_uv], 0, |u, v| Some([u, v, 0.0]))
            .expect("distinct tiny edge remains valid");
        assert_eq!(edges.len(), 4);
        assert!(vec3_norm(vec3_sub(edges[0][0], edges[0][1])) > 0.0);
        assert!(
            sampled_uv_wire_edges(&[vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]], 0, |u, v| Some(
                [u, v, 0.0]
            ),)
            .is_none()
        );
    }

    #[test]
    fn sampled_refinement_rejects_collapsed_hole_without_changing_legacy_path() {
        let surface = SurfaceKind::Plane {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]];
        let collapsed_hole = vec![[1.0, 1.0], [1.0, 1.0], [2.0, 1.0], [1.0, 1.0]];
        let uv_loops = vec![outer, collapsed_hole];
        let loops_3d = uv_loops
            .iter()
            .map(|ring| ring.iter().map(|[u, v]| [*u, *v, 0.0]).collect())
            .collect::<Vec<Vec<[f64; 3]>>>();
        let eval_surface = |u, v| {
            Some(SurfaceEvaluation {
                point: [u, v, 0.0],
                normal: [0.0, 0.0, 1.0],
            })
        };
        let mut sampled_result = TessellatedMesh::default();
        let mut sampled_depth = None;
        assert_eq!(
            try_tessellate_trimmed_surface_with_uv_and_sampled_error(
                &surface,
                |_, _, _| None,
                eval_surface,
                &loops_3d,
                Some(&uv_loops),
                [0.1, 0.1],
                true,
                true,
                &mut sampled_result,
                Some(0.01),
                500_000,
                7,
                &mut sampled_depth,
            ),
            TrimmedSurfaceResult::Rejected
        );

        let mut legacy_result = TessellatedMesh::default();
        assert_eq!(
            try_tessellate_trimmed_surface_with_uv(
                &surface,
                |_, _, _| None,
                eval_surface,
                &loops_3d,
                Some(&uv_loops),
                [0.1, 0.1],
                true,
                true,
                &mut legacy_result,
            ),
            TrimmedSurfaceResult::Tessellated
        );
        let legacy_area = legacy_result
            .triangles
            .iter()
            .map(|triangle| {
                let ab = vec3_sub(triangle[1], triangle[0]);
                let ac = vec3_sub(triangle[2], triangle[0]);
                vec3_norm(vec3_cross(ab, ac)) * 0.5
            })
            .sum::<f64>();
        assert!((legacy_area - 8.0).abs() < 1e-9);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed swept-face fixture must tessellate for its explicit-UV assertions."
    )]
    fn opt_in_swept_face_uses_explicit_uv_trim_and_extended_section() {
        let brep = ruled_sweep_face_with_explicit_uv_trim();
        let mesh = tessellate_swept_brep_face_with_explicit_uv(&brep, 7, [0.0, 1.0, 0.0], 0.01)
            .expect("valid explicit-UV swept face");
        assert!(!mesh.triangles.is_empty());
        assert_eq!(mesh.triangles.len(), mesh.normals.len());
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .all(|point| point.iter().all(|coordinate| coordinate.is_finite()))
        );
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .any(|point| point[0] < 0.0 || point[0] > 1.0)
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The valid swept-face fixture must tessellate before invalid pcurve cases are checked."
    )]
    fn swept_face_explicit_uv_route_uses_mapped_direction_and_requires_pcurve() {
        let brep = ruled_sweep_face_with_explicit_uv_trim();
        let mesh = tessellate_brep_face_with_explicit_uv(&brep, 7, 0.01)
            .expect("mapped swept support and explicit p-curve trim");
        assert!(!mesh.triangles.is_empty());
        assert_eq!(mesh.triangles.len(), mesh.normals.len());
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .chain(mesh.normals.iter().flatten())
                .flatten()
                .all(|value| value.is_finite())
        );

        let mut missing_pcurve = brep;
        missing_pcurve.curves.retain(|curve| curve.id != 13);
        assert!(tessellate_brep_face_with_explicit_uv(&missing_pcurve, 7, 0.01).is_none());

        let mut nonunit_sweep = ruled_sweep_face_with_explicit_uv_trim();
        if let SurfaceKind::Swept { direction, .. } = &mut nonunit_sweep.surfaces[0].kind {
            *direction = Vector3::from_array([0.0, 0.0, 2.0]);
        } else {
            unreachable!();
        }
        assert!(tessellate_brep_face_with_explicit_uv(&nonunit_sweep, 7, 0.01).is_none());
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed line and circle sweep fixtures must tessellate for their output assertions."
    )]
    fn swept_face_explicit_uv_tessellation_supports_line_and_circle_sections() {
        // 1. Line section
        let mut brep_line = ruled_sweep_face_with_explicit_uv_trim();
        brep_line.curves[0].kind = CurveKind::Line {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            direction: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let mesh_line = tessellate_brep_face_with_explicit_uv(&brep_line, 7, 0.01)
            .expect("line swept face tessellation");
        assert!(!mesh_line.triangles.is_empty());
        assert_eq!(mesh_line.triangles.len(), mesh_line.normals.len());
        assert!(
            mesh_line
                .triangles
                .iter()
                .flatten()
                .chain(mesh_line.normals.iter().flatten())
                .flatten()
                .all(|value| value.is_finite())
        );
        // In the XZ plane, all vertices have y == 0.0
        for tri in &mesh_line.triangles {
            for pt in tri {
                assert!(pt[1].abs() < 1e-10);
            }
        }
        for n in &mesh_line.normals {
            for v in n {
                assert!((v[1].abs() - 1.0).abs() < 1e-10);
            }
        }

        // 2. Circle section
        let mut brep_circle = ruled_sweep_face_with_explicit_uv_trim();
        brep_circle.curves[0].kind = CurveKind::Circle {
            center: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 10.0,
        };
        // UV trim: [0, pi/2] x [0, 5]
        brep_circle.curves[1].kind = CurveKind::Nurbs(NurbsCurve {
            degree: 1,
            control_vertex_count: 5,
            vertex_dimension: 2,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![
                vec![0.0, 0.0],
                vec![std::f64::consts::FRAC_PI_2, 0.0],
                vec![std::f64::consts::FRAC_PI_2, 5.0],
                vec![0.0, 5.0],
                vec![0.0, 0.0],
            ],
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            knot_multiplicities: vec![2, 1, 1, 1, 2],
            sources: vec![],
        });
        let mesh_circle = tessellate_brep_face_with_explicit_uv(&brep_circle, 7, 0.01)
            .expect("circle swept face tessellation");
        assert!(!mesh_circle.triangles.is_empty());
        assert_eq!(mesh_circle.triangles.len(), mesh_circle.normals.len());
        assert!(
            mesh_circle
                .triangles
                .iter()
                .flatten()
                .chain(mesh_circle.normals.iter().flatten())
                .flatten()
                .all(|value| value.is_finite())
        );
        // Points should lie on cylinder radius 10.0 in XY plane and z in [0, 5]
        for tri in &mesh_circle.triangles {
            for pt in tri {
                let r = pt[0].hypot(pt[1]);
                assert!((r - 10.0).abs() < 0.1, "r = {r} expected ~10.0");
                assert!(pt[2] >= -1e-6 && pt[2] <= 5.0 + 1e-6);
            }
        }
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions verify exact mapped geometry generated from the fixture coordinates."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed mapped-sweep fixtures must tessellate for their node-binding assertions."
    )]
    fn mapped_xt_swept_face_binds_exact_nodes_and_uses_supplied_uv_loop() {
        use crate::brep::{
            SweptSurfaceData, tessellate_mapped_xt_swept_face_with_explicit_uv,
            tessellate_mapped_xt_swept_face_with_sampled_chord_error,
        };

        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        brep.curves[0].source.type_name = "B_CURVE".to_owned();
        let section = brep.curves[0].clone();
        brep.surfaces[0].source.node_index = 9;
        brep.surfaces[0].source.node_type = 67;
        brep.surfaces[0].source.type_name = "SWEPT_SURF".to_owned();
        brep.surfaces[0].kind = SurfaceKind::Swept {
            section: section.id,
            direction: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        let swept = SweptSurfaceData {
            source: brep.surfaces[0].source.clone(),
            section: Some(section.source.node_index),
            sweep: [Some(0.0), Some(1.0), Some(0.0)],
            scale: Some(9876.0),
        };
        let uv_loop = vec![[-0.5, 0.0], [1.5, 0.0], [1.5, 1.0], [-0.5, 1.0]];
        let mesh = tessellate_mapped_xt_swept_face_with_explicit_uv(
            &brep,
            &swept,
            &section,
            7,
            std::slice::from_ref(&uv_loop),
        )
        .expect("validated supplied single loop");
        let points: Vec<_> = mesh.triangles.iter().flatten().copied().collect();
        assert!(points.contains(&[-0.5, 0.0, 0.0]));
        assert!(points.contains(&[1.5, 0.0, 0.0]));
        assert!(points.contains(&[1.5, 1.0, 0.0]));
        assert!(points.contains(&[-0.5, 1.0, 0.0]));

        assert!(
            tessellate_mapped_xt_swept_face_with_sampled_chord_error(
                &brep,
                &swept,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
                f64::NAN,
            )
            .is_none()
        );
        assert!(
            tessellate_mapped_xt_swept_face_with_sampled_chord_error(
                &brep,
                &swept,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
                0.0,
            )
            .is_none()
        );

        let sampled = tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &brep,
            &swept,
            &section,
            7,
            std::slice::from_ref(&uv_loop),
            1e-9,
        )
        .expect("linear swept face meets sampled tolerance without refinement");
        assert_eq!(sampled.triangles.len(), 2);
        assert_eq!(sampled.wire_edges.len(), 4);
        let reverse_loop = uv_loop.iter().rev().copied().collect::<Vec<_>>();
        let reversed_sampled = tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &brep,
            &swept,
            &section,
            7,
            &[reverse_loop],
            1e-9,
        )
        .expect("reversed linear swept face meets the same tolerance");
        let mesh_area = |mesh: &TessellatedMesh| {
            mesh.triangles
                .iter()
                .map(|triangle| {
                    let ab = vec3_sub(triangle[1], triangle[0]);
                    let ac = vec3_sub(triangle[2], triangle[0]);
                    vec3_norm(vec3_cross(ab, ac)) * 0.5
                })
                .sum::<f64>()
        };
        assert!((mesh_area(&sampled) - mesh_area(&reversed_sampled)).abs() < 1e-12);

        let small_uv = vec![
            [0.5, 0.0],
            [0.500_001, 0.0],
            [0.500_002, 0.0],
            [0.500_002, 2e-7],
            [0.5, 2e-7],
        ];
        assert!(signed_area_2d(&small_uv).abs() > 0.0);
        assert!(!loops_have_crossing_constraints(&[small_uv.clone()]));
        let small_mesh = tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &brep,
            &swept,
            &section,
            7,
            &[small_uv],
            1e-9,
        )
        .expect("small nonzero UV edges are retained in sampled mode");
        assert_eq!(small_mesh.triangles.len(), 3);
        assert!(
            (mesh_area(&small_mesh) - 4e-13).abs() < 1e-22,
            "small mesh area was {:.17e}",
            mesh_area(&small_mesh)
        );
        assert!(small_mesh.triangles.iter().all(|triangle| {
            let ab = vec3_sub(triangle[1], triangle[0]);
            let ac = vec3_sub(triangle[2], triangle[0]);
            vec3_norm(vec3_cross(ab, ac)) * 0.5 < 1e-12
        }));
        assert_eq!(small_mesh.wire_edges.len(), 5);

        let mut narrow_brep = brep.clone();
        let CurveKind::Nurbs(profile) = &mut narrow_brep.curves[0].kind else {
            panic!("swept section is NURBS")
        };
        profile.control_vertices = vec![vec![0.0, 0.0, 0.0], vec![1e15, 0.0, 0.0]];
        let narrow_section = narrow_brep.curves[0].clone();
        let narrow_notch = vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.5 + 2e-12, 1.0],
            [0.5 + 2e-12, 0.5],
            [0.5 - 2e-12, 0.5],
            [0.5 - 2e-12, 1.0],
            [0.0, 1.0],
        ];
        assert!(narrow_notch[3][0] - narrow_notch[6][0] < 5e-12);
        let CurveKind::Nurbs(_) = &narrow_section.kind else {
            panic!("narrow section is NURBS")
        };
        let narrow_mesh = tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &narrow_brep,
            &swept,
            &narrow_section,
            7,
            &[narrow_notch.clone()],
            1.0,
        )
        .expect("sampled triangulation preserves the source-space-visible narrow notch");
        let expected_narrow_area = signed_area_2d(&narrow_notch).abs() * 1e15;
        assert!((mesh_area(&narrow_mesh) - expected_narrow_area).abs() < 100.0);
        assert!(mesh_area(&narrow_mesh) < 1e15 - 1000.0);
        assert_eq!(narrow_mesh.wire_edges.len(), narrow_notch.len());
        assert!(
            narrow_mesh
                .wire_edges
                .iter()
                .flatten()
                .any(|point| { vec3_norm(vec3_sub(*point, [5e14 - 2000.0, 0.5, 0.0])) < 1e-3 })
        );
        assert!(
            narrow_mesh
                .wire_edges
                .iter()
                .flatten()
                .any(|point| { vec3_norm(vec3_sub(*point, [5e14 + 2000.0, 0.5, 0.0])) < 0.1 })
        );

        let orientation = |face_sense, surface_sense| {
            let mut oriented_brep = brep.clone();
            oriented_brep.faces[0].sense = face_sense;
            oriented_brep.surfaces[0].sense = surface_sense;
            let oriented_mesh = tessellate_mapped_xt_swept_face_with_explicit_uv(
                &oriented_brep,
                &swept,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
            )
            .expect("each known face/support sense pair tessellates");
            let triangle = oriented_mesh.triangles[0];
            let normal = oriented_mesh.normals[0];
            let winding_z = vec3_cross(
                vec3_sub(triangle[1], triangle[0]),
                vec3_sub(triangle[2], triangle[0]),
            )[2]
            .signum();
            let normal_z = (normal[0][2] + normal[1][2] + normal[2][2]).signum();
            assert_eq!(winding_z, normal_z);
            (winding_z, normal_z)
        };
        let forward = orientation(Sense::Positive, Sense::Positive);
        assert_eq!(
            orientation(Sense::Negative, Sense::Positive),
            (-forward.0, -forward.1)
        );
        assert_eq!(
            orientation(Sense::Positive, Sense::Negative),
            (-forward.0, -forward.1)
        );
        assert_eq!(orientation(Sense::Negative, Sense::Negative), forward);

        let mut changed_scale = swept.clone();
        changed_scale.scale = Some(f64::MAX);
        let same_mesh = tessellate_mapped_xt_swept_face_with_explicit_uv(
            &brep,
            &changed_scale,
            &section,
            7,
            std::slice::from_ref(&uv_loop),
        )
        .expect("scale remains unused");
        assert_eq!(mesh.triangles, same_mesh.triangles);

        let shifted_loop = uv_loop
            .iter()
            .map(|[u, v]| [*u + 1000.0, *v])
            .collect::<Vec<_>>();
        let shifted_mesh = tessellate_mapped_xt_swept_face_with_explicit_uv(
            &brep,
            &swept,
            &section,
            7,
            &[shifted_loop],
        )
        .expect("parameter offset does not change UV seed density");
        assert_eq!(mesh.triangles.len(), shifted_mesh.triangles.len());

        let mut wrong_source = swept.clone();
        wrong_source.source.node_index += 1;
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &brep,
                &wrong_source,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
            )
            .is_none()
        );
        let mut non_unit = swept.clone();
        non_unit.sweep[1] = Some(2.0);
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &brep,
                &non_unit,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
            )
            .is_none()
        );
        let mut unsupported_brep = brep.clone();
        unsupported_brep.surfaces[0].kind = SurfaceKind::Unsupported {
            type_name: "SWEPT_SURF".to_owned(),
        };
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &unsupported_brep,
                &non_unit,
                &section,
                7,
                std::slice::from_ref(&uv_loop),
            )
            .is_none()
        );
        let mut holed_brep = brep.clone();
        holed_brep.faces[0].loops.push(4);
        holed_brep.loops.push(Loop {
            id: 4,
            face: 1,
            half_edges: vec![],
            source: holed_brep.loops[0].source.clone(),
        });
        let hole_loop = vec![[0.0, 0.2], [0.2, 0.2], [0.2, 0.4], [0.0, 0.4]];
        let holed_mesh = tessellate_mapped_xt_swept_face_with_explicit_uv(
            &holed_brep,
            &swept,
            &section,
            7,
            &[uv_loop.clone(), hole_loop.clone()],
        )
        .expect("existing constrained triangulator handles a validated hole");
        assert!(!holed_mesh.triangles.is_empty());
        let covered_area: f64 = holed_mesh
            .triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|point| [point[0], point[1]]);
                (((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * 0.5).abs()
            })
            .sum();
        assert!((covered_area - 1.96).abs() < 1e-9);
        assert!(holed_mesh.triangles.iter().all(|triangle| {
            let centroid = [
                triangle.iter().map(|point| point[0]).sum::<f64>() / 3.0,
                triangle.iter().map(|point| point[1]).sum::<f64>() / 3.0,
            ];
            !point_inside_polygon(centroid, &hole_loop)
        }));
        let sampled_hole_mesh = tessellate_mapped_xt_swept_face_with_sampled_chord_error(
            &holed_brep,
            &swept,
            &section,
            7,
            &[uv_loop.clone(), hole_loop.clone()],
            1e-9,
        )
        .expect("sampled refinement preserves the constrained hole");
        let sampled_hole_area = sampled_hole_mesh
            .triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|point| [point[0], point[1]]);
                (((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * 0.5).abs()
            })
            .sum::<f64>();
        assert!((sampled_hole_area - 1.96).abs() < 1e-9);
        let crossed_hole = vec![[0.0, 0.2], [0.2, 0.4], [0.2, 0.2], [0.0, 0.4]];
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &holed_brep,
                &swept,
                &section,
                7,
                &[uv_loop.clone(), crossed_hole],
            )
            .is_none()
        );
        let mut nonfinite_hole = hole_loop.clone();
        nonfinite_hole[1][0] = f64::NAN;
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &holed_brep,
                &swept,
                &section,
                7,
                &[uv_loop.clone(), nonfinite_hole],
            )
            .is_none()
        );
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &brep,
                &swept,
                &section,
                7,
                &[uv_loop.clone(), hole_loop],
            )
            .is_none()
        );
        let mut malformed = uv_loop;
        malformed[0][1] = f64::NAN;
        assert!(
            tessellate_mapped_xt_swept_face_with_explicit_uv(
                &brep,
                &swept,
                &section,
                7,
                &[malformed],
            )
            .is_none()
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The valid swept-face fixture must tessellate before unsupported inputs are checked."
    )]
    fn swept_face_api_rejects_non_swept_support_even_with_valid_uv_trims() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        let CurveKind::Nurbs(uv_trim) = &mut brep
            .curves
            .iter_mut()
            .find(|curve| curve.id == 12)
            .expect("UV parameter curve")
            .kind
        else {
            panic!("UV parameter curve is NURBS")
        };
        uv_trim.control_vertices = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        brep.surfaces[0].kind = SurfaceKind::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 2,
            v_control_vertex_count: 2,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: false,
            u_closed: false,
            v_closed: false,
            rational: false,
            surface_form: 0,
            control_vertices: vec![
                vec![0.0, 0.0, 0.0],
                vec![0.0, 1.0, 0.0],
                vec![1.0, 0.0, 0.0],
                vec![1.0, 1.0, 0.0],
            ],
            u_knots: vec![0.0, 1.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![2, 2],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        });

        assert!(
            tessellate_brep_face_with_explicit_uv(&brep, 7, 0.01).is_some(),
            "the same face and trims are valid for the general explicit-UV API"
        );
        assert!(
            tessellate_swept_brep_face_with_explicit_uv(&brep, 7, [0.0, 1.0, 0.0], 0.01).is_none()
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions preserve exact boundary behavior for the deliberately scaled fixture."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed ruled-sweep fixture must evaluate for its scale assertions."
    )]
    fn ruled_sweep_normal_is_independent_of_small_units_and_knot_domain_scale() {
        let section = NurbsCurve {
            degree: 1,
            control_vertex_count: 2,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![vec![0.0, 0.0, 0.0], vec![2e-9, 0.0, 0.0]],
            knots: vec![10.0, 20.0],
            knot_multiplicities: vec![2, 2],
            sources: vec![],
        };
        let evaluation =
            evaluate_ruled_sweep_surface(&CurveKind::Nurbs(section), [0.0, 2e-9, 0.0], 15.0, 0.5)
                .expect("small ruled surface retains a unit normal");
        assert_eq!(evaluation.point, [1e-9, 1e-9, 0.0]);
        assert!((evaluation.normal[2] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn swept_face_is_supported_by_generic_tessellation() {
        let brep = ruled_sweep_face_with_explicit_uv_trim();
        assert!(!tessellate_brep_to_mesh(&brep, 0.01).triangles.is_empty());
    }

    #[test]
    fn opt_in_swept_face_rejects_missing_uv_and_malformed_direction() {
        let mut brep = ruled_sweep_face_with_explicit_uv_trim();
        brep.curves.retain(|curve| curve.id != 13);
        assert!(
            tessellate_swept_brep_face_with_explicit_uv(&brep, 7, [0.0, 1.0, 0.0], 0.01).is_none()
        );

        let brep = ruled_sweep_face_with_explicit_uv_trim();
        for direction in [[0.0; 3], [f64::NAN, 1.0, 0.0]] {
            assert!(
                tessellate_swept_brep_face_with_explicit_uv(&brep, 7, direction, 0.01).is_none()
            );
        }
    }

    #[test]
    fn test_trimmed_curve_endpoints_and_nurbs_bounds() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        // 1. Basis curves:
        // Basis 1: NURBS curve along X from 0.0 to 10.0 over parameter domain [0.0, 1.0]
        let nurbs_basis = NurbsCurve {
            degree: 1,
            control_vertex_count: 2,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![vec![0.0, 0.0, 0.0], vec![10.0, 0.0, 0.0]],
            knots: vec![0.0, 1.0],
            knot_multiplicities: vec![2, 2],
            sources: vec![],
        };
        let c_nurbs_basis = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Nurbs(nurbs_basis),
            source: source(),
        };

        // Basis 2: Line along +Y from (5.0, 0.0, 0.0)
        let c_line_basis_1 = CurveGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Line {
                point: Vector3::from_array([5.0, 0.0, 0.0]),
                direction: Vector3::from_array([0.0, 1.0, 0.0]),
            },
            source: source(),
        };

        // Basis 3: Semicircular Arc from (5.0, 5.0, 0.0) to (0.0, 5.0, 0.0)
        let c_circle_basis = CurveGeometry {
            id: 3,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Circle {
                center: Vector3::from_array([2.5, 5.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                radius: 2.5,
            },
            source: source(),
        };

        // Basis 4: Line along -Y from (0.0, 5.0, 0.0)
        let c_line_basis_2 = CurveGeometry {
            id: 4,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Line {
                point: Vector3::from_array([0.0, 5.0, 0.0]),
                direction: Vector3::from_array([0.0, -1.0, 0.0]),
            },
            source: source(),
        };

        // 2. Trimmed curves:
        // Trimmed NURBS: parameter interval [0.0, 0.5] -> X in [0.0, 5.0]
        let c_trimmed_nurbs = CurveGeometry {
            id: 10,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_parameter: 0.0,
                end_parameter: 0.5,
                start_point: Vector3::from_array([0.0, 0.0, 0.0]),
                end_point: Vector3::from_array([5.0, 0.0, 0.0]),
            },
            source: source(),
        };

        // Trimmed Line 1: (5.0, 0.0, 0.0) -> (5.0, 5.0, 0.0)
        let c_trimmed_line_1 = CurveGeometry {
            id: 11,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 2,
                start_parameter: 0.0,
                end_parameter: 5.0,
                start_point: Vector3::from_array([5.0, 0.0, 0.0]),
                end_point: Vector3::from_array([5.0, 5.0, 0.0]),
            },
            source: source(),
        };

        // Trimmed Arc: (5.0, 5.0, 0.0) -> (0.0, 5.0, 0.0)
        let c_trimmed_arc = CurveGeometry {
            id: 12,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 3,
                start_parameter: 0.0,
                end_parameter: std::f64::consts::PI,
                start_point: Vector3::from_array([5.0, 5.0, 0.0]),
                end_point: Vector3::from_array([0.0, 5.0, 0.0]),
            },
            source: source(),
        };

        // Trimmed Line 2: (0.0, 5.0, 0.0) -> (0.0, 0.0, 0.0)
        let c_trimmed_line_2 = CurveGeometry {
            id: 13,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 4,
                start_parameter: 0.0,
                end_parameter: 5.0,
                start_point: Vector3::from_array([0.0, 5.0, 0.0]),
                end_point: Vector3::from_array([0.0, 0.0, 0.0]),
            },
            source: source(),
        };

        // 3. Half-edges with NO vertices (vertex: None):
        let he0 = HalfEdge {
            id: 100,
            loop_id: Some(2),
            forward: Some(101),
            backward: Some(103),
            vertex: None,
            other: None,
            edge: Some(200),
            curve: Some(10),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he1 = HalfEdge {
            id: 101,
            loop_id: Some(2),
            forward: Some(102),
            backward: Some(100),
            vertex: None,
            other: None,
            edge: Some(201),
            curve: Some(11),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he2 = HalfEdge {
            id: 102,
            loop_id: Some(2),
            forward: Some(103),
            backward: Some(101),
            vertex: None,
            other: None,
            edge: Some(202),
            curve: Some(12),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he3 = HalfEdge {
            id: 103,
            loop_id: Some(2),
            forward: Some(100),
            backward: Some(102),
            vertex: None,
            other: None,
            edge: Some(203),
            curve: Some(13),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };

        // 4. Edges with NO start/end vertices:
        let e0 = Edge {
            id: 200,
            owner: source(),
            half_edges: vec![100],
            start_vertex: None,
            end_vertex: None,
            curve: Some(10),
            tolerance: None,
            source: source(),
        };
        let e1 = Edge {
            id: 201,
            owner: source(),
            half_edges: vec![101],
            start_vertex: None,
            end_vertex: None,
            curve: Some(11),
            tolerance: None,
            source: source(),
        };
        let e2 = Edge {
            id: 202,
            owner: source(),
            half_edges: vec![102],
            start_vertex: None,
            end_vertex: None,
            curve: Some(12),
            tolerance: None,
            source: source(),
        };
        let e3 = Edge {
            id: 203,
            owner: source(),
            half_edges: vec![103],
            start_vertex: None,
            end_vertex: None,
            curve: Some(13),
            tolerance: None,
            source: source(),
        };

        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 1,
                back_shell: 0,
                front_shell: 0,
                loops: vec![2],
                surface: Some(500),
                sense: Sense::Positive,
                attributes: vec![],
                source: source(),
            }],
            loops: vec![Loop {
                id: 2,
                face: 1,
                half_edges: vec![100, 101, 102, 103],
                source: source(),
            }],
            half_edges: vec![he0, he1, he2, he3],
            edges: vec![e0, e1, e2, e3],
            vertices: vec![],
            points: vec![],
            curves: vec![
                c_nurbs_basis,
                c_line_basis_1,
                c_circle_basis,
                c_line_basis_2,
                c_trimmed_nurbs,
                c_trimmed_line_1,
                c_trimmed_arc,
                c_trimmed_line_2,
            ],
            surfaces: vec![SurfaceGeometry {
                id: 500,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Plane {
                    point: Vector3::from_array([0.0, 0.0, 0.0]),
                    normal: Vector3::from_array([0.0, 0.0, 1.0]),
                    x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                },
                source: source(),
            }],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 1,
                closed_edge_ring_count: 1,
                euler_characteristic: 1,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        let mesh = tessellate_brep_to_mesh(&brep, 0.1);
        assert!(
            !mesh.triangles.is_empty(),
            "Mesh should contain triangles from planar face tessellation"
        );

        // Verify that all triangles cover ONLY the trimmed [0.0, 0.5] span (X in [0.0, 5.0])
        // and do not extend into the untrimmed [0.5, 1.0] span of the NURBS spline (X in (5.0, 10.0])
        for tri in &mesh.triangles {
            for pt in tri {
                assert!(
                    pt[0] >= -1e-6 && pt[0] <= 5.0 + 1e-6,
                    "X coordinate should stay within trimmed [0.0, 5.0] span: got {}",
                    pt[0]
                );
                assert!(
                    pt[1] >= -1e-6 && pt[1] <= 7.5 + 1e-6,
                    "Y coordinate should stay within [0.0, 7.5]: got {}",
                    pt[1]
                );
                assert!(
                    pt[2].abs() < 1e-6,
                    "Z coordinate should be 0.0 for planar face on XY plane: got {}",
                    pt[2]
                );
            }
        }

        // Verify normals are [0.0, 0.0, 1.0]
        for norm_tri in &mesh.normals {
            for n in norm_tri {
                assert!(
                    (n[0]).abs() < 1e-6 && (n[1]).abs() < 1e-6 && (n[2] - 1.0).abs() < 1e-6,
                    "Normal should be [0.0, 0.0, 1.0]: got {n:?}"
                );
            }
        }
    }

    #[test]
    fn test_compute_newell_normal() {
        // Counter-clockwise unit square in XY plane
        let ccw_square = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
        ];
        let n_ccw = compute_newell_normal(&ccw_square);
        assert!((n_ccw[0]).abs() < 1e-9);
        assert!((n_ccw[1]).abs() < 1e-9);
        assert!((n_ccw[2] - 1.0).abs() < 1e-9);

        // Clockwise unit square in XY plane
        let cw_square = vec![
            [0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
            [10.0, 0.0, 0.0],
        ];
        let n_cw = compute_newell_normal(&cw_square);
        assert!((n_cw[0]).abs() < 1e-9);
        assert!((n_cw[1]).abs() < 1e-9);
        assert!((n_cw[2] + 1.0).abs() < 1e-9);

        // Inclined triangle in 3D
        let inclined_tri = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 2.0], [0.0, 2.0, 2.0]];
        let n_inc = compute_newell_normal(&inclined_tri);
        // Normal should be parallel to [2, 0, 2] x [0, 2, 2] = [-4, -4, 4] -> normalized [-1/sqrt(3), -1/sqrt(3), 1/sqrt(3)]
        let expected = 1.0 / 3.0_f64.sqrt();
        assert!((n_inc[0] - (-expected)).abs() < 1e-6);
        assert!((n_inc[1] - (-expected)).abs() < 1e-6);
        assert!((n_inc[2] - expected).abs() < 1e-6);
    }

    #[test]
    fn test_tessellate_generic_polygon_multi_loop_fallback() {
        let outer = vec![
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 100.0, 0.0],
            [0.0, 100.0, 0.0],
        ];
        let hole = vec![
            [30.0, 30.0, 0.0],
            [70.0, 30.0, 0.0],
            [70.0, 70.0, 0.0],
            [30.0, 70.0, 0.0],
        ];

        let mut mesh = TessellatedMesh::default();
        tessellate_generic_polygon(&[outer, hole], true, &mut mesh);

        assert!(
            !mesh.triangles.is_empty(),
            "Should generate triangles for polygon with hole"
        );

        let mut total_area = 0.0;
        for tri in &mesh.triangles {
            // Verify all points are planar on Z=0
            for pt in tri {
                assert!(pt[2].abs() < 1e-6, "Z should be 0: got {}", pt[2]);
            }
            // 2D area
            let [p0, p1, p2] = *tri;
            let a =
                0.5 * ((p1[0] - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (p1[1] - p0[1])).abs();
            total_area += a;

            // Centroid must NOT be inside the hole [30..70, 30..70]
            let c_x = (p0[0] + p1[0] + p2[0]) / 3.0;
            let c_y = (p0[1] + p1[1] + p2[1]) / 3.0;
            let in_hole = c_x > 30.01 && c_x < 69.99 && c_y > 30.01 && c_y < 69.99;
            assert!(
                !in_hole,
                "Triangle centroid ({c_x}, {c_y}) lies inside the hole!"
            );
        }

        // Expected area: 100*100 - 40*40 = 10000 - 1600 = 8400
        assert!(
            (total_area - 8400.0).abs() < 1.0,
            "Total area should be 8400.0, got {total_area}"
        );
    }

    #[test]
    fn test_tessellate_generic_polygon_raw_scale_concentric_annulus() {
        let sample_circle = |radius: f64, reverse: bool| {
            (0..24)
                .map(|index| {
                    let index = if reverse { 24 - index } else { index };
                    let angle = std::f64::consts::TAU * f64::from(index) / 24.0;
                    [radius * angle.cos(), radius * angle.sin(), 0.0]
                })
                .collect::<Vec<_>>()
        };
        let outer = sample_circle(0.0065, false);
        let hole = sample_circle(0.0049, true);
        let mut mesh = TessellatedMesh::default();

        tessellate_generic_polygon(&[outer, hole], true, &mut mesh);

        assert!(!mesh.triangles.is_empty());
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite())
        );
        let area = mesh
            .triangles
            .iter()
            .map(|[a, b, c]| {
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs()
            })
            .sum::<f64>();
        let sampled_annulus_area =
            12.0 * (0.0065_f64.powi(2) - 0.0049_f64.powi(2)) * (std::f64::consts::TAU / 24.0).sin();
        assert!((area - sampled_annulus_area).abs() <= sampled_annulus_area * 1e-9);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed offset-surface fixture must evaluate for its expected point assertions."
    )]
    fn test_evaluate_offset_surface() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![],
            surfaces: vec![
                SurfaceGeometry {
                    id: 10,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Plane {
                        point: Vector3::from_array([0.0, 0.0, 0.0]),
                        normal: Vector3::from_array([0.0, 0.0, 1.0]),
                        x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                    },
                    source: source(),
                },
                SurfaceGeometry {
                    id: 20,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Cylinder {
                        point: Vector3::from_array([0.0, 0.0, 0.0]),
                        axis: Vector3::from_array([0.0, 0.0, 1.0]),
                        radius: 10.0,
                        x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                    },
                    source: source(),
                },
            ],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 0,
                closed_edge_ring_count: 0,
                euler_characteristic: 0,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        // Offset Plane
        let offset_plane = SurfaceKind::Offset {
            basis_surface: 10,
            offset: 7.5,
        };
        let eval_plane =
            evaluate_surface(&brep, &offset_plane, 3.0, 4.0).expect("plane eval failed");
        assert!((eval_plane.point[0] - 3.0).abs() < 1e-6);
        assert!((eval_plane.point[1] - 4.0).abs() < 1e-6);
        assert!((eval_plane.point[2] - 7.5).abs() < 1e-6);
        assert!((eval_plane.normal[2] - 1.0).abs() < 1e-6);

        // Offset Cylinder
        let offset_cyl = SurfaceKind::Offset {
            basis_surface: 20,
            offset: 2.0,
        };
        // At u = 0.0, v = 5.0, basis point is [10.0, 0.0, 5.0], normal is [1.0, 0.0, 0.0]
        // Offset point should be [12.0, 0.0, 5.0]
        let eval_cyl =
            evaluate_surface(&brep, &offset_cyl, 0.0, 5.0).expect("cylinder eval failed");
        assert!((eval_cyl.point[0] - 12.0).abs() < 1e-6);
        assert!((eval_cyl.point[1] - 0.0).abs() < 1e-6);
        assert!((eval_cyl.point[2] - 5.0).abs() < 1e-6);
        assert!((eval_cyl.normal[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed spun-surface fixtures must evaluate for their expected point assertions."
    )]
    fn test_evaluate_spun_surface() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        // Profile curve: line along X from [5.0, 0.0, 2.0] in direction [1.0, 0.0, 0.0]
        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![CurveGeometry {
                id: 1,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array([5.0, 0.0, 2.0]),
                    direction: Vector3::from_array([1.0, 0.0, 0.0]),
                },
                source: source(),
            }],
            surfaces: vec![],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 0,
                closed_edge_ring_count: 0,
                euler_characteristic: 0,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        let spun = SurfaceKind::Spun {
            profile: 1,
            base: Vector3::from_array([0.0, 0.0, 0.0]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            start: None,
            end: None,
            start_parameter: Some(0.0),
            end_parameter: Some(10.0),
            x_axis: Some(Vector3::from_array([1.0, 0.0, 0.0])),
        };

        // u = 0.0 (r = 5.0, z = 2.0), v = 0.0 -> [5.0, 0.0, 2.0]
        let eval0 = evaluate_surface(&brep, &spun, 0.0, 0.0).expect("spun eval at 0,0 failed");
        assert!((eval0.point[0] - 5.0).abs() < 1e-5);
        assert!((eval0.point[1] - 0.0).abs() < 1e-5);
        assert!((eval0.point[2] - 2.0).abs() < 1e-5);

        // u = 0.0, v = PI / 2 -> [0.0, 5.0, 2.0]
        let eval_90 = evaluate_surface(&brep, &spun, 0.0, std::f64::consts::FRAC_PI_2)
            .expect("spun eval at 0, pi/2 failed");
        assert!((eval_90.point[0] - 0.0).abs() < 1e-5);
        assert!((eval_90.point[1] - 5.0).abs() < 1e-5);
        assert!((eval_90.point[2] - 2.0).abs() < 1e-5);

        // u = 5.0 (r = 10.0, z = 2.0), v = PI -> [-10.0, 0.0, 2.0]
        let eval_180 = evaluate_surface(&brep, &spun, 5.0, std::f64::consts::PI)
            .expect("spun eval at 5, pi failed");
        assert!((eval_180.point[0] - (-10.0)).abs() < 1e-5);
        assert!((eval_180.point[1] - 0.0).abs() < 1e-5);
        assert!((eval_180.point[2] - 2.0).abs() < 1e-5);

        // Normal check: profile is in XY (tangent is in XY plane), rotating around Z
        // So the surface is a horizontal annular disc at Z=2.0
        // Normal must point along Z (+Z or -Z)
        assert!((eval0.normal[0]).abs() < 1e-4);
        assert!((eval0.normal[1]).abs() < 1e-4);
        assert!((eval0.normal[2].abs() - 1.0).abs() < 1e-4);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed spun-surface fixture must tessellate for its mesh assertions."
    )]
    fn test_tessellate_brep_face_spun_surface() {
        let source = |node_index: u32| SourceNodeRef {
            node_index,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: String::new(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 3,
                back_shell: 0,
                front_shell: 0,
                surface: Some(2),
                sense: Sense::Positive,
                loops: vec![],
                attributes: vec![],
                source: source(30),
            }],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![CurveGeometry {
                id: 1,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Line {
                    point: Vector3::from_array([5.0, 0.0, 0.0]),
                    direction: Vector3::from_array([0.0, 0.0, 1.0]),
                },
                source: source(10),
            }],
            surfaces: vec![SurfaceGeometry {
                id: 2,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Spun {
                    profile: 1,
                    base: Vector3::from_array([0.0, 0.0, 0.0]),
                    axis: Vector3::from_array([0.0, 0.0, 1.0]),
                    start: None,
                    end: None,
                    start_parameter: Some(0.0),
                    end_parameter: Some(10.0),
                    x_axis: Some(Vector3::from_array([1.0, 0.0, 0.0])),
                },
                source: source(20),
            }],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 0,
                closed_edge_ring_count: 0,
                euler_characteristic: 0,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        let mesh =
            tessellate_brep_face(&brep, 3, 0.5).expect("spun face should tessellate into a mesh");
        assert!(!mesh.triangles.is_empty());
        assert_eq!(mesh.triangles.len(), mesh.normals.len());
        assert!(
            mesh.triangles
                .iter()
                .flatten()
                .chain(mesh.normals.iter().flatten())
                .flatten()
                .all(|value| value.is_finite())
        );

        for tri in &mesh.triangles {
            for pt in tri {
                let r = pt[0].hypot(pt[1]);
                assert!((r - 5.0).abs() < 1e-4, "r = {r}, expected 5.0");
                assert!(pt[2] >= -1e-4 && pt[2] <= 10.0 + 1e-4);
            }
        }

        assert!(tessellate_brep_face(&brep, 999, 0.5).is_none());
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "The fixed conical-sector fixture must tessellate for its apex assertions."
    )]
    fn test_tessellate_conical_sector_with_apex_vertex() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        // Cone parameters:
        // Base at Z=0.0, radius R=10.0, Apex at Z=20.0
        // Half-angle: tan(half_angle) = -10.0 / 20.0 = -0.5
        // cos_half_angle = 2 / sqrt(5), sin_half_angle = -1 / sqrt(5)
        let r_base: f64 = 10.0;
        let h_apex: f64 = 20.0;
        let hypot: f64 = (r_base * r_base + h_apex * h_apex).sqrt();
        let cos_ha = h_apex / hypot;
        let sin_ha = -r_base / hypot;

        let s_cone = SurfaceGeometry {
            id: 10,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Cone {
                point: Vector3::from_array([0.0, 0.0, 0.0]),
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                radius: r_base,
                sin_half_angle: sin_ha,
                cos_half_angle: cos_ha,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source(),
        };

        // Vertices:
        // 1: Apex (0, 0, 20)
        // 2: Base 0 (10, 0, 0) (u = 0)
        // 3: Base 1 (0, 10, 0) (u = pi/2)
        let v1 = Vertex {
            id: 1,
            point: 1,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v2 = Vertex {
            id: 2,
            point: 2,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v3 = Vertex {
            id: 3,
            point: 3,
            tolerance: None,
            owner: source(),
            source: source(),
        };

        let p1 = PointGeometry {
            id: 1,
            owner: None,
            position: Vector3::from_array([0.0, 0.0, h_apex]),
            source: source(),
        };
        let p2 = PointGeometry {
            id: 2,
            owner: None,
            position: Vector3::from_array([r_base, 0.0, 0.0]),
            source: source(),
        };
        let p3 = PointGeometry {
            id: 3,
            owner: None,
            position: Vector3::from_array([0.0, r_base, 0.0]),
            source: source(),
        };

        // Curves:
        // Basis circle for base arc: radius 10 at Z=0
        let c_circle = CurveGeometry {
            id: 100,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Circle {
                center: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                radius: r_base,
            },
            source: source(),
        };

        // Trimmed arc: 0 to pi/2
        let c_arc = CurveGeometry {
            id: 101,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 100,
                start_parameter: 0.0,
                end_parameter: std::f64::consts::FRAC_PI_2,
                start_point: Vector3::from_array([r_base, 0.0, 0.0]),
                end_point: Vector3::from_array([0.0, r_base, 0.0]),
            },
            source: source(),
        };

        // HalfEdges and Edges:
        // he1: Apex (1) -> Base 0 (2) (straight line ray)
        // he2: Base 0 (2) -> Base 1 (3) (circular arc)
        // he3: Base 1 (3) -> Apex (1) (straight line ray)
        // Note: In Parasolid XT schema (Section 5.3.9), halfedge.vertex is the FORWARD vertex of the fin.
        let he1 = HalfEdge {
            id: 1,
            loop_id: Some(1),
            forward: Some(2),
            backward: Some(3),
            vertex: Some(2),
            other: None,
            edge: Some(1),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he2 = HalfEdge {
            id: 2,
            loop_id: Some(1),
            forward: Some(3),
            backward: Some(1),
            vertex: Some(3),
            other: None,
            edge: Some(2),
            curve: Some(101),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he3 = HalfEdge {
            id: 3,
            loop_id: Some(1),
            forward: Some(1),
            backward: Some(2),
            vertex: Some(1),
            other: None,
            edge: Some(3),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };

        let e1 = Edge {
            id: 1,
            owner: source(),
            half_edges: vec![1],
            start_vertex: Some(1),
            end_vertex: Some(2),
            curve: None,
            tolerance: None,
            source: source(),
        };
        let e2 = Edge {
            id: 2,
            owner: source(),
            half_edges: vec![2],
            start_vertex: Some(2),
            end_vertex: Some(3),
            curve: Some(101),
            tolerance: None,
            source: source(),
        };
        let e3 = Edge {
            id: 3,
            owner: source(),
            half_edges: vec![3],
            start_vertex: Some(3),
            end_vertex: Some(1),
            curve: None,
            tolerance: None,
            source: source(),
        };

        let mut brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 1,
                back_shell: 0,
                front_shell: 0,
                loops: vec![1],
                surface: Some(10),
                sense: Sense::Positive,
                attributes: vec![],
                source: source(),
            }],
            loops: vec![Loop {
                id: 1,
                face: 1,
                half_edges: vec![1, 2, 3],
                source: source(),
            }],
            half_edges: vec![he1, he2, he3],
            edges: vec![e1, e2, e3],
            vertices: vec![v1, v2, v3],
            points: vec![p1, p2, p3],
            curves: vec![c_circle, c_arc],
            surfaces: vec![s_cone],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 1,
                closed_edge_ring_count: 1,
                euler_characteristic: 1,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        let mesh = tessellate_brep_to_mesh(&brep, 0.5);
        assert!(
            !mesh.triangles.is_empty(),
            "Cone sector mesh must contain triangles"
        );
        assert_eq!(
            mesh.triangles.len(),
            mesh.normals.len(),
            "Triangles and normals count must match"
        );

        let mut total_area = 0.0;
        for (tri, norm) in mesh.triangles.iter().zip(mesh.normals.iter()) {
            let d10 = vec3_sub(tri[1], tri[0]);
            let d20 = vec3_sub(tri[2], tri[0]);
            let cross = vec3_cross(d10, d20);
            let area = 0.5 * vec3_norm(cross);
            assert!(area > 1e-6, "Triangle must have positive area: got {area}");
            total_area += area;

            for (&pt, &n) in tri.iter().zip(norm.iter()) {
                assert!(pt.iter().all(|x| x.is_finite()));
                assert!(n.iter().all(|x| x.is_finite()));

                // Vertex should lie within z in [0, 20]
                assert!(
                    pt[2] >= -1e-4 && pt[2] <= h_apex + 1e-4,
                    "Z coordinate out of range: {}",
                    pt[2]
                );

                // Expected radius at this height: r(z) = r_base * (1 - z / h_apex)
                let expected_r = r_base * (1.0 - pt[2] / h_apex);
                let actual_r = (pt[0] * pt[0] + pt[1] * pt[1]).sqrt();
                assert!(
                    (actual_r - expected_r).abs() < 0.1,
                    "Vertex radius {actual_r} does not match expected cone radius {expected_r}"
                );

                // Normal check: unit length
                let n_len = vec3_norm(n);
                assert!(
                    (n_len - 1.0).abs() < 1e-4,
                    "Normal must be normalized: {n_len}"
                );

                // Normal Z component should be -sin_half_angle = +1/sqrt(5) ~ 0.4472
                assert!(
                    (n[2] - (-sin_ha)).abs() < 1e-3,
                    "Normal Z should match cone slope: got {}, expected {}",
                    n[2],
                    -sin_ha
                );
            }
        }
        assert!(
            total_area > 1.0,
            "Total area must be positive: got {total_area}"
        );

        // XT uses v=-dot(P-P0,A) and its cone angle has the opposite sign for
        // this same physical cone. Keep the B3 assertions above as a baseline.
        let sin_xt = -sin_ha;
        let axis = [0.0, 0.0, 1.0];
        let xt_surface = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            axis: Vector3::from_array(axis),
            radius: r_base,
            sin_half_angle: sin_xt,
            cos_half_angle: cos_ha,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let point = [0.0, 0.0, 0.0];
        let x = [1.0, 0.0, 0.0];
        let y = vec3_cross(axis, x);
        let (u, v) = (0.7, -4.0);
        let surface_point = evaluate_surface_kind(&xt_surface, u, v).unwrap().point;
        let uv = project_cone_uv(surface_point, &[], 0, point, axis, x, y, true).unwrap();
        assert!((uv[0] - u).abs() < 1e-10);
        assert!((uv[1] - v).abs() < 1e-10);
        assert!(!xt_cone_loops_match_support(
            point,
            axis,
            r_base,
            sin_xt,
            cos_ha,
            &[vec![[2.5, 0.0, 10.0]]],
        ));
        let apex = vec3_scale(axis, 3.0);
        let neighbor = evaluate_surface_kind(&xt_surface, 1.1, -3.0).unwrap().point;
        let other_neighbor = evaluate_surface_kind(&xt_surface, 1.8, -3.0).unwrap().point;
        let apex_uv = project_cone_uv(
            apex,
            &[apex, neighbor, other_neighbor],
            0,
            point,
            axis,
            x,
            y,
            true,
        )
        .unwrap();
        assert!((apex_uv[0] - 1.1).abs() < 1e-10);
        assert!((apex_uv[1] + 3.0).abs() < 1e-10);
        assert_eq!(surface_periodic_axes(&xt_surface), (true, false));

        let tilted_axis = [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0];
        let tilted_x = [2.0 / 5.0_f64.sqrt(), -1.0 / 5.0_f64.sqrt(), 0.0];
        let tilted_y = vec3_cross(tilted_axis, tilted_x);
        let tilted_origin = [3.0, -2.0, 4.0];
        let tilted_surface = SurfaceKind::XtCone {
            point: Vector3::from_array(tilted_origin),
            axis: Vector3::from_array(tilted_axis),
            radius: r_base,
            sin_half_angle: sin_xt,
            cos_half_angle: cos_ha,
            x_axis: Vector3::from_array(tilted_x),
        };
        let tilted_point = evaluate_surface_kind(&tilted_surface, u, v).unwrap().point;
        let tilted_uv = project_cone_uv(
            tilted_point,
            &[],
            0,
            tilted_origin,
            tilted_axis,
            tilted_x,
            tilted_y,
            true,
        )
        .unwrap();
        assert!((tilted_uv[0] - u).abs() < 1e-10);
        assert!((tilted_uv[1] - v).abs() < 1e-10);

        brep.surfaces[0].kind = xt_surface.clone();
        let offset = 2.0;
        brep.surfaces.push(SurfaceGeometry {
            id: 11,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Offset {
                basis_surface: 10,
                offset,
            },
            source: source(),
        });
        brep.faces[0].surface = Some(11);
        assert!(
            tessellate_brep_to_mesh(&brep, 0.5).triangles.is_empty(),
            "XT offset must reject a trim that includes the apex"
        );
        brep.points.iter_mut().find(|p| p.id == 1).unwrap().position =
            Vector3::from_array([5.0, 0.0, 10.0]);
        brep.surfaces.push(SurfaceGeometry {
            id: 12,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Offset {
                basis_surface: 10,
                offset: 6.0,
            },
            source: source(),
        });
        brep.faces[0].surface = Some(12);
        assert!(
            tessellate_brep_to_mesh(&brep, 0.5).triangles.is_empty(),
            "XT offset must reject a positive-q source face whose shifted chart crosses the apex"
        );
        brep.surfaces[0].kind = SurfaceKind::XtCone {
            point: Vector3::from_array(point),
            axis: Vector3::from_array(axis),
            radius: r_base,
            sin_half_angle: sin_xt,
            cos_half_angle: -cos_ha,
            x_axis: Vector3::from_array(x),
        };
        brep.surfaces.push(SurfaceGeometry {
            id: 13,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Offset {
                basis_surface: 10,
                offset,
            },
            source: source(),
        });
        brep.faces[0].surface = Some(13);
        assert!(
            tessellate_brep_to_mesh(&brep, 0.5).triangles.is_empty(),
            "XT offset must reject cos(a) <= 0 where the p82 normal orientation changes"
        );
        brep.surfaces[0].kind = xt_surface.clone();
        brep.faces[0].surface = Some(10);
        let xt_mesh = tessellate_brep_to_mesh(&brep, 0.5);
        assert!(
            !xt_mesh.triangles.is_empty(),
            "XT cone face must tessellate"
        );
        for (triangle, normals) in xt_mesh.triangles.iter().zip(&xt_mesh.normals) {
            let face_normal = vec3_normalize(vec3_cross(
                vec3_sub(triangle[1], triangle[0]),
                vec3_sub(triangle[2], triangle[0]),
            ));
            assert!(
                vec3_dot(face_normal, normals[0]) > 0.0,
                "XT winding disagrees with its natural normal: {}",
                vec3_dot(face_normal, normals[0])
            );
            for (&vertex, &normal) in triangle.iter().zip(normals) {
                let uv = project_cone_uv(vertex, triangle, 0, point, axis, x, y, true).unwrap();
                let expected = evaluate_surface_kind(&xt_surface, uv[0], uv[1]).unwrap();
                assert!(dist_sq(&vertex, &expected.point) < 1e-8);
                assert!(vec3_dot(normal, expected.normal) > 0.999);
            }
        }

        brep.faces[0].surface = Some(11);
        brep.surfaces[0].sense = Sense::Negative;
        let oriented_offset = -offset;
        let shifted_point = vec3_scale(axis, -oriented_offset * sin_xt);
        let shifted_radius = r_base - oriented_offset * cos_ha;
        let shifted_v_at_base = -oriented_offset * sin_xt;
        let shifted_circle_center = vec3_sub(shifted_point, vec3_scale(axis, shifted_v_at_base));
        let shifted_circle_radius = shifted_radius + shifted_v_at_base * (sin_xt / cos_ha);
        let offset_boundary_point = |u: f64, v: f64| {
            let v = v - oriented_offset * sin_xt;
            let radial = vec3_add(vec3_scale(x, u.cos()), vec3_scale(y, u.sin()));
            vec3_add(
                vec3_add(shifted_point, vec3_scale(axis, -v)),
                vec3_scale(radial, shifted_radius + v * (sin_xt / cos_ha)),
            )
        };
        brep.points.iter_mut().for_each(|p| {
            p.position = Vector3::from_array(match p.id {
                1 => offset_boundary_point(0.0, -10.0),
                2 => offset_boundary_point(0.0, 0.0),
                3 => offset_boundary_point(std::f64::consts::FRAC_PI_2, 0.0),
                _ => p.position.to_array(),
            });
        });
        if let CurveKind::Circle { center, radius, .. } = &mut brep.curves[0].kind {
            *center = Vector3::from_array(shifted_circle_center);
            *radius = shifted_circle_radius;
        }
        if let CurveKind::Trimmed {
            start_point,
            end_point,
            ..
        } = &mut brep.curves[1].kind
        {
            *start_point = Vector3::from_array(offset_boundary_point(0.0, 0.0));
            *end_point =
                Vector3::from_array(offset_boundary_point(std::f64::consts::FRAC_PI_2, 0.0));
        }
        let offset_mesh = tessellate_brep_to_mesh(&brep, 0.5);
        assert!(
            !offset_mesh.triangles.is_empty(),
            "offset XT cone must tessellate"
        );
        let shifted_surface = SurfaceKind::XtCone {
            point: Vector3::from_array(shifted_point),
            axis: Vector3::from_array(axis),
            radius: shifted_radius,
            sin_half_angle: sin_xt,
            cos_half_angle: cos_ha,
            x_axis: Vector3::from_array(x),
        };
        for triangle in &offset_mesh.triangles {
            for &vertex in triangle {
                let uv =
                    project_cone_uv(vertex, triangle, 0, shifted_point, axis, x, y, true).unwrap();
                let expected = evaluate_surface_kind(&shifted_surface, uv[0], uv[1]).unwrap();
                assert!(dist_sq(&vertex, &expected.point) < 1e-8);
            }
        }
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "The fixed cone fixture must tessellate to construct the boundary-conflict case."
    )]
    fn xt_cone_tessellation_rejects_boundary_conflicting_with_chart_support() {
        let point = [0.0; 3];
        let axis = [0.0, -1.0, 0.0];
        let x_axis = [1.0, 0.0, 0.0];
        let radius = 0.0065;
        let sin_half_angle = std::f64::consts::FRAC_1_SQRT_2;
        let cos_half_angle = std::f64::consts::FRAC_1_SQRT_2;
        let surface = SurfaceKind::XtCone {
            point: Vector3::from_array(point),
            axis: Vector3::from_array(axis),
            radius,
            sin_half_angle,
            cos_half_angle,
            x_axis: Vector3::from_array(x_axis),
        };
        let conflict = vec![vec![[0.0045, 0.002, 0.0]]];
        let mut mesh = TessellatedMesh::default();
        tessellate_conical_face(
            point,
            axis,
            radius,
            sin_half_angle,
            cos_half_angle,
            true,
            &surface,
            &conflict,
            0.001,
            &mut mesh,
        );
        assert!(mesh.triangles.is_empty());

        let compatible_uv = [
            [0.0, 0.002],
            [std::f64::consts::FRAC_PI_2, 0.002],
            [std::f64::consts::FRAC_PI_2, 0.003],
            [0.0, 0.003],
        ];
        let compatible = vec![
            compatible_uv
                .into_iter()
                .map(|uv| evaluate_surface_kind(&surface, uv[0], uv[1]).unwrap().point)
                .collect(),
        ];
        tessellate_conical_face(
            point,
            axis,
            radius,
            sin_half_angle,
            cos_half_angle,
            true,
            &surface,
            &compatible,
            0.001,
            &mut mesh,
        );
        assert!(!mesh.triangles.is_empty());
    }

    #[test]
    fn test_tessellate_spherical_triangle_with_pole_vertex() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };

        let radius = 10.0;
        let s_sphere = SurfaceGeometry {
            id: 20,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Sphere {
                center: Vector3::from_array([0.0, 0.0, 0.0]),
                radius,
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source(),
        };

        // Vertices:
        // 1: North Pole (0, 0, 10)
        // 2: Equator 0 (10, 0, 0)
        // 3: Equator 1 (0, 10, 0)
        let v1 = Vertex {
            id: 1,
            point: 1,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v2 = Vertex {
            id: 2,
            point: 2,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v3 = Vertex {
            id: 3,
            point: 3,
            tolerance: None,
            owner: source(),
            source: source(),
        };

        let p1 = PointGeometry {
            id: 1,
            owner: None,
            position: Vector3::from_array([0.0, 0.0, radius]),
            source: source(),
        };
        let p2 = PointGeometry {
            id: 2,
            owner: None,
            position: Vector3::from_array([radius, 0.0, 0.0]),
            source: source(),
        };
        let p3 = PointGeometry {
            id: 3,
            owner: None,
            position: Vector3::from_array([0.0, radius, 0.0]),
            source: source(),
        };

        // Curves:
        // Basis circle on equator: radius 10 at Z=0
        let c_circle = CurveGeometry {
            id: 200,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Circle {
                center: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                radius,
            },
            source: source(),
        };

        // Trimmed arc on equator from 0 to pi/2
        let c_arc = CurveGeometry {
            id: 201,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 200,
                start_parameter: 0.0,
                end_parameter: std::f64::consts::FRAC_PI_2,
                start_point: Vector3::from_array([radius, 0.0, 0.0]),
                end_point: Vector3::from_array([0.0, radius, 0.0]),
            },
            source: source(),
        };

        // HalfEdges and Edges:
        // he1: North Pole (1) -> Equator 0 (2)
        // he2: Equator 0 (2) -> Equator 1 (3) (circular arc on equator)
        // he3: Equator 1 (3) -> North Pole (1)
        // Note: In Parasolid XT schema (Section 5.3.9), halfedge.vertex is the FORWARD vertex of the fin.
        let he1 = HalfEdge {
            id: 1,
            loop_id: Some(1),
            forward: Some(2),
            backward: Some(3),
            vertex: Some(2),
            other: None,
            edge: Some(1),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he2 = HalfEdge {
            id: 2,
            loop_id: Some(1),
            forward: Some(3),
            backward: Some(1),
            vertex: Some(3),
            other: None,
            edge: Some(2),
            curve: Some(201),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he3 = HalfEdge {
            id: 3,
            loop_id: Some(1),
            forward: Some(1),
            backward: Some(2),
            vertex: Some(1),
            other: None,
            edge: Some(3),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };

        let e1 = Edge {
            id: 1,
            owner: source(),
            half_edges: vec![1],
            start_vertex: Some(1),
            end_vertex: Some(2),
            curve: None,
            tolerance: None,
            source: source(),
        };
        let e2 = Edge {
            id: 2,
            owner: source(),
            half_edges: vec![2],
            start_vertex: Some(2),
            end_vertex: Some(3),
            curve: Some(201),
            tolerance: None,
            source: source(),
        };
        let e3 = Edge {
            id: 3,
            owner: source(),
            half_edges: vec![3],
            start_vertex: Some(3),
            end_vertex: Some(1),
            curve: None,
            tolerance: None,
            source: source(),
        };

        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 1,
                back_shell: 0,
                front_shell: 0,
                loops: vec![1],
                surface: Some(20),
                sense: Sense::Positive,
                attributes: vec![],
                source: source(),
            }],
            loops: vec![Loop {
                id: 1,
                face: 1,
                half_edges: vec![1, 2, 3],
                source: source(),
            }],
            half_edges: vec![he1, he2, he3],
            edges: vec![e1, e2, e3],
            vertices: vec![v1, v2, v3],
            points: vec![p1, p2, p3],
            curves: vec![c_circle, c_arc],
            surfaces: vec![s_sphere],
            topology: crate::brep::TopologyValidation {
                valid: true,
                closed_loop_count: 1,
                closed_edge_ring_count: 1,
                euler_characteristic: 1,
            },
            metrics: crate::brep::BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        let mesh = tessellate_brep_to_mesh(&brep, 0.5);
        assert!(
            !mesh.triangles.is_empty(),
            "Spherical triangle mesh must contain triangles"
        );
        assert_eq!(
            mesh.triangles.len(),
            mesh.normals.len(),
            "Triangles and normals count must match"
        );

        let mut total_area = 0.0;
        for (tri, norm) in mesh.triangles.iter().zip(mesh.normals.iter()) {
            let d10 = vec3_sub(tri[1], tri[0]);
            let d20 = vec3_sub(tri[2], tri[0]);
            let cross = vec3_cross(d10, d20);
            let area = 0.5 * vec3_norm(cross);
            assert!(area > 1e-6, "Triangle must have positive area: got {area}");
            total_area += area;

            for (&pt, &n) in tri.iter().zip(norm.iter()) {
                assert!(pt.iter().all(|x| x.is_finite()));
                assert!(n.iter().all(|x| x.is_finite()));

                // Vertex should lie on sphere surface
                let dist = vec3_norm(pt);
                assert!(
                    (dist - radius).abs() < 0.1,
                    "Vertex dist {dist} does not match sphere radius {radius}"
                );

                // Normal check: unit length and outward from origin
                let n_len = vec3_norm(n);
                assert!(
                    (n_len - 1.0).abs() < 1e-4,
                    "Normal must be normalized: {n_len}"
                );

                let expected_norm = vec3_scale(pt, 1.0 / dist);
                let diff_norm = vec3_norm(vec3_sub(n, expected_norm));
                assert!(
                    diff_norm < 1e-3,
                    "Normal should match sphere outward normal: diff={diff_norm}"
                );
            }
        }
        assert!(
            total_area > 1.0,
            "Total area must be positive: got {total_area}"
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "The fixed periodic-hole fixtures must align for the seam assertions."
    )]
    fn periodic_hole_alignment_checks_both_seams() {
        let outer = vec![[3.0, 3.0], [3.5, 3.0], [3.5, 3.5], [3.0, 3.5]];
        let hole_across_u = vec![[-3.18, 3.1], [-3.08, 3.1], [-3.08, 3.2], [-3.18, 3.2]];
        let aligned = align_periodic_hole(
            &hole_across_u,
            &outer,
            [Some(2.0 * std::f64::consts::PI), None],
        )
        .unwrap();
        assert!(aligned.iter().all(|p| point_inside_polygon(*p, &outer)));

        let hole_across_both = vec![
            [-3.18, -3.18],
            [-3.08, -3.18],
            [-3.08, -3.08],
            [-3.18, -3.08],
        ];
        let aligned = align_periodic_hole(
            &hole_across_both,
            &outer,
            [
                Some(2.0 * std::f64::consts::PI),
                Some(2.0 * std::f64::consts::PI),
            ],
        )
        .unwrap();
        assert!(aligned.iter().all(|p| point_inside_polygon(*p, &outer)));
    }

    #[test]
    fn invalid_trim_constraints_and_nested_holes_are_rejected() {
        let bow_tie = vec![[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]];
        assert!(loops_have_crossing_constraints(&[bow_tie.clone()]));
        let narrow_parallel_edges =
            vec![[0.5, 0.0], [0.500_001, 0.0], [0.500_001, 1e8], [0.5, 1e8]];
        let load_constraints = |ring: &[[f64; 2]]| {
            let points = ring
                .iter()
                .map(|point| Point2::new(point[0], point[1]))
                .collect::<Vec<_>>();
            let edges = (0..ring.len())
                .map(|index| [index, (index + 1) % ring.len()])
                .collect::<Vec<_>>();
            let mut conflict = false;
            let triangulation = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
                points,
                edges,
                |_| conflict = true,
            );
            (triangulation.is_ok(), conflict)
        };
        assert_eq!(load_constraints(&narrow_parallel_edges), (true, false));
        let (bow_tie_loaded, bow_tie_conflict) = load_constraints(&bow_tie);
        assert!(bow_tie_conflict || !bow_tie_loaded);
        let outer = vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]];
        let hole = vec![[1.0, 1.0], [4.0, 1.0], [4.0, 4.0], [1.0, 4.0]];
        let nested_hole = vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]];
        assert!(loops_have_nested_holes(&[outer, hole, nested_hole], 0));
    }

    #[test]
    fn periodic_unwrap_is_bounded_and_band_gate_is_narrow() {
        assert!(unwrap_periodic_to_nearest(1e300, 0.0, 2.0 * std::f64::consts::PI).is_none());
        let loops = vec![
            vec![[0.0, 1.0], [1.0, 1.0], [2.0, 1.0]],
            vec![[0.0, 2.0], [-1.0, 2.0], [-2.0, 2.0]],
        ];
        let wraps = [
            [2.0 * std::f64::consts::PI, 0.0],
            [-2.0 * std::f64::consts::PI, 0.0],
        ];
        assert_eq!(
            is_simple_periodic_band(&loops, &wraps, 2.0 * std::f64::consts::PI),
            Some(0)
        );
        assert_eq!(
            is_simple_periodic_band(
                &loops,
                &[
                    [2.0 * std::f64::consts::PI, 0.0],
                    [2.0 * std::f64::consts::PI, 0.0]
                ],
                2.0 * std::f64::consts::PI,
            ),
            Some(0)
        );
        assert!(
            is_simple_periodic_band(
                &loops,
                &[
                    [4.0 * std::f64::consts::PI, 0.0],
                    [-2.0 * std::f64::consts::PI, 0.0]
                ],
                2.0 * std::f64::consts::PI,
            )
            .is_none()
        );
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "The fixed periodic-cone fixture must produce its boundary polygon."
    )]
    fn variable_v_periodic_cone_band_stays_inside_its_boundary() {
        let period = 2.0 * std::f64::consts::PI;
        let sample_count = 48;
        let mut lower = (0..sample_count)
            .map(|index| [f64::from(index) * period / f64::from(sample_count), 0.0])
            .collect::<Vec<_>>();
        lower.push([period + 4.0 * f64::EPSILON, 0.0]);
        let mut upper = (0..sample_count)
            .map(|index| {
                let u = -(f64::from(index) * period / f64::from(sample_count));
                [u, 0.5 + 0.1 * u.sin()]
            })
            .collect::<Vec<_>>();
        upper.push([-period + 4.0 * f64::EPSILON, 0.5]);
        let wraps = [[period, 0.0], [-period, 0.0]];

        let polygon = cone_periodic_band_polygon(&[lower.clone(), upper.clone()], &wraps, period)
            .expect("opposite U wraps with a variable V boundary form a seam-cut polygon");
        assert!(!loops_have_crossing_constraints(&[polygon.clone()]));
        for index in 0..16 {
            let u = polygon[0][0] + (f64::from(index) + 0.5) * period / 16.0;
            assert!(point_inside_polygon([u, 0.25], &polygon));
            assert!(!point_inside_polygon([u, 0.7], &polygon));
        }

        let crossing_upper = (0..sample_count)
            .map(|index| {
                let u = -(f64::from(index) * period / f64::from(sample_count));
                [u, 0.1 * u.sin()]
            })
            .collect::<Vec<_>>();
        assert!(
            cone_periodic_band_polygon(&[lower.clone(), crossing_upper], &wraps, period,).is_none()
        );

        assert!(
            cone_periodic_band_polygon(
                &[lower, upper],
                &[[period, 0.0], [-2.0 * period, 0.0]],
                period,
            )
            .is_none()
        );
    }

    #[test]
    fn sphere_latitude_is_not_a_periodic_chart_axis() {
        let sphere = SurfaceKind::Sphere {
            center: Vector3::from_array([0.0; 3]),
            radius: 1.0,
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let torus = SurfaceKind::Torus {
            center: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            major_radius: 2.0,
            minor_radius: 1.0,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        assert_eq!(surface_periodic_axes(&sphere), (true, false));
        assert_eq!(surface_periodic_axes(&torus), (true, true));
    }

    #[test]
    fn test_split_polygon_at_periodic_seam() {
        let period = 2.0 * std::f64::consts::PI;

        // Polygon crossing seam at u = 0 (from u = -0.5 to u = 0.5)
        let crossing_quad = vec![[-0.5, 0.0], [0.5, 0.0], [0.5, 1.0], [-0.5, 1.0]];

        let splits = split_polygon_at_periodic_seam(&crossing_quad, period);
        assert_eq!(splits.len(), 2);

        let left = &splits[0];
        let right = &splits[1];

        assert!(left.iter().all(|p| p[0] <= 1e-6));
        assert!(right.iter().all(|p| p[0] >= -1e-6));

        // Polygon strictly inside [1.0, 2.0]
        let inside_quad = vec![[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]];

        let no_splits = split_polygon_at_periodic_seam(&inside_quad, period);
        assert_eq!(no_splits.len(), 1);
        assert_eq!(no_splits[0], inside_quad);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions verify exact boundary samples from the deterministic blend fixture."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed blend-boundary fixtures must evaluate for their exact boundary assertions."
    )]
    fn blend_boundary_surface_evaluation() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let s0 = SurfaceGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Plane {
                point: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source(),
        };
        let s1 = SurfaceGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Plane {
                point: Vector3::from_array([0.0, 0.0, 10.0]),
                normal: Vector3::from_array([0.0, 1.0, 0.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source(),
        };
        let blend = SurfaceGeometry {
            id: 10,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::BlendedEdge {
                blend_type: crate::brep::model::BlendType::RollingBall,
                supporting_surfaces: [1, 2],
                spine_curve: 0,
                ranges: [1.0, 1.0],
                thumb_weights: [1.0, 1.0],
                boundary_surfaces: [None, None],
                start: None,
                end: None,
            },
            source: source(),
        };
        let boundary0 = SurfaceKind::BlendBoundary {
            boundary_index: 0,
            blend_surface: 10,
        };
        let boundary1 = SurfaceKind::BlendBoundary {
            boundary_index: 1,
            blend_surface: 10,
        };
        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![],
            surfaces: vec![s0, s1, blend],
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
        };

        // boundary_index 0 resolves to supporting_surfaces[(1 - 0) % 2] = supporting_surfaces[1] = s1 (Plane at z=10, normal +Y)
        let eval0 = evaluate_surface(&brep, &boundary0, 2.0, 3.0).expect("eval boundary 0");
        assert_eq!(eval0.point, [2.0, 0.0, 7.0]);
        assert_eq!(eval0.normal, [0.0, 1.0, 0.0]);

        // boundary_index 1 resolves to supporting_surfaces[(1 - 1) % 2] = supporting_surfaces[0] = s0 (Plane at z=0, normal +Z)
        let eval1 = evaluate_surface(&brep, &boundary1, 4.0, 5.0).expect("eval boundary 1");
        assert_eq!(eval1.point, [4.0, 5.0, 0.0]);
        assert_eq!(eval1.normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Assertions preserve exact surface samples from the deterministic swept fixture."
    )]
    #[allow(
        clippy::expect_used,
        reason = "The fixed swept parabola and hyperbola fixtures must evaluate for their assertions."
    )]
    fn swept_surface_with_trimmed_parabola_and_hyperbola() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let parabola_basis = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Parabola {
                origin: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                focal_length: 1.0,
            },
            source: source(),
        };
        let trimmed_parabola = CurveGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_point: Vector3::from_array([0.0, 0.0, 0.0]),
                end_point: Vector3::from_array([1.0, 2.0, 0.0]),
                start_parameter: 0.0,
                end_parameter: 2.0,
            },
            source: source(),
        };
        // Re-mapped trimmed parabola with domain [10.0, 20.0]
        let remapped_parabola = CurveGeometry {
            id: 3,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_point: Vector3::from_array([25.0, 10.0, 0.0]),
                end_point: Vector3::from_array([100.0, 20.0, 0.0]),
                start_parameter: 10.0,
                end_parameter: 20.0,
            },
            source: source(),
        };
        let swept_surf = SurfaceKind::Swept {
            section: 2,
            direction: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        let swept_remapped_surf = SurfaceKind::Swept {
            section: 3,
            direction: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![parabola_basis, trimmed_parabola, remapped_parabola],
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
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        };

        // When u is in domain [0.0, 2.0], e.g. u = 1.0:
        // Parabola at t=1.0: P(1) = [1/4, 1, 0] = [0.25, 1.0, 0.0].
        // Swept point = [0.25, 1.0, 3.0].
        let eval = evaluate_surface(&brep, &swept_surf, 1.0, 3.0).expect("eval swept parabola");
        assert_eq!(eval.point, [0.25, 1.0, 3.0]);
        assert!(eval.normal[2].abs() < 1e-12); // perpendicular to sweep direction

        // When u in [0, 1] is outside domain [10, 20], e.g. u = 0.5:
        // Remapped to 10.0 + 0.5 * 10.0 = 15.0:
        // P(15) = [225/4, 15, 0] = [56.25, 15.0, 0.0].
        let eval_remap = evaluate_surface(&brep, &swept_remapped_surf, 0.5, 2.0)
            .expect("eval remapped swept parabola");
        assert_eq!(eval_remap.point, [56.25, 15.0, 2.0]);
    }

    #[test]
    fn swept_face_3d_loop_projection_tessellation() {
        let source = || SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let line_curve = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Line {
                point: Vector3::from_array([0.0, 0.0, 0.0]),
                direction: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: source(),
        };
        let swept_surface = SurfaceGeometry {
            id: 10,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Swept {
                section: 1,
                direction: Vector3::from_array([0.0, 1.0, 0.0]),
            },
            source: source(),
        };
        // Face on XY plane swept in +Y from line along X.
        // Loop vertices in 3D: (0, 0, 0) -> (2, 0, 0) -> (2, 3, 0) -> (0, 3, 0)
        let edge0 = Edge {
            id: 100,
            owner: source(),
            half_edges: vec![10],
            start_vertex: None,
            end_vertex: None,
            curve: Some(1),
            tolerance: None,
            source: source(),
        };
        let edge1 = Edge {
            id: 101,
            owner: source(),
            half_edges: vec![11],
            start_vertex: None,
            end_vertex: None,
            curve: None,
            tolerance: None,
            source: source(),
        };
        let edge2 = Edge {
            id: 102,
            owner: source(),
            half_edges: vec![12],
            start_vertex: None,
            end_vertex: None,
            curve: None,
            tolerance: None,
            source: source(),
        };
        let edge3 = Edge {
            id: 103,
            owner: source(),
            half_edges: vec![13],
            start_vertex: None,
            end_vertex: None,
            curve: None,
            tolerance: None,
            source: source(),
        };
        let v0 = Vertex {
            id: 1,
            point: 1,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v1 = Vertex {
            id: 2,
            point: 2,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v2 = Vertex {
            id: 3,
            point: 3,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let v3 = Vertex {
            id: 4,
            point: 4,
            tolerance: None,
            owner: source(),
            source: source(),
        };
        let p0 = PointGeometry {
            id: 1,
            position: Vector3::from_array([0.0, 0.0, 0.0]),
            owner: None,
            source: source(),
        };
        let p1 = PointGeometry {
            id: 2,
            position: Vector3::from_array([2.0, 0.0, 0.0]),
            owner: None,
            source: source(),
        };
        let p2 = PointGeometry {
            id: 3,
            position: Vector3::from_array([2.0, 3.0, 0.0]),
            owner: None,
            source: source(),
        };
        let p3 = PointGeometry {
            id: 4,
            position: Vector3::from_array([0.0, 3.0, 0.0]),
            owner: None,
            source: source(),
        };

        let he0 = HalfEdge {
            id: 10,
            loop_id: Some(1),
            forward: Some(11),
            backward: Some(13),
            vertex: Some(1),
            other: None,
            edge: Some(100),
            curve: Some(1),
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he1 = HalfEdge {
            id: 11,
            loop_id: Some(1),
            forward: Some(12),
            backward: Some(10),
            vertex: Some(2),
            other: None,
            edge: Some(101),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he2 = HalfEdge {
            id: 12,
            loop_id: Some(1),
            forward: Some(13),
            backward: Some(11),
            vertex: Some(3),
            other: None,
            edge: Some(102),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };
        let he3 = HalfEdge {
            id: 13,
            loop_id: Some(1),
            forward: Some(10),
            backward: Some(12),
            vertex: Some(4),
            other: None,
            edge: Some(103),
            curve: None,
            sense: Sense::Positive,
            dummy: false,
            source: source(),
        };

        let mut brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "13006".into(),
            complete: true,
            bodies: vec![],
            regions: vec![],
            shells: vec![],
            faces: vec![Face {
                id: 1,
                back_shell: 0,
                front_shell: 0,
                loops: vec![1],
                surface: Some(10),
                sense: Sense::Positive,
                attributes: vec![],
                source: source(),
            }],
            loops: vec![Loop {
                id: 1,
                face: 1,
                half_edges: vec![10, 11, 12, 13],
                source: source(),
            }],
            half_edges: vec![he0, he1, he2, he3],
            edges: vec![edge0, edge1, edge2, edge3],
            vertices: vec![v0, v1, v2, v3],
            points: vec![p0, p1, p2, p3],
            curves: vec![line_curve],
            surfaces: vec![swept_surface],
            topology: TopologyValidation {
                valid: true,
                closed_loop_count: 1,
                closed_edge_ring_count: 1,
                euler_characteristic: 1,
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
        };

        let mesh = tessellate_brep_to_mesh(&brep, 0.1);
        assert!(
            !mesh.triangles.is_empty(),
            "projected swept face should tessellate"
        );
        assert_eq!(
            mesh.triangles.len(),
            2,
            "quad polygon should triangulate to 2 triangles"
        );

        let (section, direction) = match &brep.surfaces[0].kind {
            SurfaceKind::Swept { section, direction } => (*section, *direction),
            _ => unreachable!(),
        };
        brep.surfaces[0].kind = SurfaceKind::SweptWithSource {
            section,
            sweep: direction,
            scale: Some(1.0),
        };
        let source_swept_mesh = tessellate_brep_to_mesh(&brep, 0.1);
        assert!(source_swept_mesh.triangles.is_empty());
        assert!(source_swept_mesh.wire_edges.is_empty());
    }
}
