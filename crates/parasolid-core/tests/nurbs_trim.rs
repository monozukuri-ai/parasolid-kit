use parasolid_core::brep::{
    BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, Face, HalfEdge, Loop,
    NurbsCurve, NurbsSurface, Sense, SourceNodeRef, SurfaceGeometry, SurfaceKind,
    TopologyValidation, tessellate_brep_face,
};

use std::num::TryFromIntError;

const SPANS: u16 = 25;

fn source() -> SourceNodeRef {
    SourceNodeRef {
        node_index: 1,
        node_type: 0,
        type_name: String::new(),
        node_id: None,
        byte_range: 0..0,
    }
}

fn degree_one_curve(points: Vec<[f64; 3]>) -> Result<CurveKind, TryFromIntError> {
    let count = u16::try_from(points.len())?;
    let last = points.len() - 1;
    let mut multiplicities = vec![1; points.len()];
    multiplicities[0] = 2;
    multiplicities[last] = 2;
    Ok(CurveKind::Nurbs(NurbsCurve {
        degree: 1,
        control_vertex_count: u32::from(count),
        vertex_dimension: 3,
        knot_type: 0,
        periodic: false,
        closed: false,
        rational: false,
        curve_form: 0,
        control_vertices: points.into_iter().map(Vec::from).collect(),
        knots: (0..count).map(f64::from).collect(),
        knot_multiplicities: multiplicities,
        sources: vec![],
    }))
}

fn curve(id: u32, points: Vec<[f64; 3]>, sense: Sense) -> Result<CurveGeometry, TryFromIntError> {
    Ok(CurveGeometry {
        id,
        sense,
        owner: None,
        kind: degree_one_curve(points)?,
        source: source(),
    })
}

fn helical_nurbs_face() -> Result<BrepModel, TryFromIntError> {
    let last = usize::from(SPANS);
    let mut rail_a = Vec::with_capacity(last + 1);
    let mut rail_b = Vec::with_capacity(last + 1);
    for index in 0..=SPANS {
        let theta = 6.0 * std::f64::consts::PI * f64::from(index) / f64::from(SPANS);
        let z = 1.2 * theta / (2.0 * std::f64::consts::PI);
        rail_a.push([1.2 * theta.cos(), 1.2 * theta.sin(), z]);
        rail_b.push([
            1.2 * (theta + 0.08).cos(),
            1.2 * (theta + 0.08).sin(),
            z + 0.60,
        ]);
    }

    let mut v_multiplicities = vec![1; last + 1];
    v_multiplicities[0] = 2;
    v_multiplicities[last] = 2;
    let mut control_vertices = rail_a.clone();
    control_vertices.extend(rail_b.iter().copied());
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_control_vertex_count: 2,
        v_control_vertex_count: u32::from(SPANS) + 1,
        vertex_dimension: 3,
        u_knot_type: 0,
        v_knot_type: 0,
        u_periodic: false,
        v_periodic: false,
        u_closed: false,
        v_closed: false,
        rational: false,
        surface_form: 0,
        control_vertices: control_vertices.into_iter().map(Vec::from).collect(),
        u_knots: vec![0.0, 1.0],
        v_knots: (0..=SPANS).map(f64::from).collect(),
        u_knot_multiplicities: vec![2, 2],
        v_knot_multiplicities: v_multiplicities,
        sources: vec![],
    };

    let reversed_rail_b = rail_b.iter().rev().copied().collect();
    let end_cap = vec![rail_b[last], rail_a[last]];
    let start_cap = vec![rail_b[0], rail_a[0]];
    let curves = vec![
        curve(1, rail_a, Sense::Positive)?,
        curve(2, end_cap, Sense::Negative)?,
        curve(3, reversed_rail_b, Sense::Positive)?,
        curve(4, start_cap, Sense::Positive)?,
    ];
    Ok(face_with_nurbs_boundary(surface, curves))
}

fn face_with_nurbs_boundary(surface: NurbsSurface, curves: Vec<CurveGeometry>) -> BrepModel {
    let loop_ids = [11, 12, 13, 14];
    BrepModel {
        source_format: BrepSourceFormat::Binary,
        schema_key: String::new(),
        complete: true,
        assemblies: vec![],
        instances: vec![],
        bodies: vec![],
        regions: vec![],
        shells: vec![],
        faces: vec![Face {
            id: 10,
            back_shell: 0,
            front_shell: 0,
            loops: vec![20],
            surface: Some(30),
            sense: Sense::Positive,
            attributes: vec![],
            source: source(),
        }],
        loops: vec![Loop {
            id: 20,
            face: 10,
            half_edges: loop_ids.to_vec(),
            source: source(),
        }],
        half_edges: loop_ids
            .iter()
            .copied()
            .zip(1..)
            .enumerate()
            .map(|(index, (id, curve_id))| HalfEdge {
                id,
                loop_id: Some(20),
                forward: Some(loop_ids[(index + 1) % loop_ids.len()]),
                backward: Some(loop_ids[(index + loop_ids.len() - 1) % loop_ids.len()]),
                vertex: None,
                other: None,
                edge: None,
                curve: Some(curve_id),
                sense: Sense::Positive,
                dummy: false,
                source: source(),
            })
            .collect(),
        edges: vec![],
        vertices: vec![],
        points: vec![],
        curves,
        surfaces: vec![SurfaceGeometry {
            id: 30,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Nurbs(surface),
            source: source(),
        }],
        attributes: vec![],
        topology: TopologyValidation {
            valid: true,
            closed_loop_count: 1,
            closed_edge_ring_count: 0,
            euler_characteristic: 1,
        },
        metrics: BrepMetrics {
            bounding_box: None,
            surface_area: None,
            volume: None,
        },
        diagnostics: vec![],
    }
}

#[test]
fn direct_nurbs_trim_tracks_negative_cap_sense_and_folded_surface_turns()
-> Result<(), Box<dyn std::error::Error>> {
    let brep = helical_nurbs_face()?;
    let mesh =
        tessellate_brep_face(&brep, 10, 0.05).ok_or("helical NURBS face did not tessellate")?;
    assert!(!mesh.triangles.is_empty());
    assert_eq!(mesh.wire_edges.len(), 2 * usize::from(SPANS) + 2);
    assert!(
        mesh.wire_edges
            .iter()
            .flatten()
            .flatten()
            .all(|coordinate| coordinate.is_finite())
    );
    assert!(
        mesh.triangles
            .iter()
            .flatten()
            .flatten()
            .all(|coordinate| coordinate.is_finite())
    );
    let z_bounds = mesh
        .triangles
        .iter()
        .flatten()
        .map(|point| point[2])
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[min, max], z| {
            [min.min(z), max.max(z)]
        });
    assert!(z_bounds[0] < 0.01, "trim lost the lower turn: {z_bounds:?}");
    assert!(z_bounds[1] > 4.19, "trim lost the upper turn: {z_bounds:?}");
    Ok(())
}
