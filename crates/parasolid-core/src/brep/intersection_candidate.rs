//! Bounded, explicit chart-plane intersection candidates.

use std::{error::Error, fmt};

use super::{NurbsCurve, SurfaceKind, evaluate_ruled_sweep_point_candidate};

const SCAN_STEPS: u32 = 1024;
const BISECTION_STEPS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartIntersectionError {
    InvalidInput(&'static str),
    NoRoot,
    Ambiguous { roots: usize },
    Evaluation,
}

impl fmt::Display for ChartIntersectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
            Self::NoRoot => f.write_str("no sampled sign-change root in section-parameter bracket"),
            Self::Ambiguous { roots } => {
                write!(
                    f,
                    "{roots} sampled sign-change roots in section-parameter bracket"
                )
            }
            Self::Evaluation => {
                f.write_str("failed to evaluate chart-plane intersection candidate")
            }
        }
    }
}

impl Error for ChartIntersectionError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartIntersectionCandidate {
    pub uv: [f64; 2],
    pub point: [f64; 3],
    /// Absolute normalized implicit torus residual at `point`.
    pub torus_residual: f64,
}

/// Finds the unique sampled sign-change root in a caller-supplied section-parameter bracket.
///
/// This opt-in helper follows the XT 2022 chart-plane construction: the plane
/// intersects a ruled sweep, and the resulting point is checked against a torus.
/// It uses a fixed 1024-interval scan and bisection; roots without a sign change
/// can be missed, so this is a candidate evaluator, not a complete intersection
/// solver. It does not affect generic B-Rep evaluation or tessellation.
///
/// # Errors
/// Returns an error for invalid inputs, failed point evaluation, no sampled
/// sign-change root, or multiple sampled roots.
pub fn intersect_torus_ruled_sweep_with_chart_plane_candidate(
    section: &NurbsCurve,
    sweep_direction: [f64; 3],
    torus: &SurfaceKind,
    plane_point: [f64; 3],
    plane_normal: [f64; 3],
    u_bracket: [f64; 2],
) -> Result<ChartIntersectionCandidate, ChartIntersectionError> {
    let invalid = |message| Err(ChartIntersectionError::InvalidInput(message));
    if u_bracket.iter().any(|x| !x.is_finite()) || u_bracket[0] >= u_bracket[1] {
        return invalid("invalid section-parameter bracket");
    }
    if plane_point
        .iter()
        .chain(plane_normal.iter())
        .any(|x| !x.is_finite())
        || sweep_direction.iter().any(|x| !x.is_finite())
    {
        return invalid("nonfinite chart-plane or sweep input");
    }
    let SurfaceKind::Torus {
        center,
        axis,
        major_radius,
        minor_radius,
        ..
    } = torus
    else {
        return invalid("chart-plane candidate requires an analytic torus");
    };
    if !major_radius.is_finite()
        || !minor_radius.is_finite()
        || *major_radius <= 0.0
        || *minor_radius <= 0.0
    {
        return invalid("invalid torus radii");
    }
    let axis = axis.to_array();
    let axis_len = norm(axis);
    let normal_len = norm(plane_normal);
    if !axis_len.is_finite() || axis_len == 0.0 || !normal_len.is_finite() || normal_len == 0.0 {
        return invalid("degenerate torus axis or chart-plane normal");
    }
    let normal_sweep = dot(plane_normal, sweep_direction);
    if !normal_sweep.is_finite() || normal_sweep == 0.0 {
        return invalid("chart plane is parallel to ruled sweep");
    }
    if norm(sweep_direction) == 0.0 || !norm(sweep_direction).is_finite() {
        return invalid("zero ruled-sweep direction");
    }
    if !(u_bracket[1] - u_bracket[0]).is_finite() {
        return invalid("section-parameter bracket is too wide");
    }

    let residual_at = |u: f64| -> Result<(f64, f64, [f64; 3]), ChartIntersectionError> {
        let section_point =
            super::evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(section, u)
                .map_err(|_| ChartIntersectionError::Evaluation)?;
        let v = dot(
            plane_normal,
            std::array::from_fn(|i| plane_point[i] - section_point[i]),
        ) / normal_sweep;
        if !v.is_finite() {
            return Err(ChartIntersectionError::Evaluation);
        }
        let point = evaluate_ruled_sweep_point_candidate(section, sweep_direction, u, v)
            .map_err(|_| ChartIntersectionError::Evaluation)?;
        let residual = torus_residual(
            point,
            center.to_array(),
            axis.map(|x| x / axis_len),
            *major_radius,
            *minor_radius,
        )
        .ok_or(ChartIntersectionError::Evaluation)?;
        Ok((residual, v, point))
    };

    let roots = scan_sign_change_roots(u_bracket, |u| {
        residual_at(u).map(|(residual, _, _)| residual)
    })?;
    match roots.as_slice() {
        [] => Err(ChartIntersectionError::NoRoot),
        [u] => {
            let (residual, v, point) = residual_at(*u)?;
            Ok(ChartIntersectionCandidate {
                uv: [*u, v],
                point,
                torus_residual: residual.abs(),
            })
        }
        _ => Err(ChartIntersectionError::Ambiguous { roots: roots.len() }),
    }
}

fn scan_sign_change_roots(
    u_bracket: [f64; 2],
    residual_at: impl Fn(f64) -> Result<f64, ChartIntersectionError>,
) -> Result<Vec<f64>, ChartIntersectionError> {
    let mut roots = Vec::new();
    let mut left = u_bracket[0];
    let mut f_left = residual_at(left)?;
    if f_left == 0.0 {
        push_root(&mut roots, left);
    }
    for i in 1..=SCAN_STEPS {
        let right =
            u_bracket[0] + (u_bracket[1] - u_bracket[0]) * f64::from(i) / f64::from(SCAN_STEPS);
        let f_right = residual_at(right)?;
        if f_right == 0.0 {
            push_root(&mut roots, right);
        } else if f_left != 0.0 && f_left.signum() != f_right.signum() {
            let (mut lo, mut hi, mut flo) = (left, right, f_left);
            for _ in 0..BISECTION_STEPS {
                let mid = lo * 0.5 + hi * 0.5;
                let f_mid = residual_at(mid)?;
                if f_mid == 0.0 {
                    lo = mid;
                    hi = mid;
                    break;
                }
                if flo.signum() == f_mid.signum() {
                    lo = mid;
                    flo = f_mid;
                } else {
                    hi = mid;
                }
            }
            let u = lo * 0.5 + hi * 0.5;
            push_root(&mut roots, u);
        }
        (left, f_left) = (right, f_right);
    }
    Ok(roots)
}

#[allow(
    clippy::float_cmp,
    reason = "Exact root equality suppresses only duplicate roots produced by the same solve."
)]
fn push_root(roots: &mut Vec<f64>, root: f64) {
    if roots.last().is_none_or(|previous: &f64| *previous != root) {
        roots.push(root);
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn norm(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}

fn torus_residual(
    point: [f64; 3],
    center: [f64; 3],
    axis: [f64; 3],
    major: f64,
    minor: f64,
) -> Option<f64> {
    let q = std::array::from_fn(|i| point[i] - center[i]);
    let q2 = dot(q, q);
    let axial = dot(q, axis) / norm(axis);
    let radial2 = q2 - axial * axial;
    let value = ((q2 + major * major - minor * minor).powi(2) - 4.0 * major * major * radial2)
        / (4.0 * major * major * minor * minor);
    value.is_finite().then_some(value)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::brep::Vector3;

    fn section() -> NurbsCurve {
        NurbsCurve {
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
                vec![2.0, 0.0, 0.0],
                vec![4.0, 0.0, 0.0],
            ],
            knots: vec![0.0, 1.0],
            knot_multiplicities: vec![3, 3],
            sources: vec![],
        }
    }
    fn torus() -> SurfaceKind {
        SurfaceKind::Torus {
            center: Vector3::from_array([0.0, 0.0, 0.0]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            major_radius: 2.0,
            minor_radius: 0.5,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        }
    }
    fn solve(bracket: [f64; 2]) -> Result<ChartIntersectionCandidate, ChartIntersectionError> {
        intersect_torus_ruled_sweep_with_chart_plane_candidate(
            &section(),
            [0.0, 0.0, 1.0],
            &torus(),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            bracket,
        )
    }

    #[test]
    fn candidate_is_bounded_and_reports_unique_and_multiple_roots() {
        let root = solve([0.3, 0.45]).unwrap();
        assert!((root.uv[0] - 0.375).abs() < 1e-10);
        assert_eq!(root.uv[1], 0.0);
        assert!(root.torus_residual < 1e-10);
        assert!((solve([0.375, 0.5]).unwrap().uv[0] - 0.375).abs() < 1e-12);
        assert!((solve([0.25, 0.375]).unwrap().uv[0] - 0.375).abs() < 1e-12);
        assert_eq!(
            solve([0.25, 0.75]),
            Err(ChartIntersectionError::Ambiguous { roots: 2 })
        );
        assert_eq!(
            solve([0.2, 0.8]),
            Err(ChartIntersectionError::Ambiguous { roots: 2 })
        );
        assert_eq!(solve([0.0, 0.2]), Err(ChartIntersectionError::NoRoot));
        assert_eq!(
            ChartIntersectionError::NoRoot.to_string(),
            "no sampled sign-change root in section-parameter bracket"
        );
        assert_eq!(
            ChartIntersectionError::Ambiguous { roots: 2 }.to_string(),
            "2 sampled sign-change roots in section-parameter bracket"
        );
    }

    #[test]
    fn candidate_rejects_degenerate_geometry_and_nonfinite_bounds() {
        assert!(matches!(
            solve([1.0, 1.0]),
            Err(ChartIntersectionError::InvalidInput(_))
        ));
        assert!(matches!(
            solve([f64::NAN, 2.0]),
            Err(ChartIntersectionError::InvalidInput(_))
        ));
        assert!(matches!(
            intersect_torus_ruled_sweep_with_chart_plane_candidate(
                &section(),
                [0.0; 3],
                &torus(),
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [1.0, 2.0]
            ),
            Err(ChartIntersectionError::InvalidInput(_))
        ));
        let parallel = intersect_torus_ruled_sweep_with_chart_plane_candidate(
            &section(),
            [0.0, 0.0, 1.0],
            &torus(),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0],
        );
        assert_eq!(
            parallel,
            Err(ChartIntersectionError::InvalidInput(
                "chart plane is parallel to ruled sweep"
            ))
        );
        let mut degenerate_torus = torus();
        if let SurfaceKind::Torus { axis, .. } = &mut degenerate_torus {
            *axis = Vector3::from_array([0.0; 3]);
        }
        assert_eq!(
            intersect_torus_ruled_sweep_with_chart_plane_candidate(
                &section(),
                [0.0, 0.0, 1.0],
                &degenerate_torus,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [0.0, 1.0],
            ),
            Err(ChartIntersectionError::InvalidInput(
                "degenerate torus axis or chart-plane normal"
            ))
        );
    }
}
