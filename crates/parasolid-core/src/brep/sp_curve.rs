//! Bounded point/tangent evaluation for mapped Parasolid `SP_CURVE` records.

use std::io;

use super::{
    CurveEvaluation, CurveKind, evaluate_mapped_nurbs_curve_candidate_2d, evaluate_surface_point,
};
use crate::brep::{BrepId, BrepModel, CurveGeometry};

const MAX_NUMERICAL_STEP: f64 = 1.0e-3;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Evaluates an `SP_CURVE` point and tangent using its mapped support and 2D B-curve.
///
/// The 2D curve derivative and support partial derivatives are estimated with
/// bounded finite differences, then combined by the chain rule. The increasing
/// parameter tangent is oriented by the `SP_CURVE` curve sense; the referenced
/// 2D B-curve contributes its natural parameterization.
///
/// Only direct `SP_CURVE` geometry backed by a supported mapped 2D NURBS
/// parameter curve and a point-evaluable analytic/NURBS support surface is
/// accepted. Periodic parameter curves must satisfy the NURBS evaluator's exact
/// repeated-control and matching-knot-extension checks. Trim wrappers and
/// unsupported support-surface forms return an error.
///
/// # Errors
/// Returns invalid data when the parameter is nonfinite, mapped references are
/// missing or ambiguous, geometry is unsupported, derivatives are unavailable,
/// or the resulting point/tangent is degenerate or nonfinite.
pub fn evaluate_sp_curve_candidate(
    brep: &BrepModel,
    curve_id: BrepId,
    parameter: f64,
) -> io::Result<CurveEvaluation> {
    if !parameter.is_finite() {
        return Err(invalid("nonfinite SP_CURVE parameter"));
    }
    let curve = unique_by_id(
        &brep.curves,
        curve_id,
        "SP_CURVE reference is missing or ambiguous",
    )?;
    let CurveKind::SurfaceParametric {
        surface,
        parameter_curve,
        ..
    } = &curve.kind
    else {
        return Err(invalid("curve is not an SP_CURVE"));
    };
    if *parameter_curve == curve_id {
        return Err(invalid("cyclic SP_CURVE parameter-curve reference"));
    }
    let uv_curve = unique_by_id(
        &brep.curves,
        *parameter_curve,
        "SP_CURVE parameter-curve reference is missing or ambiguous",
    )?;
    let CurveKind::Nurbs(nurbs) = &uv_curve.kind else {
        return Err(invalid("SP_CURVE parameter curve is not a mapped B-curve"));
    };
    // Rational 2D B-curves store (u, v, weight), so dimension 3 is valid.
    if (!nurbs.rational && nurbs.vertex_dimension != 2)
        || (nurbs.rational && nurbs.vertex_dimension != 3)
    {
        return Err(invalid("SP_CURVE parameter curve is not two-dimensional"));
    }
    let support = unique_by_id(
        &brep.surfaces,
        *surface,
        "SP_CURVE support-surface reference is missing or ambiguous",
    )?;

    let uv = evaluate_mapped_nurbs_curve_candidate_2d(nurbs, parameter)?;
    let uv_tangent = derivative_2d(parameter, |t| {
        evaluate_mapped_nurbs_curve_candidate_2d(nurbs, t).ok()
    })
    .ok_or_else(|| invalid("SP_CURVE parameter-curve tangent is unavailable"))?;
    let point = evaluate_surface_point(&support.kind, uv[0], uv[1])
        .ok_or_else(|| invalid("SP_CURVE support point is unsupported"))?;
    let du = derivative_3d(uv[0], |u| evaluate_surface_point(&support.kind, u, uv[1]))
        .ok_or_else(|| invalid("SP_CURVE support U derivative is unavailable"))?;
    let dv = derivative_3d(uv[1], |v| evaluate_surface_point(&support.kind, uv[0], v))
        .ok_or_else(|| invalid("SP_CURVE support V derivative is unavailable"))?;
    let mut tangent = add(scale(du, uv_tangent[0]), scale(dv, uv_tangent[1]));
    let orientation = curve
        .sense
        .multiplier()
        .ok_or_else(|| invalid("SP_CURVE orientation is unknown"))?;
    tangent = scale(tangent, orientation);
    if !point.into_iter().all(f64::is_finite)
        || !tangent.into_iter().all(f64::is_finite)
        || norm(tangent) <= 1.0e-12
    {
        return Err(invalid("SP_CURVE evaluation is degenerate or nonfinite"));
    }
    Ok(CurveEvaluation { point, tangent })
}

fn unique_by_id<'a, T>(items: &'a [T], id: BrepId, error: &'static str) -> io::Result<&'a T>
where
    T: HasId,
{
    let mut matches = items.iter().filter(|item| item.id() == id);
    let found = matches.next().ok_or_else(|| invalid(error))?;
    if matches.next().is_some() {
        return Err(invalid(error));
    }
    Ok(found)
}

trait HasId {
    fn id(&self) -> BrepId;
}

impl HasId for CurveGeometry {
    fn id(&self) -> BrepId {
        self.id
    }
}

impl HasId for crate::brep::SurfaceGeometry {
    fn id(&self) -> BrepId {
        self.id
    }
}

fn derivative_2d<F>(at: f64, mut evaluate: F) -> Option<[f64; 2]>
where
    F: FnMut(f64) -> Option<[f64; 2]>,
{
    derivative(at, &mut evaluate)
}

fn derivative_3d<F>(at: f64, mut evaluate: F) -> Option<[f64; 3]>
where
    F: FnMut(f64) -> Option<[f64; 3]>,
{
    derivative(at, &mut evaluate)
}

fn derivative<const N: usize, F>(at: f64, evaluate: &mut F) -> Option<[f64; N]>
where
    F: FnMut(f64) -> Option<[f64; N]>,
{
    let mut step = (1.0e-5 * at.abs().max(1.0)).min(MAX_NUMERICAL_STEP);
    let center = evaluate(at)?;
    for _ in 0..9 {
        let before = evaluate(at - step);
        let after = evaluate(at + step);
        let result = match (before, after) {
            (Some(before), Some(after)) => Some(std::array::from_fn(|i| {
                (after[i] - before[i]) / (2.0 * step)
            })),
            (Some(before), None) => Some(std::array::from_fn(|i| (center[i] - before[i]) / step)),
            (None, Some(after)) => Some(std::array::from_fn(|i| (after[i] - center[i]) / step)),
            (None, None) => None,
        };
        if let Some(result) = result.filter(|result| result.iter().all(|value| value.is_finite())) {
            return Some(result);
        }
        step *= 0.5;
    }
    None
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn norm(a: [f64; 3]) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}

#[cfg(test)]
mod tests {
    use super::evaluate_sp_curve_candidate;
    use crate::brep::{
        BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, NurbsCurve, Sense,
        SourceNodeRef, SurfaceGeometry, SurfaceKind, TopologyValidation, Vector3,
    };

    fn source() -> SourceNodeRef {
        SourceNodeRef {
            node_index: 0,
            node_type: 1,
            type_name: "test".into(),
            node_id: None,
            byte_range: 0..0,
        }
    }

    fn fixture(curve_sense: Sense, uv_sense: Sense) -> BrepModel {
        let uv_curve = NurbsCurve {
            degree: 1,
            control_vertex_count: 2,
            vertex_dimension: 2,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![vec![0.0, 0.0], vec![1.0, 2.0]],
            knots: vec![0.0, 1.0],
            knot_multiplicities: vec![2, 2],
            sources: vec![],
        };
        BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "test".into(),
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
            curves: vec![
                CurveGeometry {
                    id: 10,
                    sense: curve_sense,
                    owner: None,
                    kind: CurveKind::SurfaceParametric {
                        surface: 20,
                        parameter_curve: 11,
                        original_curve: None,
                        tolerance_to_original: None,
                    },
                    source: source(),
                },
                CurveGeometry {
                    id: 11,
                    sense: uv_sense,
                    owner: None,
                    kind: CurveKind::Nurbs(uv_curve),
                    source: source(),
                },
            ],
            surfaces: vec![SurfaceGeometry {
                id: 20,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Plane {
                    point: Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    normal: Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    x_axis: Vector3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
                source: source(),
            }],
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
    // The fixture is intentionally valid; the separate tests cover rejected inputs.
    #[allow(clippy::unwrap_used)]
    fn evaluates_embedded_point_and_chain_rule_tangent_with_orientation() {
        let result =
            evaluate_sp_curve_candidate(&fixture(Sense::Negative, Sense::Negative), 10, 0.4)
                .unwrap();
        assert!((result.point[0] - 0.4).abs() < 1e-8);
        assert!((result.point[1] - 0.8).abs() < 1e-8);
        assert!(result.point[2].abs() < 1e-8);
        assert!((result.tangent[0] + 1.0).abs() < 1e-5);
        assert!((result.tangent[1] + 2.0).abs() < 1e-5);
        assert!(result.tangent[2].abs() < 1e-8);
    }

    #[test]
    fn rejects_unknown_orientation_and_dangling_references() {
        assert!(
            evaluate_sp_curve_candidate(&fixture(Sense::Unknown, Sense::Positive), 10, 0.4)
                .is_err()
        );
        let mut missing = fixture(Sense::Positive, Sense::Positive);
        missing.surfaces.clear();
        assert!(evaluate_sp_curve_candidate(&missing, 10, 0.4).is_err());
    }

    #[test]
    fn rejects_self_referential_parameter_curve() {
        let mut cyclic = fixture(Sense::Positive, Sense::Positive);
        let id = cyclic.curves[0].id;
        if let CurveKind::SurfaceParametric {
            parameter_curve, ..
        } = &mut cyclic.curves[0].kind
        {
            *parameter_curve = id;
        }
        assert!(evaluate_sp_curve_candidate(&cyclic, 10, 0.4).is_err());
    }
}
