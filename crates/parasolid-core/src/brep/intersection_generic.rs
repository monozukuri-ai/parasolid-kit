//! Local Newton evaluation for otherwise-unhandled XT intersection supports.

use super::{evaluate_brep_surface, BrepModel, IntersectionEvaluationError, SurfaceKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GenericIntersectionEvaluation {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
    pub uv_a: [f64; 2],
    pub uv_b: [f64; 2],
}

type SurfaceUvEndpoints = [[f64; 2]; 2];

/// Solves the local support pair at one chart-normal section plane.
#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
pub(crate) fn evaluate_generic_chart_intersection(
    brep: &BrepModel,
    surface_a: &SurfaceKind,
    surface_b: &SurfaceKind,
    surface_uv_endpoints: [SurfaceUvEndpoints; 2],
    chord_points: [[f64; 3]; 2],
    fraction: f64,
    chordal_error: Option<f64>,
) -> Result<GenericIntersectionEvaluation, IntersectionEvaluationError> {
    let [first_support_uv, second_support_uv] = surface_uv_endpoints;
    let [first_uv_start, first_uv_end] = first_support_uv;
    let [second_uv_start, second_uv_end] = second_support_uv;
    let [chord_start, chord_end] = chord_points;
    let invalid = |message| Err(IntersectionEvaluationError::InvalidInput(message));
    if first_uv_start
        .into_iter()
        .chain(first_uv_end)
        .chain(second_uv_start)
        .chain(second_uv_end)
        .chain(chord_start)
        .chain(chord_end)
        .chain([fraction])
        .any(|value| !value.is_finite())
        || !(0.0..=1.0).contains(&fraction)
        || chordal_error.is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return invalid("nonfinite or invalid generic intersection input");
    }
    let chord = sub(chord_end, chord_start);
    let chord_length = norm(chord);
    if !chord_length.is_finite() || chord_length <= f64::EPSILON {
        return invalid("generic intersection chart chord is degenerate");
    }
    let [first_uv_start, first_uv_end] =
        unwrap_periodic_uv_pair(brep, surface_a, [first_uv_start, first_uv_end]);
    let [second_uv_start, second_uv_end] =
        unwrap_periodic_uv_pair(brep, surface_b, [second_uv_start, second_uv_end]);
    let chord_normal = scale(chord, 1.0 / chord_length);
    let chord_point = add(chord_start, scale(chord, fraction));
    let seed = [
        (first_uv_start[0] + (first_uv_end[0] - first_uv_start[0]) * fraction),
        (first_uv_start[1] + (first_uv_end[1] - first_uv_start[1]) * fraction),
        (second_uv_start[0] + (second_uv_end[0] - second_uv_start[0]) * fraction),
        (second_uv_start[1] + (second_uv_end[1] - second_uv_start[1]) * fraction),
    ];
    let scale_at = chord_start
        .into_iter()
        .chain(chord_end)
        .map(f64::abs)
        .fold(chord_length.max(1.0), f64::max);
    let tolerance = 1.0e-9 * scale_at;
    let bounds: [[f64; 2]; 4] = std::array::from_fn(|axis| {
        let (a, b) = match axis {
            0 => (first_uv_start[0], first_uv_end[0]),
            1 => (first_uv_start[1], first_uv_end[1]),
            2 => (second_uv_start[0], second_uv_end[0]),
            _ => (second_uv_start[1], second_uv_end[1]),
        };
        let span = (a - b).abs();
        let (surface, uv) = if axis < 2 {
            (surface_a, [seed[0], seed[1]])
        } else {
            (surface_b, [seed[2], seed[3]])
        };
        let step = (span * 1.0e-3).max(uv[axis % 2].abs().max(1.0) * 1.0e-6);
        let mut before = uv;
        let mut after = uv;
        before[axis % 2] -= step;
        after[axis % 2] += step;
        let derivative = evaluate_brep_surface(brep, surface, after[0], after[1])
            .zip(evaluate_brep_surface(brep, surface, before[0], before[1]))
            .map(|(after, before)| norm(sub(after.point, before.point)) / (2.0 * step))
            .filter(|value| value.is_finite() && *value > 1.0e-12);
        let error_margin = derivative.map_or(0.0, |value| {
            chordal_error.unwrap_or(tolerance).max(tolerance) / value
        });
        let margin = (span * 0.05)
            .max(error_margin)
            .max(a.abs().max(b.abs()).max(1.0) * 1.0e-6);
        [a.min(b) - margin, a.max(b) + margin]
    });
    let residual = |x: [f64; 4]| -> Option<[f64; 4]> {
        let a = evaluate_brep_surface(brep, surface_a, x[0], x[1])?;
        let b = evaluate_brep_surface(brep, surface_b, x[2], x[3])?;
        let delta = sub(a.point, b.point);
        Some([
            delta[0],
            delta[1],
            delta[2],
            dot(sub(a.point, chord_point), chord_normal),
        ])
    };
    let solve = |mut x: [f64; 4]| -> Option<[f64; 4]> {
        for _ in 0..32 {
            let r = residual(x)?;
            let rnorm = r.into_iter().map(f64::abs).fold(0.0, f64::max);
            if rnorm <= tolerance {
                return Some(x);
            }
            let mut jacobian = [[0.0; 4]; 4];
            for column in 0..4 {
                let h = 1.0e-6 * x[column].abs().max(1.0);
                let mut plus = x;
                let mut minus = x;
                plus[column] = (x[column] + h).min(bounds[column][1]);
                minus[column] = (x[column] - h).max(bounds[column][0]);
                let span = plus[column] - minus[column];
                if span <= 0.0 {
                    return None;
                }
                let rp = residual(plus)?;
                let rm = residual(minus)?;
                for row in 0..4 {
                    jacobian[row][column] = (rp[row] - rm[row]) / span;
                }
            }
            let step = solve_linear_4(jacobian, r.map(|value| -value))?;
            if step.into_iter().any(|value| !value.is_finite()) {
                return None;
            }
            let mut alpha = 1.0;
            let mut accepted = None;
            for _ in 0..16 {
                let candidate = std::array::from_fn(|i| x[i] + alpha * step[i]);
                if (0..4).all(|i| candidate[i] >= bounds[i][0] && candidate[i] <= bounds[i][1])
                    && residual(candidate).is_some_and(|next| {
                        next.into_iter().map(f64::abs).fold(0.0, f64::max) < rnorm
                    })
                {
                    accepted = Some(candidate);
                    break;
                }
                alpha *= 0.5;
            }
            x = accepted?;
        }
        None
    };

    let mut roots: Vec<([f64; 4], [f64; 3])> = Vec::new();
    let perturbations = [
        [1.0, 1.0, 1.0, 1.0],
        [1.0, -1.0, -1.0, 1.0],
        [-1.0, 1.0, -1.0, 1.0],
        [-1.0, -1.0, 1.0, -1.0],
    ];
    // shortcut: roots outside this bounded UV neighborhood are not isolated; widen the search if XT charts stop bracketing their local root.
    let starts = std::iter::once(seed).chain(perturbations.into_iter().map(|signs| {
        std::array::from_fn(|axis| {
            let step = (bounds[axis][1] - bounds[axis][0]) * 0.05;
            (seed[axis] + signs[axis] * step).clamp(bounds[axis][0], bounds[axis][1])
        })
    }));
    for start in starts {
        let Some(root) = solve(start) else { continue };
        let Some(a) = evaluate_brep_surface(brep, surface_a, root[0], root[1]) else {
            continue;
        };
        let Some(b) = evaluate_brep_surface(brep, surface_b, root[2], root[3]) else {
            continue;
        };
        if norm(sub(a.point, b.point)) > tolerance * 4.0
            || dot(sub(a.point, chord_point), chord_normal).abs() > tolerance * 4.0
        {
            continue;
        }
        if !roots
            .iter()
            .any(|(_, point)| norm(sub(*point, a.point)) <= tolerance * 8.0)
        {
            roots.push((root, a.point));
        }
    }
    let mut in_bound = roots.into_iter().filter(|(_, point)| {
        chordal_error.is_none_or(|error| norm(sub(*point, chord_point)) <= error + tolerance)
    });
    let Some((root, point)) = in_bound.next() else {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    };
    if in_bound.next().is_some() {
        return Err(IntersectionEvaluationError::Unsupported(
            "generic chart plane has multiple local support-pair roots",
        ));
    }
    let a = evaluate_brep_surface(brep, surface_a, root[0], root[1]).ok_or(
        IntersectionEvaluationError::Unsupported("generic support A evaluation failed"),
    )?;
    let b = evaluate_brep_surface(brep, surface_b, root[2], root[3]).ok_or(
        IntersectionEvaluationError::Unsupported("generic support B evaluation failed"),
    )?;
    let mut tangent = normalize(cross(a.normal, b.normal)).ok_or(
        IntersectionEvaluationError::Unsupported("generic intersection tangent is singular"),
    )?;
    if dot(tangent, chord) < 0.0 {
        tangent = scale(tangent, -1.0);
    }
    if !point.into_iter().chain(tangent).all(f64::is_finite) {
        return Err(IntersectionEvaluationError::Unsupported(
            "generic intersection result is nonfinite",
        ));
    }
    Ok(GenericIntersectionEvaluation {
        point,
        tangent,
        uv_a: [root[0], root[1]],
        uv_b: [root[2], root[3]],
    })
}

fn unwrap_periodic_uv_pair(
    brep: &BrepModel,
    surface: &SurfaceKind,
    mut endpoints: [[f64; 2]; 2],
) -> [[f64; 2]; 2] {
    let Some(periods) = surface_periods(brep, surface, 0) else {
        return endpoints;
    };
    for (axis, period) in periods.into_iter().enumerate() {
        let Some(period) = period else { continue };
        let delta = endpoints[1][axis] - endpoints[0][axis];
        if !delta.is_finite() || !period.is_finite() || period <= 0.0 {
            continue;
        }
        let wrapped_delta = delta - period * (delta / period).round();
        if wrapped_delta.is_finite() && wrapped_delta.abs() <= period * 0.5 + 1e-9 {
            endpoints[1][axis] = endpoints[0][axis] + wrapped_delta;
        }
    }
    endpoints
}

fn surface_periods(
    brep: &BrepModel,
    surface: &SurfaceKind,
    depth: usize,
) -> Option<[Option<f64>; 2]> {
    if depth > 8 {
        return None;
    }
    let angle = Some(2.0 * std::f64::consts::PI);
    Some(match surface {
        SurfaceKind::Cylinder { .. }
        | SurfaceKind::Cone { .. }
        | SurfaceKind::XtCone { .. }
        | SurfaceKind::Sphere { .. } => [angle, None],
        SurfaceKind::Torus { .. } => [angle, angle],
        SurfaceKind::Spun { .. } => [None, angle],
        SurfaceKind::Offset { basis_surface, .. } => {
            let mut matches = brep
                .surfaces
                .iter()
                .filter(|candidate| candidate.id == *basis_surface);
            let basis = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            return surface_periods(brep, &basis.kind, depth + 1);
        }
        SurfaceKind::Nurbs(nurbs) => {
            let u_period = if nurbs.u_periodic {
                Some(periodic_knot_span(
                    &nurbs.u_knots,
                    &nurbs.u_knot_multiplicities,
                    usize::from(nurbs.u_degree),
                    usize::try_from(nurbs.u_control_vertex_count).ok()?,
                )?)
            } else {
                None
            };
            let v_period = if nurbs.v_periodic {
                Some(periodic_knot_span(
                    &nurbs.v_knots,
                    &nurbs.v_knot_multiplicities,
                    usize::from(nurbs.v_degree),
                    usize::try_from(nurbs.v_control_vertex_count).ok()?,
                )?)
            } else {
                None
            };
            [u_period, v_period]
        }
        _ => [None, None],
    })
}

fn periodic_knot_span(
    knots: &[f64],
    multiplicities: &[u16],
    degree: usize,
    control_count: usize,
) -> Option<f64> {
    if knots.len() != multiplicities.len() || control_count <= degree {
        return None;
    }
    let expanded_count = control_count.checked_add(degree)?.checked_add(1)?;
    let mut expanded = Vec::with_capacity(expanded_count);
    for (&knot, &multiplicity) in knots.iter().zip(multiplicities) {
        if !knot.is_finite()
            || expanded.len().checked_add(usize::from(multiplicity))? > expanded_count
        {
            return None;
        }
        expanded.extend(std::iter::repeat_n(knot, usize::from(multiplicity)));
    }
    if expanded.len() != expanded_count {
        return None;
    }
    let period = expanded[control_count] - expanded[degree];
    (period.is_finite() && period > 0.0).then_some(period)
}

fn solve_linear_4(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    for column in 0..4 {
        let pivot = (column..4)
            .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))?;
        let magnitude = matrix[pivot][column].abs();
        if !magnitude.is_finite() || magnitude <= 1.0e-12 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        for row in column + 1..4 {
            let factor = matrix[row][column] / matrix[column][column];
            for entry in column..4 {
                matrix[row][entry] -= factor * matrix[column][entry];
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut result = [0.0; 4];
    for row in (0..4).rev() {
        result[row] = (rhs[row]
            - (row + 1..4)
                .map(|column| matrix[row][column] * result[column])
                .sum::<f64>())
            / matrix[row][row];
    }
    result.into_iter().all(f64::is_finite).then_some(result)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn scale(a: [f64; 3], value: f64) -> [f64; 3] {
    a.map(|component| component * value)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(a);
    (length.is_finite() && length > 1.0e-12).then(|| scale(a, 1.0 / length))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::brep::{BrepMetrics, BrepSourceFormat, TopologyValidation, Vector3};

    fn empty_brep() -> BrepModel {
        BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "synthetic".into(),
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
        }
    }

    fn vector(x: f64, y: f64, z: f64) -> Vector3 {
        Vector3 { x, y, z }
    }

    #[test]
    fn generic_chart_plane_solver_finds_local_pair_intersection() {
        let brep = empty_brep();
        let cylinder = SurfaceKind::Cylinder {
            point: vector(0.0, 0.0, 0.0),
            axis: vector(0.0, 0.0, 1.0),
            radius: 1.0,
            x_axis: vector(1.0, 0.0, 0.0),
        };
        let plane = SurfaceKind::Plane {
            point: vector(0.0, 0.0, 0.0),
            normal: vector(0.0, 0.0, 1.0),
            x_axis: vector(1.0, 0.0, 0.0),
        };
        let start = [1.0, 0.0, 0.0];
        let end = [0.0, 1.0, 0.0];
        let result = evaluate_generic_chart_intersection(
            &brep,
            &cylinder,
            &plane,
            [
                [[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]],
                [[start[0], start[1]], [end[0], end[1]]],
            ],
            [start, end],
            0.5,
            Some(0.3),
        )
        .expect("local chart-plane root");
        let a = evaluate_brep_surface(&brep, &cylinder, std::f64::consts::FRAC_PI_4, 0.0)
            .unwrap()
            .point;
        let b = evaluate_brep_surface(&brep, &plane, result.point[0], result.point[1])
            .unwrap()
            .point;
        let chord = sub(end, start);
        let chord_point = scale(add(start, end), 0.5);
        assert!(norm(sub(result.point, a)) < 1.0e-8);
        assert!(norm(sub(result.point, b)) < 1.0e-8);
        assert!(norm(sub(a, b)) < 1.0e-8);
        assert!(dot(sub(result.point, chord_point), normalize(chord).unwrap()).abs() < 1.0e-8);
        assert!(dot(result.tangent, chord) > 0.0);
        assert!(evaluate_generic_chart_intersection(
            &brep,
            &cylinder,
            &plane,
            [
                [[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]],
                [[start[0], start[1]], [end[0], end[1]]],
            ],
            [start, end],
            0.5,
            Some(0.2),
        )
        .is_err());
    }

    #[test]
    fn generic_chart_solver_unwraps_cylinder_uv_across_periodic_seam() {
        let brep = empty_brep();
        let cylinder = SurfaceKind::Cylinder {
            point: vector(0.0, 0.0, 0.0),
            axis: vector(0.0, 0.0, 1.0),
            radius: 1.0,
            x_axis: vector(1.0, 0.0, 0.0),
        };
        let plane = SurfaceKind::Plane {
            point: vector(0.0, 0.0, 0.0),
            normal: vector(0.0, 0.0, 1.0),
            x_axis: vector(1.0, 0.0, 0.0),
        };
        let start_angle = 2.0 * std::f64::consts::PI - 0.2;
        let end_angle: f64 = 0.2;
        let start = [start_angle.cos(), start_angle.sin(), 0.0];
        let end = [end_angle.cos(), end_angle.sin(), 0.0];

        let result = evaluate_generic_chart_intersection(
            &brep,
            &cylinder,
            &plane,
            [
                [[start_angle, 0.0], [end_angle, 0.0]],
                [[start[0], start[1]], [end[0], end[1]]],
            ],
            [start, end],
            0.5,
            Some(0.05),
        )
        .expect("local seam-crossing chart root");

        assert!(norm(sub(result.point, [1.0, 0.0, 0.0])) <= 1.0e-8);
    }
}
