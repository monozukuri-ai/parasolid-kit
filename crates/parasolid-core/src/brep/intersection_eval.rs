//! Bounded exact cases of `X_T` `INTERSECTION` curve evaluation.

use std::{error::Error, fmt};

use super::{
    BrepModel, CurveKind, IntersectionLimitKind, IntersectionUvType, NurbsCurve, SurfaceKind,
    SweptSurfaceData,
};

const EPS_FACTOR: f64 = 64.0 * f64::EPSILON;

fn stable_quadratic_roots(a: f64, b: f64, c: f64, discriminant_sqrt: f64) -> Option<[f64; 2]> {
    if ![a, b, c, discriminant_sqrt].into_iter().all(f64::is_finite) || a == 0.0 {
        return None;
    }
    if discriminant_sqrt == 0.0 {
        let root = -b / (2.0 * a);
        return root.is_finite().then_some([root; 2]);
    }
    let q = -0.5 * (b + discriminant_sqrt.copysign(b));
    if !q.is_finite() || q == 0.0 {
        return None;
    }
    let roots = [q / a, c / q];
    roots.into_iter().all(f64::is_finite).then_some(roots)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntersectionEvaluationError {
    Unsupported(&'static str),
    InvalidInput(&'static str),
    OutsideChartDomain,
}

impl fmt::Display for IntersectionEvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) | Self::InvalidInput(message) => f.write_str(message),
            Self::OutsideChartDomain => f.write_str("parameter is outside the mapped chart domain"),
        }
    }
}

impl Error for IntersectionEvaluationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntersectionCurveEvaluation {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
}

/// Evaluates an INTERSECTION branch using the bounded candidate for its support pair.
///
/// This dispatch only selects existing opt-in candidates. XT cone/cylinder
/// and plane/XT-cone pairs require exactly one mapped-axis convention to validate.
///
/// # Errors
/// Returns an error when the curve, support pair, or selected candidate is invalid or unsupported.
pub fn evaluate_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection { surfaces, .. } = curve else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection must reference two distinct support surfaces",
        ));
    }
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    match (&a.kind, &b.kind) {
        (SurfaceKind::Plane { .. }, SurfaceKind::Plane { .. }) => {
            evaluate_plane_plane_intersection_candidate(brep, curve, parameter)
        }
        (SurfaceKind::Plane { .. }, SurfaceKind::Offset { .. })
        | (SurfaceKind::Offset { .. }, SurfaceKind::Plane { .. }) => {
            evaluate_plane_swept_offset_intersection_candidate(brep, curve, parameter)
        }
        (SurfaceKind::Plane { .. }, SurfaceKind::Cylinder { .. })
        | (SurfaceKind::Cylinder { .. }, SurfaceKind::Plane { .. }) => {
            evaluate_plane_cylinder_intersection_candidate(brep, curve, parameter)
        }
        (SurfaceKind::Plane { .. }, SurfaceKind::XtCone { .. })
        | (SurfaceKind::XtCone { .. }, SurfaceKind::Plane { .. }) => select_unique_axis_candidate(
            evaluate_plane_xt_cone_intersection_candidate(brep, curve, parameter),
            evaluate_plane_xt_cone_intersection_candidate_with_reversed_mapped_axis(
                brep, curve, parameter,
            ),
            "both plane/XT-cone axis conventions validate",
            "neither plane/XT-cone axis convention validates",
        ),
        (SurfaceKind::Plane { .. }, SurfaceKind::Cone { .. })
        | (SurfaceKind::Cone { .. }, SurfaceKind::Plane { .. }) => {
            evaluate_plane_xt_cone_intersection_candidate_with_reversed_mapped_axis(
                brep, curve, parameter,
            )
        }
        (SurfaceKind::Cylinder { .. }, SurfaceKind::Cylinder { .. }) => {
            evaluate_cylinder_cylinder_intersection_candidate(brep, curve, parameter)
        }
        (SurfaceKind::Cone { .. }, SurfaceKind::Cylinder { .. })
        | (SurfaceKind::Cylinder { .. }, SurfaceKind::Cone { .. }) => {
            evaluate_cone_cylinder_intersection_candidate_with_axis(brep, curve, parameter, false)
        }
        (SurfaceKind::XtCone { .. }, SurfaceKind::Cylinder { .. })
        | (SurfaceKind::Cylinder { .. }, SurfaceKind::XtCone { .. }) => {
            select_unique_cone_axis_candidate(
                evaluate_cone_cylinder_intersection_candidate(brep, curve, parameter),
                evaluate_cone_cylinder_intersection_candidate_with_reversed_mapped_axis(
                    brep, curve, parameter,
                ),
            )
        }
        (SurfaceKind::Cylinder { .. }, SurfaceKind::Torus { .. })
        | (SurfaceKind::Torus { .. }, SurfaceKind::Cylinder { .. }) => {
            evaluate_cylinder_torus_intersection_candidate(brep, curve, parameter)
        }
        (
            SurfaceKind::Torus { .. },
            SurfaceKind::Swept { .. } | SurfaceKind::SweptWithSource { .. },
        )
        | (
            SurfaceKind::Swept { .. } | SurfaceKind::SweptWithSource { .. },
            SurfaceKind::Torus { .. },
        ) => evaluate_torus_ruled_sweep_intersection_candidate(brep, curve, parameter),
        _ => super::intersection_generic_dispatch::evaluate_generic_intersection(
            brep, curve, parameter,
        ),
    }
}

/// Evaluates regular `PLANE`/`OFFSET_SURF` branches whose offset basis is a mapped
/// `SWEPT_SURF`. The local UV Newton solve is bounded to the adjacent chart UV
/// rectangle and accepts exactly one root within the transmitted chordal error.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies fixed chart bounds and endpoints"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The chart solve keeps numerical updates and geometry checks in their required order."
)]
fn evaluate_plane_swept_offset_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_limit_kind,
        start_points,
        end_limit_kind,
        end_points,
        intersection_uv,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite()
        || chart_points.len() < 2
        || chart_points.len() > 16_384
        || surfaces[0] == surfaces[1]
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or supports",
        ));
    }
    if start_limit_kind != &Some(IntersectionLimitKind::Limit)
        || end_limit_kind != &Some(IntersectionLimitKind::Limit)
        || start_points.len() != 1
        || end_points.len() != 1
        || !near(start_points[0].to_array(), chart_points[0].to_array())
        || !near(
            end_points[0].to_array(),
            chart_points[chart_points.len() - 1].to_array(),
        )
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset candidate requires ordinary single-point limits matching chart endpoints",
        ));
    }
    let uv_chart = intersection_uv
        .as_ref()
        .filter(|data| {
            data.uv_type == IntersectionUvType::Both
                && data
                    .chart
                    .as_ref()
                    .is_some_and(|uv| uv.len() == chart_points.len())
        })
        .and_then(|data| data.chart.as_ref())
        .ok_or(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset candidate requires complete two-surface chart UV data",
        ))?;
    let error = chart_parameters
        .chordal_error
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset candidate requires finite nonnegative chart chordal error",
        ))?;
    let base = chart_parameters
        .base_parameter
        .filter(|v| v.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let (first, second) = (
        unique_surface(brep, surfaces[0])?,
        unique_surface(brep, surfaces[1])?,
    );
    let (plane, offset, plane_first) = match (&first.kind, &second.kind) {
        (SurfaceKind::Plane { .. }, SurfaceKind::Offset { .. }) => (first, second, true),
        (SurfaceKind::Offset { .. }, SurfaceKind::Plane { .. }) => (second, first, false),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only plane/offset intersections",
            ));
        }
    };
    let SurfaceKind::Plane {
        point: origin,
        normal,
        ..
    } = &plane.kind
    else {
        unreachable!()
    };
    let Some(plane_sense) = plane.sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let Some(offset_sense) = offset.sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let plane_normal = normalize(scale(normal.to_array(), plane_sense)).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid plane normal"),
    )?;
    let SurfaceKind::Offset { basis_surface, .. } = &offset.kind else {
        unreachable!()
    };
    let swept_surface = unique_surface(brep, *basis_surface)?;
    let Some((section_id, direction)) = swept_surface.kind.swept_section_direction() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "offset basis is not a mapped SWEPT_SURF",
        ));
    };
    let section = unique_curve(brep, section_id)?;
    if !matches!(&section.kind, CurveKind::Nurbs(_)) {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset candidate requires a mapped NURBS section",
        ));
    }
    let sweep = direction.to_array();
    let swept_data = SweptSurfaceData {
        source: swept_surface.source.clone(),
        section: Some(section.source.node_index),
        sweep: sweep.map(Some),
        // Mapped SWEPT_SURF stores the unit direction; its internal scale is not used here.
        scale: None,
    };
    let plane_origin = origin.to_array();
    if !plane_origin
        .into_iter()
        .chain(plane_normal)
        .all(f64::is_finite)
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid plane support geometry",
        ));
    }
    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(plane_origin.iter())
        .fold(1.0_f64, |maximum, value| maximum.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    // Keep plane checks strict; composed offset evaluation has a larger roundoff floor.
    let chart_support_tolerance = 256.0 * f64::EPSILON * coordinate_scale;
    let offset_evaluation_tolerance = 8192.0 * f64::EPSILON * coordinate_scale;
    let mut stations = vec![base];
    let mut tangents = Vec::with_capacity(points.len());
    let mut offset_uvs = Vec::with_capacity(points.len());
    for (point, uv) in points.iter().zip(uv_chart) {
        let (plane_uv_pair, offset_uv) = if plane_first {
            (uv.first_surface, uv.second_surface)
        } else {
            (uv.second_surface, uv.first_surface)
        };
        let [Some(plane_u), Some(plane_v)] =
            plane_uv_pair.ok_or(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart requires complete UV pairs",
            ))?
        else {
            return Err(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart requires complete UV pairs",
            ));
        };
        let [Some(u), Some(v)] = offset_uv.ok_or(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset chart requires complete UV pairs",
        ))?
        else {
            return Err(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart requires complete UV pairs",
            ));
        };
        if !u.is_finite() || !v.is_finite() || !point.iter().all(|value| value.is_finite()) {
            return Err(IntersectionEvaluationError::InvalidInput(
                "nonfinite plane/swept-offset chart data",
            ));
        }
        let plane_evaluation = super::evaluate_surface(&plane.kind, plane_u, plane_v).ok_or(
            IntersectionEvaluationError::InvalidInput("invalid plane chart parameters"),
        )?;
        let evaluation = super::evaluate_mapped_xt_swept_offset_surface_candidate(
            brep,
            offset,
            &swept_data,
            section,
            u,
            v,
        )
        .ok_or(IntersectionEvaluationError::Unsupported(
            "mapped swept offset chart evaluation failed",
        ))?;
        if norm(sub(evaluation.point, *point)) > offset_evaluation_tolerance
            || norm(sub(plane_evaluation.point, *point)) > chart_support_tolerance
            || dot(sub(*point, plane_origin), plane_normal).abs() > chart_support_tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both supports",
            ));
        }
        let offset_normal = scale(evaluation.normal, offset_sense);
        let (normal_a, normal_b) = if plane_first {
            (plane_normal, offset_normal)
        } else {
            (offset_normal, plane_normal)
        };
        let tangent = normalize(cross(normal_a, normal_b)).ok_or(
            IntersectionEvaluationError::Unsupported(
                "plane/swept-offset intersection is singular at a chart point",
            ),
        )?;
        tangents.push(tangent);
        offset_uvs.push([u, v]);
    }
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let chord_length = norm(chord);
        if !chord_length.is_finite()
            || chord_length <= tolerance
            || dot(tangents[index], chord) <= 0.0
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed plane/swept-offset chart chord",
            ));
        }
        let next = stations[index] + chord_length * scale_at;
        if !next.is_finite() || next <= stations[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        stations.push(next);
        if index + 1 < points.len() - 1 {
            let previous_cos = dot(tangents[index + 1], chord) / chord_length;
            let next_chord = sub(points[index + 2], points[index + 1]);
            let next_length = norm(next_chord);
            let next_cos = dot(tangents[index + 1], next_chord) / next_length;
            if !previous_cos.is_finite()
                || !next_cos.is_finite()
                || previous_cos <= 0.0
                || next_cos <= 0.0
            {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate plane/swept-offset chart tangent",
                ));
            }
            scale_at *= previous_cos / next_cos;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate intersection chart parameter scale",
                ));
            }
        }
    }
    let (nearest_index, nearest_station) = stations
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - parameter).abs().total_cmp(&(*b - parameter).abs()))
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let parameter_scale = parameter.abs().max(nearest_station.abs());
    if (nearest_station - parameter).abs() <= EPS_FACTOR * parameter_scale {
        return Ok(IntersectionCurveEvaluation {
            point: points[nearest_index],
            tangent: tangents[nearest_index],
        });
    }
    let last_station = stations
        .last()
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    if parameter < stations[0] || parameter > *last_station {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    let index = stations
        .windows(2)
        .position(|pair| pair[0] < parameter && parameter < pair[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - stations[index]) / (stations[index + 1] - stations[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length plane/swept-offset chart chord",
    ))?;
    let chord_point = add(points[index], scale(chord, fraction));
    let uv0 = offset_uvs[index];
    let uv1 = offset_uvs[index + 1];
    let low = [uv0[0].min(uv1[0]), uv0[1].min(uv1[1])];
    let high = [uv0[0].max(uv1[0]), uv0[1].max(uv1[1])];
    let fixed_u = low[0] == high[0];
    let fixed_v = low[1] == high[1];
    if fixed_u && fixed_v {
        // shortcut: both UVs fixed provide no search variable; add a local 3D solve if such valid intervals exist.
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset chart UV bracket is a single point",
        ));
    }
    let chart_bound = error + tolerance;
    let point_at = |uv: [f64; 2]| -> Option<[f64; 3]> {
        super::evaluate_mapped_xt_swept_offset_surface_candidate(
            brep,
            offset,
            &swept_data,
            section,
            uv[0],
            uv[1],
        )
        .map(|e| e.point)
    };
    let residual = |uv: [f64; 2]| -> Option<[f64; 2]> {
        let point = point_at(uv)?;
        Some([
            dot(sub(point, plane_origin), plane_normal),
            dot(sub(point, chord_point), chord_normal),
        ])
    };
    let solve = |mut uv: [f64; 2]| -> Option<([f64; 2], [f64; 3])> {
        for _ in 0..32 {
            let r = residual(uv)?;
            let rnorm = r[0].abs().max(r[1].abs());
            if rnorm <= tolerance {
                return Some((uv, point_at(uv)?));
            }
            let mut jac = [[0.0; 2]; 2];
            for axis in 0..2 {
                let h = ((high[axis] - low[axis]) * 1.0e-5).max(1.0e-8);
                let mut plus = uv;
                let mut minus = uv;
                plus[axis] = (uv[axis] + h).min(high[axis]);
                minus[axis] = (uv[axis] - h).max(low[axis]);
                let rp = residual(plus)?;
                let rm = residual(minus)?;
                let span = plus[axis] - minus[axis];
                if span <= 0.0 {
                    return None;
                }
                for row in 0..2 {
                    jac[row][axis] = (rp[row] - rm[row]) / span;
                }
            }
            let det = jac[0][0] * jac[1][1] - jac[0][1] * jac[1][0];
            let jac_scale =
                (jac[0][0].abs() + jac[0][1].abs()) * (jac[1][0].abs() + jac[1][1].abs());
            if !det.is_finite() || jac_scale == 0.0 || det.abs() <= EPS_FACTOR * jac_scale {
                return None;
            }
            let step = [
                (-r[0] * jac[1][1] + r[1] * jac[0][1]) / det,
                (-r[1] * jac[0][0] + r[0] * jac[1][0]) / det,
            ];
            if step.iter().any(|value| !value.is_finite()) {
                return None;
            }
            let mut alpha = 1.0;
            let mut accepted = None;
            for _ in 0..16 {
                let candidate = [uv[0] + alpha * step[0], uv[1] + alpha * step[1]];
                if candidate[0] >= low[0]
                    && candidate[0] <= high[0]
                    && candidate[1] >= low[1]
                    && candidate[1] <= high[1]
                    && residual(candidate)
                        .is_some_and(|next| next[0].abs().max(next[1].abs()) < rnorm)
                {
                    accepted = Some(candidate);
                    break;
                }
                alpha *= 0.5;
            }
            uv = accepted?;
        }
        None
    };
    let mut roots: Vec<([f64; 2], [f64; 3])> = Vec::new();
    let varying_axis = if fixed_u {
        Some(1)
    } else if fixed_v {
        Some(0)
    } else {
        None
    };
    if let Some(axis) = varying_axis {
        // shortcut: eight samples can miss extra roots; use interval isolation when branch completeness is required.
        const CHART_PLANE_SUBINTERVALS: usize = 8;
        let subinterval_count = u32::try_from(CHART_PLANE_SUBINTERVALS).map_err(|_| {
            IntersectionEvaluationError::InvalidInput("chart sample count exceeds u32")
        })?;
        let fixed_axis = 1 - axis;
        let mut samples = Vec::with_capacity(CHART_PLANE_SUBINTERVALS + 1);
        for step in 0..=subinterval_count {
            let mut uv = [0.0; 2];
            uv[fixed_axis] = low[fixed_axis];
            uv[axis] = low[axis]
                + (high[axis] - low[axis]) * f64::from(step) / f64::from(subinterval_count);
            let point = point_at(uv).ok_or(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart-plane sample evaluation failed",
            ))?;
            let value = dot(sub(point, chord_point), chord_normal);
            if !value.is_finite() {
                return Err(IntersectionEvaluationError::Unsupported(
                    "nonfinite plane/swept-offset chart-plane sample",
                ));
            }
            samples.push((uv, value));
        }
        let opposite_sign = |a: f64, b: f64| (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0);
        let mut brackets = Vec::new();
        for sample_index in 1..CHART_PLANE_SUBINTERVALS {
            if samples[sample_index].1 == 0.0 {
                let left = (0..sample_index)
                    .rev()
                    .find(|index| samples[*index].1 != 0.0);
                let right = (sample_index + 1..=CHART_PLANE_SUBINTERVALS)
                    .find(|index| samples[*index].1 != 0.0);
                if let (Some(left), Some(right)) = (left, right) {
                    if opposite_sign(samples[left].1, samples[right].1) {
                        brackets.push((left, right));
                    }
                }
            }
        }
        for sample_index in 0..CHART_PLANE_SUBINTERVALS {
            if opposite_sign(samples[sample_index].1, samples[sample_index + 1].1) {
                brackets.push((sample_index, sample_index + 1));
            }
        }
        if brackets.len() != 1 || samples.iter().filter(|sample| sample.1 == 0.0).count() > 1 {
            return Err(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart plane does not select one sampled 1D root",
            ));
        }
        let (left, right) = brackets[0];
        let (mut low_uv, mut low_value) = samples[left];
        let (mut high_uv, _) = samples[right];
        let mut root = None;
        for _ in 0..64 {
            let mut uv = low_uv;
            uv[axis] = (low_uv[axis] + high_uv[axis]) * 0.5;
            let point = point_at(uv).ok_or(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart-plane refinement failed",
            ))?;
            let value = dot(sub(point, chord_point), chord_normal);
            if !value.is_finite() {
                return Err(IntersectionEvaluationError::Unsupported(
                    "nonfinite plane/swept-offset chart-plane refinement",
                ));
            }
            if value.abs() <= chart_support_tolerance {
                root = Some((uv, point));
                break;
            }
            if opposite_sign(low_value, value) {
                high_uv = uv;
            } else {
                low_uv = uv;
                low_value = value;
            }
        }
        let Some(root) = root else {
            return Err(IntersectionEvaluationError::Unsupported(
                "plane/swept-offset chart-plane root did not reach roundoff tolerance",
            ));
        };
        if norm(sub(root.1, chord_point)) <= chart_bound {
            roots.push(root);
        }
    } else {
        for fu in [0.1, 0.5, 0.9] {
            for fv in [0.1, 0.5, 0.9] {
                let seed = [
                    low[0] + fu * (high[0] - low[0]),
                    low[1] + fv * (high[1] - low[1]),
                ];
                if let Some(root) = solve(seed) {
                    if norm(sub(root.1, chord_point)) <= chart_bound
                        && !roots
                            .iter()
                            .any(|other| norm(sub(other.1, root.1)) <= 16.0 * tolerance)
                    {
                        roots.push(root);
                    }
                }
            }
        }
    }
    let [(root_uv, point)] = roots.as_slice() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset chart plane does not select one bounded local root",
        ));
    };
    if dot(sub(*point, plane_origin), plane_normal).abs() > chart_support_tolerance
        || dot(sub(*point, chord_point), chord_normal).abs() > chart_support_tolerance
        || norm(sub(*point, chord_point)) > chart_bound
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset root fails support or chart bounds",
        ));
    }
    let evaluation = super::evaluate_mapped_xt_swept_offset_surface_candidate(
        brep,
        offset,
        &swept_data,
        section,
        root_uv[0],
        root_uv[1],
    )
    .ok_or(IntersectionEvaluationError::Unsupported(
        "mapped swept offset root evaluation failed",
    ))?;
    let offset_normal = scale(evaluation.normal, offset_sense);
    let (normal_a, normal_b) = if plane_first {
        (plane_normal, offset_normal)
    } else {
        (offset_normal, plane_normal)
    };
    let tangent =
        normalize(cross(normal_a, normal_b)).ok_or(IntersectionEvaluationError::Unsupported(
            "plane/swept-offset intersection is singular at evaluated point",
        ))?;
    Ok(IntersectionCurveEvaluation {
        point: *point,
        tangent: if dot(tangent, tangents[index]) < 0.0 {
            scale(tangent, -1.0)
        } else {
            tangent
        },
    })
}

fn select_unique_cone_axis_candidate(
    mapped_axis: Result<IntersectionCurveEvaluation, IntersectionEvaluationError>,
    reversed_axis: Result<IntersectionCurveEvaluation, IntersectionEvaluationError>,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    select_unique_axis_candidate(
        mapped_axis,
        reversed_axis,
        "both cone/cylinder axis conventions validate",
        "neither cone/cylinder axis convention validates",
    )
}

fn select_unique_axis_candidate(
    mapped_axis: Result<IntersectionCurveEvaluation, IntersectionEvaluationError>,
    reversed_axis: Result<IntersectionCurveEvaluation, IntersectionEvaluationError>,
    both_valid_message: &'static str,
    neither_valid_message: &'static str,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    match (mapped_axis, reversed_axis) {
        (Ok(value), Err(_)) | (Err(_), Ok(value)) => Ok(value),
        (Ok(_), Ok(_)) => Err(IntersectionEvaluationError::Unsupported(both_valid_message)),
        (Err(mapped), Err(reversed)) if mapped == reversed => Err(mapped),
        (Err(_), Err(_)) => Err(IntersectionEvaluationError::Unsupported(
            neither_valid_message,
        )),
    }
}

/// Evaluates a regular torus/ruled-sweep branch inside its mapped chart.
///
/// This opt-in requires complete two-surface UV data, ordinary `L` limits,
/// and one sampled chart-plane root in the section-parameter bracket.
///
/// # Errors
/// Returns an error when the chart, support geometry, or selected root is invalid or unsupported.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies stored chart stations"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The chart evaluation keeps root selection and support validation together in their required order."
)]
pub fn evaluate_torus_ruled_sweep_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_limit_kind,
        start_points,
        end_limit_kind,
        end_points,
        intersection_uv,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 || surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or support references",
        ));
    }
    let chart_start = chart_points[0].to_array();
    let chart_end = chart_points[chart_points.len() - 1].to_array();
    let start_chart_point = match start_limit_kind {
        Some(IntersectionLimitKind::Limit) if start_points.len() == 1 => start_points[0],
        Some(IntersectionLimitKind::Terminator) if start_points.len() == 2 => start_points[1],
        Some(IntersectionLimitKind::Limit | IntersectionLimitKind::Terminator) => {
            return Err(IntersectionEvaluationError::InvalidInput(
                "torus/swept limit has the wrong number of points",
            ));
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "torus/swept candidate requires ordinary or terminator limits",
            ));
        }
    };
    let end_chart_point = match end_limit_kind {
        Some(IntersectionLimitKind::Limit) if end_points.len() == 1 => end_points[0],
        Some(IntersectionLimitKind::Terminator) if end_points.len() == 2 => end_points[1],
        Some(IntersectionLimitKind::Limit | IntersectionLimitKind::Terminator) => {
            return Err(IntersectionEvaluationError::InvalidInput(
                "torus/swept limit has the wrong number of points",
            ));
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "torus/swept candidate requires ordinary or terminator limits",
            ));
        }
    };
    if start_points
        .iter()
        .chain(end_points)
        .any(|point| !point.to_array().into_iter().all(f64::is_finite))
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "torus/swept limit contains a nonfinite point",
        ));
    }
    if !near(start_chart_point.to_array(), chart_start)
        || !near(end_chart_point.to_array(), chart_end)
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "torus/swept limit branch point disagrees with chart endpoint",
        ));
    }
    let error = chart_parameters
        .chordal_error
        .filter(|error| error.is_finite() && *error >= 0.0)
        .ok_or(IntersectionEvaluationError::Unsupported(
            "torus/swept candidate requires finite nonnegative chart chordal error",
        ))?;
    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let uv_data = intersection_uv
        .as_ref()
        .filter(|data| {
            data.uv_type == IntersectionUvType::Both
                && data
                    .chart
                    .as_ref()
                    .is_some_and(|uv| uv.len() == chart_points.len())
        })
        .ok_or(IntersectionEvaluationError::Unsupported(
            "torus/swept candidate requires complete two-surface chart UV data",
        ))?;
    let support_a = unique_surface(brep, surfaces[0])?;
    let support_b = unique_surface(brep, surfaces[1])?;
    let (torus, swept, torus_first) = match (&support_a.kind, &support_b.kind) {
        (
            SurfaceKind::Torus { .. },
            SurfaceKind::Swept { .. } | SurfaceKind::SweptWithSource { .. },
        ) => (support_a, support_b, true),
        (
            SurfaceKind::Swept { .. } | SurfaceKind::SweptWithSource { .. },
            SurfaceKind::Torus { .. },
        ) => (support_b, support_a, false),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only torus/ruled-sweep intersections",
            ));
        }
    };
    let torus_sense = torus
        .sense
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ))?;
    let swept_sense = swept
        .sense
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ))?;
    let SurfaceKind::Torus {
        center,
        axis,
        major_radius,
        minor_radius,
        ..
    } = &torus.kind
    else {
        unreachable!();
    };
    let Some((section, direction)) = swept.kind.swept_section_direction() else {
        unreachable!();
    };
    let section_geometry = unique_curve(brep, section)?;
    let CurveKind::Nurbs(section_curve) = &section_geometry.kind else {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept candidate requires a mapped NURBS section curve",
        ));
    };
    let sweep = direction.to_array();
    let sweep_length = norm(sweep);
    if !sweep_length.is_finite() || (sweep_length - 1.0).abs() > 64.0 * f64::EPSILON {
        return Err(IntersectionEvaluationError::InvalidInput(
            "mapped swept direction is not unit length",
        ));
    }
    let uv_chart = uv_data
        .chart
        .as_ref()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "torus/swept candidate requires complete two-surface chart UV data",
        ))?;
    let mut points = Vec::with_capacity(chart_points.len());
    let mut tangents = Vec::with_capacity(chart_points.len());
    let mut section_uv = Vec::with_capacity(chart_points.len());
    for (chart_point, uv) in chart_points.iter().zip(uv_chart) {
        let first_uv = complete_uv(uv.first_surface)?;
        let second_uv = complete_uv(uv.second_surface)?;
        let (torus_uv, swept_uv) = if torus_first {
            (first_uv, second_uv)
        } else {
            (second_uv, first_uv)
        };
        let torus_evaluation = super::evaluate_surface(&torus.kind, torus_uv[0], torus_uv[1])
            .ok_or(IntersectionEvaluationError::InvalidInput(
                "invalid torus chart parameters",
            ))?;
        let swept_point = super::evaluate_ruled_sweep_point_candidate(
            section_curve,
            sweep,
            swept_uv[0],
            swept_uv[1],
        )
        .map_err(|_| IntersectionEvaluationError::InvalidInput("invalid swept chart parameters"))?;
        let point = chart_point.to_array();
        if !point
            .into_iter()
            .chain(torus_evaluation.point)
            .chain(torus_evaluation.normal)
            .chain(swept_point)
            .all(f64::is_finite)
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart support evaluation is nonfinite",
            ));
        }
        let tolerance = EPS_FACTOR
            * point
                .into_iter()
                .chain(torus_evaluation.point)
                .chain(swept_point)
                .fold(1.0_f64, |m, value| m.max(value.abs()));
        if norm(sub(torus_evaluation.point, point)) > tolerance
            || norm(sub(swept_point, point)) > tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point disagrees with its mapped support UV",
            ));
        }
        let section_tangent = nurbs_section_tangent(section_curve, swept_uv[0])?;
        let torus_normal = scale(torus_evaluation.normal, torus_sense);
        let swept_normal = scale(cross(section_tangent, sweep), swept_sense);
        let (normal_a, normal_b) = if torus_first {
            (torus_normal, swept_normal)
        } else {
            (swept_normal, torus_normal)
        };
        let tangent = normalize(cross(normal_a, normal_b)).ok_or(
            IntersectionEvaluationError::Unsupported(
                "torus/swept intersection is singular at a chart point",
            ),
        )?;
        points.push(point);
        tangents.push(tangent);
        section_uv.push(swept_uv);
    }

    let mut parameters = vec![base];
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length = norm(chord);
        let coordinate_scale = points[index]
            .into_iter()
            .chain(points[index + 1])
            .fold(1.0_f64, |m, value| m.max(value.abs()));
        if !length.is_finite()
            || length <= EPS_FACTOR * coordinate_scale
            || dot(tangents[index], chord) <= 0.0
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed torus/swept chart chord",
            ));
        }
        let next = parameters[index] + length * scale_at;
        if !next.is_finite() || next <= parameters[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        parameters.push(next);
        if index + 1 < points.len() - 1 {
            let next_chord = sub(points[index + 2], points[index + 1]);
            let cos_b = dot(tangents[index + 1], chord) / length;
            let cos_a = dot(tangents[index + 1], next_chord) / norm(next_chord);
            if !cos_a.is_finite() || cos_a <= 0.0 || !cos_b.is_finite() || cos_b <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate torus/swept chart tangent",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate intersection chart parameter scale",
                ));
            }
        }
    }
    if parameter < parameters[0] || parameter > parameters[parameters.len() - 1] {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = parameters.iter().position(|value| *value == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[index],
            tangent: tangents[index],
        });
    }
    let index = parameters
        .windows(2)
        .position(|window| window[0] < parameter && parameter < window[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - parameters[index]) / (parameters[index + 1] - parameters[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_point = add(points[index], scale(chord, fraction));
    let chord_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length torus/swept chart chord",
    ))?;
    let u0 = section_uv[index][0];
    let u1 = section_uv[index + 1][0];
    if !u0.is_finite() || !u1.is_finite() || u0 == u1 {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept chart segment has no section-parameter bracket",
        ));
    }
    let candidate = super::intersect_torus_ruled_sweep_with_chart_plane_candidate(
        section_curve,
        sweep,
        &torus.kind,
        chord_point,
        chord_normal,
        [u0.min(u1), u0.max(u1)],
    )
    .map_err(|error| match error {
        super::ChartIntersectionError::InvalidInput(message) => {
            IntersectionEvaluationError::InvalidInput(message)
        }
        _ => IntersectionEvaluationError::Unsupported(
            "chart plane does not have one sampled torus/swept intersection root",
        ),
    })?;
    let coordinate_scale = chord_point
        .into_iter()
        .chain(candidate.point)
        .chain(center.to_array())
        .fold(1.0_f64, |m, value| m.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    if !candidate.point.into_iter().all(f64::is_finite)
        || !candidate.uv.into_iter().all(f64::is_finite)
        || candidate.torus_residual > 1024.0 * f64::EPSILON
        || dot(chord_normal, sub(candidate.point, chord_point)).abs() > tolerance
        || norm(sub(candidate.point, chord_point)) > error + tolerance
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept root fails chart or support consistency checks",
        ));
    }
    let section_tangent = nurbs_section_tangent(section_curve, candidate.uv[0])?;
    let torus_normal = torus_normal_at_point(
        candidate.point,
        center.to_array(),
        axis.to_array(),
        *major_radius,
        *minor_radius,
    )?;
    let swept_normal = normalize(cross(section_tangent, sweep)).ok_or(
        IntersectionEvaluationError::Unsupported(
            "torus/swept intersection is singular at the candidate point",
        ),
    )?;
    let torus_normal = scale(torus_normal, torus_sense);
    let swept_normal = scale(swept_normal, swept_sense);
    let (normal_a, normal_b) = if torus_first {
        (torus_normal, swept_normal)
    } else {
        (swept_normal, torus_normal)
    };
    let tangent =
        normalize(cross(normal_a, normal_b)).ok_or(IntersectionEvaluationError::Unsupported(
            "torus/swept intersection is singular at the candidate point",
        ))?;
    if dot(tangent, chord) <= 0.0 {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept tangent disagrees with chart order",
        ));
    }
    Ok(IntersectionCurveEvaluation {
        point: candidate.point,
        tangent,
    })
}

/// Evaluates the exact straight branch formed by two mapped planes.
///
/// The chart parameter is reconstructed from the XT base parameter/scale and
/// ordered chart chord lengths. XT's three-surface construction reduces to
/// the same chord point for a plane/plane branch. Other support pairs,
/// terminator limits, missing limits, inconsistent chart points, and queries
/// outside the chart interval fail closed. Coordinate consistency uses a
/// roundoff bound of 64 binary64 epsilons times the largest coordinate scale;
/// this is not a source geometric tolerance.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or mapped intersection is invalid.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies a stored chart station"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The exact straight-branch reconstruction and validation flow must remain ordered together."
)]
pub fn evaluate_plane_plane_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_points,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection parameter is not finite",
        ));
    }
    if chart_points.len() < 2 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart requires at least two points",
        ));
    }
    if chart_points
        .iter()
        .any(|point| !point.to_array().into_iter().all(f64::is_finite))
        || start_points
            .iter()
            .chain(end_points)
            .any(|point| !point.to_array().into_iter().all(f64::is_finite))
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart or limit contains a nonfinite point",
        ));
    }
    let Some(base_parameter) = chart_parameters.base_parameter.filter(|x| x.is_finite()) else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ));
    };
    let Some(base_scale) = chart_parameters
        .base_scale
        .filter(|x| x.is_finite() && *x > 0.0)
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ));
    };
    if start_points.len() != 1
        || end_points.len() != 1
        || !near(start_points[0].to_array(), chart_points[0].to_array())
        || !near(
            end_points[0].to_array(),
            chart_points[chart_points.len() - 1].to_array(),
        )
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection limits must be single points matching chart endpoints",
        ));
    }

    if surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection must reference two distinct support surfaces",
        ));
    }
    let surface_a = unique_surface(brep, surfaces[0])?;
    let surface_b = unique_surface(brep, surfaces[1])?;
    let (
        SurfaceKind::Plane {
            point: origin_a,
            normal: normal_a,
            ..
        },
        SurfaceKind::Plane {
            point: origin_b,
            normal: normal_b,
            ..
        },
    ) = (&surface_a.kind, &surface_b.kind)
    else {
        return Err(IntersectionEvaluationError::Unsupported(
            "candidate supports only plane/plane intersections",
        ));
    };
    let Some(sense_a) = surface_a.sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let Some(sense_b) = surface_b.sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let n_a = normalize(scale(normal_a.to_array(), sense_a)).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid first plane normal"),
    )?;
    let n_b = normalize(scale(normal_b.to_array(), sense_b)).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid second plane normal"),
    )?;
    let tangent = normalize(cross(n_a, n_b)).ok_or(IntersectionEvaluationError::Unsupported(
        "parallel planes do not define a regular intersection curve",
    ))?;

    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    let origin_a = origin_a.to_array();
    let origin_b = origin_b.to_array();
    if !origin_a.into_iter().chain(origin_b).all(f64::is_finite) {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection support origin is not finite",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(origin_a.iter())
        .chain(origin_b.iter())
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    for point in &points {
        if dot(sub(*point, origin_a), n_a).abs() > tolerance
            || dot(sub(*point, origin_b), n_b).abs() > tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support planes",
            ));
        }
    }

    let mut chart_parameters = Vec::with_capacity(points.len());
    chart_parameters.push(base_parameter);
    let mut scale_at_point = base_scale;
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let chord_length = norm(chord);
        if !chord_length.is_finite() || chord_length <= tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart contains a zero-length chord",
            ));
        }
        if dot(tangent, chord) <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "chart order disagrees with the oriented surface-normal cross product",
            ));
        }
        if !collinear(sub(points[index], points[0]), tangent, tolerance)
            || !collinear(sub(points[index + 1], points[0]), tangent, tolerance)
        {
            return Err(IntersectionEvaluationError::Unsupported(
                "chart points do not define one straight plane/plane branch",
            ));
        }
        let next_parameter = chart_parameters[index] + chord_length * scale_at_point;
        if !next_parameter.is_finite() || next_parameter <= chart_parameters[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        chart_parameters.push(next_parameter);

        if index + 1 < points.len() - 1 {
            let next_chord = sub(points[index + 2], points[index + 1]);
            let previous_chord = chord;
            let cos_b = dot(tangent, previous_chord) / chord_length;
            let cos_a = dot(tangent, next_chord) / norm(next_chord);
            if !cos_a.is_finite() || !cos_b.is_finite() || cos_a == 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "intersection chart tangent is degenerate",
                ));
            }
            scale_at_point *= cos_b / cos_a;
            if !scale_at_point.is_finite() || scale_at_point <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "intersection chart parameter scale is degenerate",
                ));
            }
        }
    }
    let first = chart_parameters[0];
    let last = chart_parameters[chart_parameters.len() - 1];
    if parameter < first || parameter > last {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = chart_parameters.iter().position(|&t| t == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[index],
            tangent,
        });
    }
    let index = chart_parameters
        .windows(2)
        .position(|interval| interval[0] < parameter && parameter < interval[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - chart_parameters[index])
        / (chart_parameters[index + 1] - chart_parameters[index]);
    let point = add(
        points[index],
        scale(sub(points[index + 1], points[index]), fraction),
    );
    // Recheck the candidate against both supports; no source-space tolerance is inferred.
    if dot(sub(point, origin_a), n_a).abs() > tolerance
        || dot(sub(point, origin_b), n_b).abs() > tolerance
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "evaluated point failed support-plane consistency check",
        ));
    }
    Ok(IntersectionCurveEvaluation { point, tangent })
}

/// Evaluates a regular plane/cylinder branch using XT's chart plane.
///
/// The two chart supports must be one plane and one cylinder. Interior points
/// are the intersections of those supports and the plane through the chart
/// chord point normal to that chord. Two valid quadratic roots are rejected
/// unless exactly one lies within the transmitted chordal error of the chord.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter and endpoint equality preserve chart selection semantics"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The chart root solve and geometric checks must preserve their current numeric and validation order."
)]
pub fn evaluate_plane_cylinder_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_points,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter or chart",
        ));
    }
    if surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection must reference two distinct support surfaces",
        ));
    }
    let surface_a = unique_surface(brep, surfaces[0])?;
    let surface_b = unique_surface(brep, surfaces[1])?;
    let (plane, cylinder, order) = match (&surface_a.kind, &surface_b.kind) {
        (
            SurfaceKind::Plane { point, normal, .. },
            SurfaceKind::Cylinder {
                point: cp,
                axis,
                radius,
                ..
            },
        ) => (
            (point.to_array(), normal.to_array(), surface_a.sense),
            (cp.to_array(), axis.to_array(), *radius, surface_b.sense),
            1.0,
        ),
        (
            SurfaceKind::Cylinder {
                point: cp,
                axis,
                radius,
                ..
            },
            SurfaceKind::Plane { point, normal, .. },
        ) => (
            (point.to_array(), normal.to_array(), surface_b.sense),
            (cp.to_array(), axis.to_array(), *radius, surface_a.sense),
            -1.0,
        ),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only plane/cylinder intersections",
            ));
        }
    };
    let Some(plane_sense) = plane.2.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let Some(cylinder_sense) = cylinder.3.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let pn = normalize(scale(plane.1, plane_sense)).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid plane normal"),
    )?;
    let axis = normalize(cylinder.1).ok_or(IntersectionEvaluationError::InvalidInput(
        "invalid cylinder axis",
    ))?;
    let (center, radius) = (cylinder.0, cylinder.2);
    if !radius.is_finite()
        || radius <= 0.0
        || !center.into_iter().chain(plane.0).all(f64::is_finite)
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid plane/cylinder geometry",
        ));
    }
    let points: Vec<_> = chart_points
        .iter()
        .map(|chart_point| chart_point.to_array())
        .collect();
    let (Some(&first_point), Some(&last_point)) = (points.first(), points.last()) else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart is empty",
        ));
    };
    if points.iter().flatten().any(|x| !x.is_finite())
        || start_points.len() != 1
        || end_points.len() != 1
        || !near(start_points[0].to_array(), first_point)
        || !near(end_points[0].to_array(), last_point)
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection requires finite chart points and single-point limits matching chart endpoints",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(center.iter())
        .chain(plane.0.iter())
        .fold(1.0_f64, |m, x| m.max(x.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    for point in &points {
        let radial = sub(
            sub(*point, center),
            scale(axis, dot(sub(*point, center), axis)),
        );
        if dot(sub(*point, plane.0), pn).abs() > tolerance
            || (norm(radial) - radius).abs() > tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support surfaces",
            ));
        }
    }
    let tangent_at = |point: [f64; 3]| -> Result<[f64; 3], IntersectionEvaluationError> {
        let radial = sub(
            sub(point, center),
            scale(axis, dot(sub(point, center), axis)),
        );
        normalize(scale(cross(pn, scale(radial, cylinder_sense)), order)).ok_or(
            IntersectionEvaluationError::Unsupported(
                "plane/cylinder intersection is singular at a chart point",
            ),
        )
    };
    let base = chart_parameters
        .base_parameter
        .filter(|x| x.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let mut ts = vec![base];
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|x| x.is_finite() && *x > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    for station_index in 0..points.len() - 1 {
        let chord = sub(points[station_index + 1], points[station_index]);
        let length = norm(chord);
        let tangent = tangent_at(points[station_index])?;
        if !length.is_finite() || length <= tolerance || dot(tangent, chord) <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed intersection chart chord",
            ));
        }
        let next_parameter = ts[station_index] + length * scale_at;
        if !next_parameter.is_finite() || next_parameter <= ts[station_index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        ts.push(next_parameter);
        if station_index + 1 < points.len() - 1 {
            let previous = chord;
            let next = sub(points[station_index + 2], points[station_index + 1]);
            let station_tangent = tangent_at(points[station_index + 1])?;
            let previous_length = norm(previous);
            let next_length = norm(next);
            let cos_b = dot(station_tangent, previous) / previous_length;
            let cos_a = dot(station_tangent, next) / next_length;
            if !cos_a.is_finite() || cos_a <= 0.0 || !cos_b.is_finite() || cos_b <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate chart tangent",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate chart parameter scale",
                ));
            }
        }
    }
    let Some(&last_parameter) = ts.last() else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart parameter range is empty",
        ));
    };
    if parameter < ts[0] || parameter > last_parameter {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(station_index) = ts.iter().position(|station| *station == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[station_index],
            tangent: tangent_at(points[station_index])?,
        });
    }
    let station_index = ts
        .windows(2)
        .position(|w| w[0] < parameter && parameter < w[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - ts[station_index]) / (ts[station_index + 1] - ts[station_index]);
    let chord_point = add(
        points[station_index],
        scale(
            sub(points[station_index + 1], points[station_index]),
            fraction,
        ),
    );
    let chord_normal = normalize(sub(points[station_index + 1], points[station_index])).ok_or(
        IntersectionEvaluationError::InvalidInput("zero-length chart chord"),
    )?;
    let line_dir =
        normalize(cross(chord_normal, pn)).ok_or(IntersectionEvaluationError::Unsupported(
            "chart plane and plane support do not define a unique line",
        ))?;
    let plane_offset = dot(pn, plane.0);
    let chord_offset = dot(chord_normal, chord_point);
    let cross_n = cross(chord_normal, pn);
    let line_point = scale(
        add(
            scale(cross(pn, cross_n), chord_offset),
            scale(cross(cross_n, chord_normal), plane_offset),
        ),
        1.0 / dot(cross_n, cross_n),
    );
    let from_center = sub(line_point, center);
    let radial_origin = sub(from_center, scale(axis, dot(from_center, axis)));
    let radial_dir = sub(line_dir, scale(axis, dot(line_dir, axis)));
    let aa = dot(radial_dir, radial_dir);
    let bb = 2.0 * dot(radial_origin, radial_dir);
    let cc = dot(radial_origin, radial_origin) - radius * radius;
    let disc = bb * bb - 4.0 * aa * cc;
    if !disc.is_finite() || aa <= 0.0 || disc < 0.0 {
        return Err(IntersectionEvaluationError::Unsupported(
            "chart plane has no isolated plane/cylinder intersection",
        ));
    }
    let mut candidates = vec![
        add(
            line_point,
            scale(line_dir, (-bb - disc.sqrt()) / (2.0 * aa)),
        ),
        add(
            line_point,
            scale(line_dir, (-bb + disc.sqrt()) / (2.0 * aa)),
        ),
    ];
    candidates
        .dedup_by(|first_candidate, second_candidate| near(*first_candidate, *second_candidate));
    let point = match candidates.as_slice() {
        [single_candidate] => *single_candidate,
        [first_candidate, second_candidate] => {
            let Some(error) = chart_parameters
                .chordal_error
                .filter(|e| e.is_finite() && *e >= 0.0)
            else {
                return Err(IntersectionEvaluationError::Unsupported(
                    "ambiguous plane/cylinder chart-plane roots without chordal error",
                ));
            };
            let first_distance = norm(sub(*first_candidate, chord_point));
            let second_distance = norm(sub(*second_candidate, chord_point));
            let limit = error + tolerance;
            match (first_distance <= limit, second_distance <= limit) {
                (true, false) => *first_candidate,
                (false, true) => *second_candidate,
                _ => {
                    return Err(IntersectionEvaluationError::Unsupported(
                        "ambiguous plane/cylinder chart-plane roots",
                    ));
                }
            }
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "ambiguous plane/cylinder chart-plane roots",
            ));
        }
    };
    let radial = sub(
        sub(point, center),
        scale(axis, dot(sub(point, center), axis)),
    );
    if dot(sub(point, plane.0), pn).abs() > tolerance || (norm(radial) - radius).abs() > tolerance {
        return Err(IntersectionEvaluationError::InvalidInput(
            "evaluated point failed support-surface consistency check",
        ));
    }
    let tangent = tangent_at(point)?;
    Ok(IntersectionCurveEvaluation { point, tangent })
}

/// Evaluates a regular plane/XT-cone branch using the XT chart section plane.
///
/// The support plane and the plane through the chart chord normal to that
/// chord define a line; substituting it into the mapped `P - vA` cone equation
/// gives at most two candidates, selected only when chordal error admits one.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
pub fn evaluate_plane_xt_cone_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    evaluate_plane_xt_cone_intersection_candidate_with_axis(brep, curve, parameter, false)
}

/// Evaluates a plane/XT-cone branch with the mapped XT axial direction reversed.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
pub fn evaluate_plane_xt_cone_intersection_candidate_with_reversed_mapped_axis(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    evaluate_plane_xt_cone_intersection_candidate_with_axis(brep, curve, parameter, true)
}

#[allow(
    clippy::float_cmp,
    reason = "the exact endpoint fraction selects the stored chart endpoint"
)]
fn chart_parameter_at_fraction(parameters: &[f64], index: usize, fraction: f64) -> f64 {
    if fraction == 0.0 {
        parameters[index]
    } else if fraction == 1.0 {
        parameters[index + 1]
    } else {
        parameters[index] + fraction * (parameters[index + 1] - parameters[index])
    }
}

/// Recovers a mapped parameter for a point on a regular plane/XT-cone chart.
/// Each chart chord contributes only its orthogonal section; the generic
/// evaluator must reproduce the requested point within the caller's tolerance.
#[allow(
    clippy::float_cmp,
    reason = "exact point and parameter equality select chart endpoints and deduplicate roots"
)]
pub fn inverse_plane_xt_cone_intersection_parameter(
    brep: &BrepModel,
    curve: &CurveKind,
    point: [f64; 3],
    tolerance: f64,
) -> Result<f64, IntersectionEvaluationError> {
    const MAX_CHART_POINTS: usize = 256;
    if !point.into_iter().all(f64::is_finite) || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "inverse intersection point and tolerance must be finite, with positive tolerance",
        ));
    }
    let CurveKind::Intersection { chart_points, .. } = curve else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if chart_points.len() < 2 || chart_points.len() > MAX_CHART_POINTS {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/XT-cone inverse requires a bounded chart of 2 to 256 points",
        ));
    }
    let cone_is_profile_mapped = intersection_is_plane_regular_cone(brep, curve)?;
    let (points, parameters) = if cone_is_profile_mapped {
        plane_xt_cone_inverse_chart_parameters(brep, curve, true)?
    } else {
        let mapped = plane_xt_cone_inverse_chart_parameters(brep, curve, false);
        let reversed = plane_xt_cone_inverse_chart_parameters(brep, curve, true);
        match (mapped, reversed) {
            (Ok(value), Err(_)) | (Err(_), Ok(value)) => value,
            (Ok(_), Ok(_)) => {
                return Err(IntersectionEvaluationError::Unsupported(
                    "both plane/XT-cone axis conventions validate for inverse parameterization",
                ));
            }
            (Err(mapped), Err(reversed)) if mapped == reversed => return Err(mapped),
            (Err(_), Err(_)) => {
                return Err(IntersectionEvaluationError::Unsupported(
                    "neither plane/XT-cone axis convention validates for inverse parameterization",
                ));
            }
        }
    };
    let mut matches = Vec::new();
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length2 = dot(chord, chord);
        if !length2.is_finite() || length2 <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate plane/XT-cone chart chord",
            ));
        }
        let fraction = if point == points[index] {
            0.0
        } else if point == points[index + 1] {
            1.0
        } else {
            dot(sub(point, points[index]), chord) / length2
        };
        if !fraction.is_finite() {
            continue;
        }
        let fraction = if index == 0 && fraction < 0.0 {
            0.0
        } else if index + 2 == points.len() && fraction > 1.0 {
            1.0
        } else if (0.0..=1.0).contains(&fraction) {
            fraction
        } else {
            continue;
        };
        let parameter = chart_parameter_at_fraction(&parameters, index, fraction);
        let candidate = evaluate_intersection_candidate(brep, curve, parameter)?;
        if norm(sub(candidate.point, point)) <= tolerance {
            matches.push((parameter, candidate.point));
        }
    }
    matches.sort_by(|a, b| a.0.total_cmp(&b.0));
    matches.dedup_by(|a, b| a.0 == b.0 && norm(sub(a.1, b.1)) <= tolerance);
    match matches.as_slice() {
        [(parameter, _)] => Ok(*parameter),
        [] => Err(IntersectionEvaluationError::Unsupported(
            "no chart section uniquely matches the requested endpoint within tolerance",
        )),
        _ => Err(IntersectionEvaluationError::Unsupported(
            "multiple chart sections match the requested endpoint within tolerance",
        )),
    }
}

fn intersection_is_plane_regular_cone(
    brep: &BrepModel,
    curve: &CurveKind,
) -> Result<bool, IntersectionEvaluationError> {
    let CurveKind::Intersection { surfaces, .. } = curve else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    Ok(matches!(
        (&a.kind, &b.kind),
        (SurfaceKind::Plane { .. }, SurfaceKind::Cone { .. })
            | (SurfaceKind::Cone { .. }, SurfaceKind::Plane { .. })
    ))
}

#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies a stored chart station"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The cone chart solve keeps branch selection and geometric validation in their required order."
)]
fn evaluate_plane_xt_cone_intersection_candidate_with_axis(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
    reverse_xt_axis: bool,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_limit_kind,
        start_points,
        end_limit_kind,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 || surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or support references",
        ));
    }
    // shortcut: only apex T records inside CHART are supported; add other singularities/continuation with validated XT mapping.
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    let (plane, cone, order) = match (&a.kind, &b.kind) {
        (
            SurfaceKind::Plane { point, normal, .. },
            SurfaceKind::XtCone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            }
            | SurfaceKind::Cone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            },
        ) => (
            (point.to_array(), normal.to_array(), a.sense),
            (
                cp.to_array(),
                axis.to_array(),
                *radius,
                *sin_half_angle,
                *cos_half_angle,
                b.sense,
            ),
            1.0,
        ),
        (
            SurfaceKind::XtCone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            }
            | SurfaceKind::Cone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            },
            SurfaceKind::Plane { point, normal, .. },
        ) => (
            (point.to_array(), normal.to_array(), b.sense),
            (
                cp.to_array(),
                axis.to_array(),
                *radius,
                *sin_half_angle,
                *cos_half_angle,
                a.sense,
            ),
            -1.0,
        ),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only plane/XT-cone or profile-mapped plane/cone intersections",
            ));
        }
    };
    let plane_sense = plane
        .2
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ))?;
    let cone_sense = cone
        .5
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ))?;
    let plane_normal = normalize(scale(plane.1, plane_sense)).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid plane normal"),
    )?;
    let xt_axis = normalize(cone.1).ok_or(IntersectionEvaluationError::InvalidInput(
        "invalid XT cone axis",
    ))?;
    let axis = if reverse_xt_axis {
        scale(xt_axis, -1.0)
    } else {
        xt_axis
    };
    let tangent_half_angle = cone.3 / cone.4;
    let angle_length = cone.3.hypot(cone.4);
    if !plane
        .0
        .into_iter()
        .chain(cone.0)
        .chain([cone.2, cone.3, cone.4])
        .all(f64::is_finite)
        || cone.2 < 0.0
        || cone.4.abs() <= 1e-12
        || !angle_length.is_finite()
        || angle_length <= 1e-12
        || !tangent_half_angle.is_finite()
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid plane/XT-cone geometry",
        ));
    }
    let points = chart_points
        .iter()
        .map(|point| point.to_array())
        .collect::<Vec<_>>();
    if points.iter().flatten().any(|value| !value.is_finite())
        || start_points
            .iter()
            .chain(end_points)
            .any(|point| !point.to_array().into_iter().all(f64::is_finite))
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection chart or limit contains a nonfinite point",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(plane.0.iter())
        .chain(cone.0.iter())
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    let cone_section_radius =
        |point: [f64; 3]| cone.2 - dot(sub(point, cone.0), axis) * tangent_half_angle;
    let cone_residual = |point: [f64; 3]| {
        let relative = sub(point, cone.0);
        let radial = sub(relative, scale(axis, dot(relative, axis)));
        let radius = cone_section_radius(point);
        (norm(radial) - radius.abs()).abs()
    };
    let validate_limit = |kind: &Option<IntersectionLimitKind>,
                          limit_points: &[super::Vector3],
                          endpoint: [f64; 3]| {
        match kind {
            Some(IntersectionLimitKind::Limit)
                if limit_points.len() == 1
                    && norm(sub(limit_points[0].to_array(), endpoint)) <= tolerance =>
            {
                Ok(())
            }
            Some(IntersectionLimitKind::Terminator) if limit_points.len() == 2 => {
                let singularity = limit_points[0].to_array();
                let branch_point = limit_points[1].to_array();
                let relative = sub(singularity, cone.0);
                let radial = sub(relative, scale(axis, dot(relative, axis)));
                if norm(sub(branch_point, endpoint)) > tolerance
                    || norm(sub(singularity, branch_point)) <= tolerance
                    || norm(radial) > tolerance
                    || cone_section_radius(singularity).abs() > tolerance
                    || dot(sub(singularity, plane.0), plane_normal).abs() > tolerance
                {
                    return Err(IntersectionEvaluationError::InvalidInput(
                        "plane/XT-cone terminator must have a distinct apex and chart endpoint branch point",
                    ));
                }
                Ok(())
            }
            Some(IntersectionLimitKind::Limit | IntersectionLimitKind::Terminator) => {
                Err(IntersectionEvaluationError::InvalidInput(
                    "plane/XT-cone limit has the wrong number of points or mismatched endpoint",
                ))
            }
            _ => Err(IntersectionEvaluationError::Unsupported(
                "plane/XT-cone candidate requires L or T limits",
            )),
        }
    };
    validate_limit(start_limit_kind, start_points, points[0])?;
    let last_point = *points
        .last()
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart has no points",
        ))?;
    validate_limit(end_limit_kind, end_points, last_point)?;
    let tangent_context = PlaneXtConeTangentContext {
        cone_origin: cone.0,
        axis,
        xt_axis,
        radius: cone.2,
        tangent_half_angle,
        cone_sense,
        plane_normal,
        order,
        tolerance,
    };
    let tangent_at = |point| plane_xt_cone_station_tangent(point, tangent_context);
    for point in &points {
        let plane_residual = dot(sub(*point, plane.0), plane_normal).abs();
        let section_radius = cone_section_radius(*point);
        let cone_error = cone_residual(*point);
        if !plane_residual.is_finite()
            || !section_radius.is_finite()
            || !cone_error.is_finite()
            || plane_residual > tolerance
            || section_radius <= tolerance
            || cone_error > tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support surfaces",
            ));
        }
    }
    let tangents = points[..points.len() - 1]
        .iter()
        .map(|point| tangent_at(*point))
        .collect::<Result<Vec<_>, _>>()?;
    let parameters = plane_xt_cone_chart_parameters(
        &points,
        chart_parameters
            .base_parameter
            .filter(|value| value.is_finite())
            .ok_or(IntersectionEvaluationError::InvalidInput(
                "intersection chart base parameter is missing or nonfinite",
            ))?,
        chart_parameters
            .base_scale
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or(IntersectionEvaluationError::InvalidInput(
                "intersection chart base scale must be finite and positive",
            ))?,
        tolerance,
        &tangents,
    )?;
    let last_parameter = parameters
        .last()
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    if parameter < parameters[0] || parameter > *last_parameter {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = parameters.iter().position(|value| *value == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[index],
            tangent: tangent_at(points[index])?,
        });
    }
    let index = parameters
        .windows(2)
        .position(|window| window[0] < parameter && parameter < window[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - parameters[index]) / (parameters[index + 1] - parameters[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_point = add(points[index], scale(chord, fraction));
    let chord_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length plane/XT-cone chart chord",
    ))?;
    let cross_normals = cross(chord_normal, plane_normal);
    let line_direction =
        normalize(cross_normals).ok_or(IntersectionEvaluationError::Unsupported(
            "chart plane and plane support do not define a unique line",
        ))?;
    let q = dot(chord_normal, chord_point);
    let d = dot(plane_normal, plane.0);
    let denominator = dot(cross_normals, cross_normals);
    let line_point = scale(
        add(
            scale(cross(plane_normal, cross_normals), q),
            scale(cross(cross_normals, chord_normal), d),
        ),
        1.0 / denominator,
    );
    let relative = sub(line_point, cone.0);
    let v0 = -dot(relative, axis);
    let vd = -dot(line_direction, axis);
    let radial_at_origin = sub(relative, scale(axis, dot(relative, axis)));
    let radial_along_direction = sub(line_direction, scale(axis, dot(line_direction, axis)));
    let radius_at_origin = cone.2 + v0 * tangent_half_angle;
    let radius_along_direction = vd * tangent_half_angle;
    let aa = dot(radial_along_direction, radial_along_direction)
        - radius_along_direction * radius_along_direction;
    let bb = 2.0
        * (dot(radial_at_origin, radial_along_direction)
            - radius_at_origin * radius_along_direction);
    let cc = dot(radial_at_origin, radial_at_origin) - radius_at_origin * radius_at_origin;
    let aa_scale = dot(radial_along_direction, radial_along_direction).abs()
        + (radius_along_direction * radius_along_direction).abs();
    let bb_scale = 2.0
        * (dot(radial_at_origin, radial_along_direction).abs()
            + (radius_at_origin * radius_along_direction).abs());
    if ![aa, bb, cc, aa_scale, bb_scale]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "chart line does not define isolated plane/XT-cone roots",
        ));
    }
    let mut roots = Vec::with_capacity(2);
    if aa_scale == 0.0 || aa.abs() <= EPS_FACTOR * aa_scale {
        if bb_scale == 0.0 || bb.abs() <= EPS_FACTOR * bb_scale {
            return Err(IntersectionEvaluationError::Unsupported(
                "chart line does not define isolated plane/XT-cone roots",
            ));
        }
        roots.push(-cc / bb);
    } else {
        let discriminant = bb * bb - 4.0 * aa * cc;
        let discriminant_scale = bb.abs().mul_add(bb.abs(), (4.0 * aa * cc).abs());
        if !discriminant.is_finite() || !discriminant_scale.is_finite() {
            return Err(IntersectionEvaluationError::InvalidInput(
                "nonfinite plane/XT-cone quadratic",
            ));
        }
        if discriminant < -EPS_FACTOR * discriminant_scale {
            return Err(IntersectionEvaluationError::Unsupported(
                "chart line has no plane/XT-cone intersection",
            ));
        }
        let root = discriminant.max(0.0).sqrt();
        let Some(quadratic_roots) = stable_quadratic_roots(aa, bb, cc, root) else {
            return Err(IntersectionEvaluationError::InvalidInput(
                "nonfinite plane/XT-cone quadratic roots",
            ));
        };
        roots.push(quadratic_roots[0]);
        if root > 0.0 {
            roots.push(quadratic_roots[1]);
        }
    }
    let error = chart_parameters
        .chordal_error
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or(IntersectionEvaluationError::Unsupported(
            "plane/XT-cone branch requires finite nonnegative chart chordal error",
        ))?;
    let limit = error + tolerance;
    if !limit.is_finite() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "plane/XT-cone chart chordal error overflows coordinate tolerance",
        ));
    }
    let roots = roots
        .into_iter()
        .filter(|root| root.is_finite())
        .map(|root| add(line_point, scale(line_direction, root)))
        .filter(|candidate| {
            cone_section_radius(*candidate) > tolerance
                && cone_residual(*candidate) <= tolerance
                && norm(sub(*candidate, chord_point)) <= limit
        })
        .collect::<Vec<_>>();
    let point = match roots.as_slice() {
        [point] => *point,
        [] => {
            return Err(IntersectionEvaluationError::Unsupported(
                "chart section plane has no plane/XT-cone root within chordal error",
            ));
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "ambiguous plane/XT-cone chart-plane roots",
            ));
        }
    };
    if dot(sub(point, plane.0), plane_normal).abs() > tolerance
        || cone_residual(point) > tolerance
        || dot(sub(point, chord_point), chord_normal).abs() > tolerance
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "evaluated point failed section-plane or support-surface consistency check",
        ));
    }
    Ok(IntersectionCurveEvaluation {
        point,
        tangent: tangent_at(point)?,
    })
}

fn plane_xt_cone_inverse_chart_parameters(
    brep: &BrepModel,
    curve: &CurveKind,
    reverse_xt_axis: bool,
) -> Result<(Vec<[f64; 3]>, Vec<f64>), IntersectionEvaluationError> {
    let CurveKind::Intersection {
        chart_parameters, ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    let chart_data = plane_xt_cone_inverse_chart_data(brep, curve, reverse_xt_axis)?;
    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let scale = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let parameters = plane_xt_cone_chart_parameters(
        &chart_data.points,
        base,
        scale,
        chart_data.geometric_tolerance,
        &chart_data.tangents,
    )?;
    Ok((chart_data.points, parameters))
}

struct PlaneXtConeInverseChartData {
    points: Vec<[f64; 3]>,
    tangents: Vec<[f64; 3]>,
    geometric_tolerance: f64,
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the geometry calculation and its established parameter contract together."
)]
fn plane_xt_cone_inverse_chart_data(
    brep: &BrepModel,
    curve: &CurveKind,
    reverse_xt_axis: bool,
) -> Result<PlaneXtConeInverseChartData, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection must reference two distinct support surfaces",
        ));
    }
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    let (plane, cone, order) = match (&a.kind, &b.kind) {
        (
            SurfaceKind::Plane { point, normal, .. },
            SurfaceKind::XtCone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            }
            | SurfaceKind::Cone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            },
        ) => (
            (point.to_array(), normal.to_array(), a.sense),
            (
                cp.to_array(),
                axis.to_array(),
                *radius,
                *sin_half_angle,
                *cos_half_angle,
                b.sense,
            ),
            1.0,
        ),
        (
            SurfaceKind::XtCone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            }
            | SurfaceKind::Cone {
                point: cp,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                ..
            },
            SurfaceKind::Plane { point, normal, .. },
        ) => (
            (point.to_array(), normal.to_array(), b.sense),
            (
                cp.to_array(),
                axis.to_array(),
                *radius,
                *sin_half_angle,
                *cos_half_angle,
                a.sense,
            ),
            -1.0,
        ),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only plane/XT-cone or profile-mapped plane/cone intersections",
            ));
        }
    };
    let plane_normal = normalize(scale(
        plane.1,
        plane
            .2
            .multiplier()
            .ok_or(IntersectionEvaluationError::Unsupported(
                "intersection support has unknown normal sense",
            ))?,
    ))
    .ok_or(IntersectionEvaluationError::InvalidInput(
        "invalid plane normal",
    ))?;
    let cone_sense = cone
        .5
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ))?;
    let xt_axis = normalize(cone.1).ok_or(IntersectionEvaluationError::InvalidInput(
        "invalid cone axis",
    ))?;
    let tangent_half_angle = cone.3 / cone.4;
    if !plane
        .0
        .into_iter()
        .chain(cone.0)
        .chain([cone.2, cone.3, cone.4])
        .all(f64::is_finite)
        || cone.2 < 0.0
        || cone.4.abs() <= 1e-12
        || !tangent_half_angle.is_finite()
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid plane/XT-cone geometry",
        ));
    }
    let points = chart_points
        .iter()
        .map(|p| p.to_array())
        .collect::<Vec<_>>();
    if points.iter().flatten().any(|x| !x.is_finite()) {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart or limit contains a nonfinite point",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(plane.0.iter())
        .chain(cone.0.iter())
        .fold(1.0_f64, |m, x| m.max(x.abs()));
    let geometric_tolerance = EPS_FACTOR * coordinate_scale;
    let axis = if reverse_xt_axis {
        scale(xt_axis, -1.0)
    } else {
        xt_axis
    };
    let tangent_context = PlaneXtConeTangentContext {
        cone_origin: cone.0,
        axis,
        xt_axis,
        radius: cone.2,
        tangent_half_angle,
        cone_sense,
        plane_normal,
        order,
        tolerance: geometric_tolerance,
    };
    let mut tangents = Vec::with_capacity(points.len() - 1);
    for point in &points[..points.len() - 1] {
        let relative = sub(*point, cone.0);
        let radial = sub(relative, scale(axis, dot(relative, axis)));
        let cone_radius = cone.2 - dot(relative, axis) * tangent_half_angle;
        if dot(sub(*point, plane.0), plane_normal).abs() > geometric_tolerance
            || (norm(radial) - cone_radius.abs()).abs() > geometric_tolerance
            || cone_radius <= geometric_tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support surfaces",
            ));
        }
        tangents.push(plane_xt_cone_station_tangent(*point, tangent_context)?);
    }
    Ok(PlaneXtConeInverseChartData {
        points,
        tangents,
        geometric_tolerance,
    })
}

#[derive(Clone, Copy)]
struct PlaneXtConeTangentContext {
    cone_origin: [f64; 3],
    axis: [f64; 3],
    xt_axis: [f64; 3],
    radius: f64,
    tangent_half_angle: f64,
    cone_sense: f64,
    plane_normal: [f64; 3],
    order: f64,
    tolerance: f64,
}

fn plane_xt_cone_station_tangent(
    point: [f64; 3],
    context: PlaneXtConeTangentContext,
) -> Result<[f64; 3], IntersectionEvaluationError> {
    let PlaneXtConeTangentContext {
        cone_origin,
        axis,
        xt_axis,
        radius,
        tangent_half_angle,
        cone_sense,
        plane_normal,
        order,
        tolerance,
    } = context;
    let relative = sub(point, cone_origin);
    let v = -dot(relative, axis);
    let radial = sub(relative, scale(axis, dot(relative, axis)));
    let radial_length = norm(radial);
    let section_radius = radius + v * tangent_half_angle;
    if !radial_length.is_finite()
        || radial_length <= tolerance
        || !section_radius.is_finite()
        || section_radius.abs() <= tolerance
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "plane/XT-cone intersection is singular at a chart point",
        ));
    }
    let canonical_radial = scale(radial, section_radius.signum() / radial_length);
    let around = cross(xt_axis, canonical_radial);
    let natural = cross(
        scale(around, section_radius),
        add(
            scale(axis, -1.0),
            scale(canonical_radial, tangent_half_angle),
        ),
    );
    let cone_normal =
        normalize(scale(natural, cone_sense)).ok_or(IntersectionEvaluationError::Unsupported(
            "plane/XT-cone intersection is singular at a chart point",
        ))?;
    normalize(scale(cross(plane_normal, cone_normal), order)).ok_or(
        IntersectionEvaluationError::Unsupported(
            "plane/XT-cone intersection is singular at a chart point",
        ),
    )
}

fn plane_xt_cone_chart_parameters(
    points: &[[f64; 3]],
    base: f64,
    mut scale_at: f64,
    tolerance: f64,
    tangents: &[[f64; 3]],
) -> Result<Vec<f64>, IntersectionEvaluationError> {
    if !base.is_finite()
        || !scale_at.is_finite()
        || scale_at <= 0.0
        || points.len() < 2
        || tangents.len() + 1 != points.len()
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection chart parameter data",
        ));
    }
    let mut parameters = vec![base];
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length = norm(chord);
        if !length.is_finite() || length <= tolerance || dot(tangents[index], chord) <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed plane/XT-cone chart chord",
            ));
        }
        let next = parameters[index] + length * scale_at;
        if !next.is_finite() || next <= parameters[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        parameters.push(next);
        if index + 1 < points.len() - 1 {
            let previous = sub(points[index + 1], points[index]);
            let next_chord = sub(points[index + 2], points[index + 1]);
            let cos_b = dot(tangents[index + 1], previous) / norm(previous);
            let cos_a = dot(tangents[index + 1], next_chord) / norm(next_chord);
            if !cos_a.is_finite() || cos_a <= 0.0 || !cos_b.is_finite() || cos_b <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate plane/XT-cone chart tangent",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate plane/XT-cone chart parameter scale",
                ));
            }
        }
    }
    Ok(parameters)
}

/// Evaluates a regular XT cone/cylinder branch from the chart's section plane.
/// The cone chart is used only to seed the solve; returned points satisfy the
/// section plane and both support equations, and must lie within CHART's error.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
pub fn evaluate_cone_cylinder_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    evaluate_cone_cylinder_intersection_candidate_with_axis(brep, curve, parameter, false)
}

/// Evaluates a cone/cylinder branch with the mapped XT axial direction
/// reversed while retaining its original angular frame.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
pub fn evaluate_cone_cylinder_intersection_candidate_with_reversed_mapped_axis(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    evaluate_cone_cylinder_intersection_candidate_with_axis(brep, curve, parameter, true)
}

/// Recovers the unique chart parameter and qualified root point matching `point`.
#[allow(
    clippy::float_cmp,
    reason = "exact point and parameter equality select chart endpoints and deduplicate roots"
)]
pub fn inverse_cone_cylinder_intersection_parameter(
    brep: &BrepModel,
    curve: &CurveKind,
    point: [f64; 3],
    tolerance: f64,
) -> Result<(f64, [f64; 3]), IntersectionEvaluationError> {
    const MAX_CHART_POINTS: usize = 32;
    if !point.into_iter().all(f64::is_finite) || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "inverse intersection point and tolerance must be finite, with positive tolerance",
        ));
    }
    let CurveKind::Intersection { chart_points, .. } = curve else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if chart_points.len() < 2 || chart_points.len() > MAX_CHART_POINTS {
        return Err(IntersectionEvaluationError::Unsupported(
            "cone/cylinder inverse requires a bounded chart of 2 to 32 points",
        ));
    }
    let mapped = cone_cylinder_inverse_chart_parameters(brep, curve, false);
    let reversed = cone_cylinder_inverse_chart_parameters(brep, curve, true);
    let (points, parameters, reverse_axis) = match (mapped, reversed) {
        (Ok((points, parameters)), Err(_)) => (points, parameters, false),
        (Err(_), Ok((points, parameters))) => (points, parameters, true),
        (Ok(_), Ok(_)) => {
            return Err(IntersectionEvaluationError::Unsupported(
                "both cone/cylinder axis conventions validate for inverse parameterization",
            ));
        }
        (Err(mapped), Err(reversed)) if mapped == reversed => return Err(mapped),
        (Err(_), Err(_)) => {
            return Err(IntersectionEvaluationError::Unsupported(
                "neither cone/cylinder axis convention validates for inverse parameterization",
            ));
        }
    };
    let mut matches = Vec::new();
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length2 = dot(chord, chord);
        if !length2.is_finite() || length2 <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate cone/cylinder chart chord",
            ));
        }
        let fraction = if point == points[index] {
            0.0
        } else if point == points[index + 1] {
            1.0
        } else {
            dot(sub(point, points[index]), chord) / length2
        };
        if !fraction.is_finite() {
            continue;
        }
        let fraction = if index == 0 && fraction < 0.0 {
            0.0
        } else if index + 2 == points.len() && fraction > 1.0 {
            1.0
        } else if (0.0..=1.0).contains(&fraction) {
            fraction
        } else {
            continue;
        };
        let parameter = chart_parameter_at_fraction(&parameters, index, fraction);
        for candidate in
            cone_cylinder_intersection_roots_with_axis(brep, curve, parameter, reverse_axis)?
        {
            if norm(sub(candidate.point, point)) <= tolerance {
                matches.push((parameter, candidate.point));
            }
        }
    }
    matches.sort_by(|a, b| a.0.total_cmp(&b.0));
    matches.dedup_by(|a, b| a.0 == b.0 && norm(sub(a.1, b.1)) <= tolerance);
    match matches.as_slice() {
        [(parameter, matched_point)] => Ok((*parameter, *matched_point)),
        [] => Err(IntersectionEvaluationError::Unsupported(
            "no chart section uniquely matches the requested endpoint within tolerance",
        )),
        _ => Err(IntersectionEvaluationError::Unsupported(
            "multiple chart sections match the requested endpoint within tolerance",
        )),
    }
}

/// Resolves an ambiguous opt-in sample against its CHART and adaptive trim chords.
/// Generic forward evaluation stays strict; this bounded selection is not a global proof.
pub(super) fn evaluate_cone_cylinder_intersection_sample_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
    trim_chord_point: [f64; 3],
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let regular_cone_cylinder = match curve {
        CurveKind::Intersection { surfaces, .. } if surfaces[0] != surfaces[1] => {
            let first = unique_surface(brep, surfaces[0])
                .ok()
                .map(|surface| &surface.kind);
            let second = unique_surface(brep, surfaces[1])
                .ok()
                .map(|surface| &surface.kind);
            matches!(
                (first, second),
                (
                    Some(SurfaceKind::Cone { .. }),
                    Some(SurfaceKind::Cylinder { .. })
                ) | (
                    Some(SurfaceKind::Cylinder { .. }),
                    Some(SurfaceKind::Cone { .. })
                )
            )
        }
        _ => false,
    };
    match evaluate_intersection_candidate(brep, curve, parameter) {
        Ok(evaluation) => return Ok(evaluation),
        Err(IntersectionEvaluationError::Unsupported(
            "neither cone/cylinder axis convention validates",
        )) => {}
        Err(IntersectionEvaluationError::Unsupported(
            "ambiguous cone/cylinder chart-plane roots",
        )) if regular_cone_cylinder => {}
        Err(error) => return Err(error),
    }
    let CurveKind::Intersection { chart_points, .. } = curve else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || !(2..=32).contains(&chart_points.len()) {
        return Err(IntersectionEvaluationError::Unsupported(
            "cone/cylinder sample requires a finite parameter and 2 to 32 chart points",
        ));
    }
    let (points, parameters, reverse_axis) = if regular_cone_cylinder {
        let (points, parameters) = cone_cylinder_inverse_chart_parameters(brep, curve, false)?;
        (points, parameters, false)
    } else {
        let mapped = cone_cylinder_inverse_chart_parameters(brep, curve, false);
        let reversed = cone_cylinder_inverse_chart_parameters(brep, curve, true);
        match (mapped, reversed) {
            (Ok((points, parameters)), Err(_)) => (points, parameters, false),
            (Err(_), Ok((points, parameters))) => (points, parameters, true),
            _ => {
                return Err(IntersectionEvaluationError::Unsupported(
                    "cone/cylinder sample requires one validated axis convention",
                ));
            }
        }
    };
    let index = parameters
        .windows(2)
        .position(|window| window[0] < parameter && parameter < window[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - parameters[index]) / (parameters[index + 1] - parameters[index]);
    let chord_point = add(
        points[index],
        scale(sub(points[index + 1], points[index]), fraction),
    );
    let chart_direction = normalize(sub(points[index + 1], points[index])).ok_or(
        IntersectionEvaluationError::InvalidInput("degenerate cone/cylinder chart chord"),
    )?;
    let roots = cone_cylinder_intersection_roots_with_axis(brep, curve, parameter, reverse_axis)?;
    select_cone_cylinder_chart_root(&roots, chord_point, trim_chord_point, chart_direction)
}

fn select_cone_cylinder_chart_root(
    roots: &[IntersectionCurveEvaluation],
    chord_point: [f64; 3],
    trim_chord_point: [f64; 3],
    chart_direction: [f64; 3],
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    if roots.is_empty() {
        return Err(IntersectionEvaluationError::Unsupported(
            "chart section has no cone/cylinder root within chordal error",
        ));
    }
    let scale = chord_point
        .into_iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let closest_chart_distance = roots
        .iter()
        .map(|root| norm(sub(root.point, chord_point)))
        .fold(f64::INFINITY, f64::min);
    let chart_tied_roots = roots
        .iter()
        .filter(|root| {
            norm(sub(root.point, chord_point)) - closest_chart_distance <= EPS_FACTOR * scale
        })
        .collect::<Vec<_>>();
    if chart_tied_roots.len() == 1 {
        return Ok(*chart_tied_roots[0]);
    }
    let mut trim_ranked = chart_tied_roots
        .into_iter()
        .map(|root| (norm(sub(root.point, trim_chord_point)), root))
        .collect::<Vec<_>>();
    trim_ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (best_distance, best) = trim_ranked
        .first()
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    if trim_ranked
        .get(1)
        .is_some_and(|(distance, _)| distance - *best_distance <= EPS_FACTOR * scale)
    {
        let mut tangent_ranked = trim_ranked
            .iter()
            .map(|(_, root)| (dot(root.tangent, chart_direction), *root))
            .filter(|(score, _)| *score > 0.0)
            .collect::<Vec<_>>();
        tangent_ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let Some((best_score, best)) = tangent_ranked.first() else {
            return Err(IntersectionEvaluationError::Unsupported(
                "no tied cone/cylinder root follows the ordered chart chord",
            ));
        };
        if tangent_ranked
            .get(1)
            .is_some_and(|(score, _)| *best_score - *score <= EPS_FACTOR)
        {
            return Err(IntersectionEvaluationError::Unsupported(
                "cone/cylinder roots remain tied by ordered chart tangent",
            ));
        }
        return Ok(**best);
    }
    Ok(**best)
}

fn cone_cylinder_inverse_chart_parameters(
    brep: &BrepModel,
    curve: &CurveKind,
    reverse_axis: bool,
) -> Result<(Vec<[f64; 3]>, Vec<f64>), IntersectionEvaluationError> {
    let CurveKind::Intersection {
        chart_points,
        chart_parameters,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    let points = chart_points
        .iter()
        .map(|point| point.to_array())
        .collect::<Vec<_>>();
    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let initial_scale = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let coordinate_scale = points
        .iter()
        .flatten()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    let parameters =
        cone_cylinder_chart_parameters(&points, base, initial_scale, tolerance, |_, parameter| {
            Ok(evaluate_cone_cylinder_intersection_candidate_with_axis(
                brep,
                curve,
                parameter,
                reverse_axis,
            )?
            .tangent)
        })?;
    Ok((points, parameters))
}

fn cone_cylinder_chart_parameters(
    points: &[[f64; 3]],
    base: f64,
    mut scale_at: f64,
    tolerance: f64,
    mut tangent_at: impl FnMut(usize, f64) -> Result<[f64; 3], IntersectionEvaluationError>,
) -> Result<Vec<f64>, IntersectionEvaluationError> {
    if !base.is_finite() || !scale_at.is_finite() || scale_at <= 0.0 || points.len() < 2 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection chart parameter data",
        ));
    }
    let mut parameters = vec![base];
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length = norm(chord);
        if !length.is_finite()
            || length <= tolerance
            || dot(tangent_at(index, parameters[index])?, chord) <= 0.0
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed cone/cylinder chart chord",
            ));
        }
        let next = parameters[index] + length * scale_at;
        if !next.is_finite() || next <= parameters[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate cone/cylinder chart parameter range",
            ));
        }
        parameters.push(next);
        if index + 1 < points.len() - 1 {
            let tangent = tangent_at(index + 1, next)?;
            let previous = sub(points[index + 1], points[index]);
            let next_chord = sub(points[index + 2], points[index + 1]);
            let cos_b = dot(tangent, previous) / norm(previous);
            let cos_a = dot(tangent, next_chord) / norm(next_chord);
            if !cos_b.is_finite() || !cos_a.is_finite() || cos_b <= 0.0 || cos_a <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate cone/cylinder chart tangent",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate cone/cylinder chart parameter scale",
                ));
            }
        }
    }
    Ok(parameters)
}

fn evaluate_cone_cylinder_intersection_candidate_with_axis(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
    reverse_xt_axis: bool,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let roots =
        cone_cylinder_intersection_roots_with_axis(brep, curve, parameter, reverse_xt_axis)?;
    match roots.as_slice() {
        [root] => Ok(*root),
        [] => Err(IntersectionEvaluationError::Unsupported(
            "chart section plane has no cone/cylinder root within chordal error",
        )),
        _ => Err(IntersectionEvaluationError::Unsupported(
            "ambiguous cone/cylinder chart-plane roots",
        )),
    }
}

#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies a stored chart station"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The mapped cone/cylinder root search preserves its ordered chart and support validation flow."
)]
fn cone_cylinder_intersection_roots_with_axis(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
    reverse_xt_axis: bool,
) -> Result<Vec<IntersectionCurveEvaluation>, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_points,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 || surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or support references",
        ));
    }
    let Some(error) = chart_parameters
        .chordal_error
        .filter(|error| error.is_finite() && *error >= 0.0)
    else {
        return Err(IntersectionEvaluationError::Unsupported(
            "cone/cylinder branch requires finite nonnegative chart chordal error",
        ));
    };
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    let (cone, cylinder, cone_sense, cylinder_sense, order) = match (&a.kind, &b.kind) {
        (SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. }, SurfaceKind::Cylinder { .. }) => {
            (&a.kind, &b.kind, a.sense, b.sense, 1.0)
        }
        (SurfaceKind::Cylinder { .. }, SurfaceKind::Cone { .. } | SurfaceKind::XtCone { .. }) => {
            (&b.kind, &a.kind, b.sense, a.sense, -1.0)
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only XT cone/cylinder intersections",
            ));
        }
    };
    let (cone_origin, cone_axis, cone_radius, sin_half_angle, cos_half_angle, x_axis, xt_profile) =
        match cone {
            SurfaceKind::Cone {
                point,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                x_axis,
            } => (
                point,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                x_axis,
                false,
            ),
            SurfaceKind::XtCone {
                point,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                x_axis,
            } => (
                point,
                axis,
                radius,
                sin_half_angle,
                cos_half_angle,
                x_axis,
                true,
            ),
            _ => {
                return Err(IntersectionEvaluationError::Unsupported(
                    "candidate supports only XT cone/cylinder intersections",
                ));
            }
        };
    let SurfaceKind::Cylinder {
        point: cylinder_origin,
        axis: cylinder_axis,
        radius: cylinder_radius,
        ..
    } = cylinder
    else {
        return Err(IntersectionEvaluationError::Unsupported(
            "candidate supports only XT cone/cylinder intersections",
        ));
    };
    let origin = cone_origin.to_array();
    let stored_axis = normalize(cone_axis.to_array()).ok_or(
        IntersectionEvaluationError::InvalidInput(if xt_profile {
            "invalid XT cone axis"
        } else {
            "invalid cone axis"
        }),
    )?;
    if reverse_xt_axis && !xt_profile {
        return Err(IntersectionEvaluationError::Unsupported(
            "regular cone has no reversed XT axis convention",
        ));
    }
    let axis = if reverse_xt_axis {
        scale(stored_axis, -1.0)
    } else {
        stored_axis
    };
    let station_axis_sign = if xt_profile { -1.0 } else { 1.0 };
    let cylinder_origin = cylinder_origin.to_array();
    let cylinder_axis = normalize(cylinder_axis.to_array()).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid cylinder axis"),
    )?;
    let tangent_half_angle = sin_half_angle / cos_half_angle;
    let angle_length = sin_half_angle.hypot(*cos_half_angle);
    if !origin
        .into_iter()
        .chain(cylinder_origin)
        .chain([
            *cone_radius,
            *sin_half_angle,
            *cos_half_angle,
            *cylinder_radius,
        ])
        .all(f64::is_finite)
        || *cone_radius < 0.0
        || *cylinder_radius <= 0.0
        || cos_half_angle.abs() <= 1e-12
        || !angle_length.is_finite()
        || angle_length <= 1e-12
        || !tangent_half_angle.is_finite()
    {
        return Err(IntersectionEvaluationError::InvalidInput(if xt_profile {
            "invalid XT cone/cylinder geometry"
        } else {
            "invalid cone/cylinder geometry"
        }));
    }
    let cone_x = normalize(sub(
        x_axis.to_array(),
        scale(axis, dot(x_axis.to_array(), axis)),
    ))
    .ok_or(IntersectionEvaluationError::InvalidInput(if xt_profile {
        "invalid XT cone reference direction"
    } else {
        "invalid cone reference direction"
    }))?;
    let cone_y = cross(stored_axis, cone_x);
    let Some(axis_correction) = cone_sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let Some(cylinder_correction) = cylinder_sense.multiplier() else {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection support has unknown normal sense",
        ));
    };
    let cone_point = |u: f64, v: f64| {
        let radial = add(scale(cone_x, u.cos()), scale(cone_y, u.sin()));
        let section_radius = cone_radius + v * tangent_half_angle;
        add(
            add(origin, scale(axis, station_axis_sign * v)),
            scale(radial, section_radius),
        )
    };
    let cone_partials = |u: f64, v: f64| {
        let radial = add(scale(cone_x, u.cos()), scale(cone_y, u.sin()));
        let around = add(scale(cone_x, -u.sin()), scale(cone_y, u.cos()));
        let section_radius = cone_radius + v * tangent_half_angle;
        (
            scale(around, section_radius),
            add(
                scale(axis, station_axis_sign),
                scale(radial, tangent_half_angle),
            ),
            radial,
            section_radius,
        )
    };
    let cone_natural = |point: [f64; 3]| -> Result<[f64; 3], IntersectionEvaluationError> {
        let relative = sub(point, origin);
        let v = station_axis_sign * dot(relative, axis);
        let radial_vector = sub(relative, scale(axis, dot(relative, axis)));
        let radial_length = norm(radial_vector);
        let section_radius = cone_radius + v * tangent_half_angle;
        let canonical_radial = if radial_length > 0.0 && section_radius.abs() > 0.0 {
            scale(radial_vector, section_radius.signum() / radial_length)
        } else {
            return Err(IntersectionEvaluationError::Unsupported(
                "cone/cylinder intersection is singular at a chart point",
            ));
        };
        let u = dot(canonical_radial, cone_y).atan2(dot(canonical_radial, cone_x));
        let around = add(scale(cone_x, -u.sin()), scale(cone_y, u.cos()));
        let du = scale(around, section_radius);
        let dv = add(
            scale(axis, station_axis_sign),
            scale(canonical_radial, tangent_half_angle),
        );
        normalize(cross(du, dv)).ok_or(IntersectionEvaluationError::Unsupported(
            "cone/cylinder intersection is singular at a chart point",
        ))
    };
    let cylinder_normal = |point: [f64; 3]| {
        let offset = sub(point, cylinder_origin);
        let radial = sub(offset, scale(cylinder_axis, dot(offset, cylinder_axis)));
        normalize(radial)
    };
    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    let Some(&last_point) = points.last() else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart is empty",
        ));
    };
    if points.iter().flatten().any(|value| !value.is_finite())
        || start_points.len() != 1
        || end_points.len() != 1
        || !near(start_points[0].to_array(), points[0])
        || !near(end_points[0].to_array(), last_point)
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection requires finite chart points and single-point limits matching chart endpoints",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(origin.iter())
        .chain(cylinder_origin.iter())
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = EPS_FACTOR * coordinate_scale;
    let uv_at = |point: [f64; 3]| -> Result<(f64, f64), IntersectionEvaluationError> {
        let relative = sub(point, origin);
        let v = station_axis_sign * dot(relative, axis);
        let radial = sub(relative, scale(axis, dot(relative, axis)));
        let section_radius = cone_radius + v * tangent_half_angle;
        let expected_radius = section_radius.abs();
        if (norm(radial) - expected_radius).abs() > tolerance || expected_radius <= tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(if xt_profile {
                "intersection chart point is not on the XT cone sheet"
            } else {
                "intersection chart point is not on the cone sheet"
            }));
        }
        let radial = scale(radial, section_radius.signum());
        Ok((dot(radial, cone_y).atan2(dot(radial, cone_x)), v))
    };
    let normals_at =
        |point: [f64; 3]| -> Result<([f64; 3], [f64; 3]), IntersectionEvaluationError> {
            let cn = cone_natural(point)?;
            let yn = cylinder_normal(point).ok_or(IntersectionEvaluationError::InvalidInput(
                "invalid cylinder radial at chart point",
            ))?;
            Ok((scale(cn, axis_correction), scale(yn, cylinder_correction)))
        };
    let tangent_at = |point: [f64; 3]| -> Result<[f64; 3], IntersectionEvaluationError> {
        let (cn, yn) = normals_at(point)?;
        normalize(scale(cross(cn, yn), order)).ok_or(IntersectionEvaluationError::Unsupported(
            "cone/cylinder intersection is singular at a chart point",
        ))
    };
    for point in &points {
        let cone_uv = uv_at(*point)?;
        let cone_eval = cone_point(cone_uv.0, cone_uv.1);
        let cyl_offset = sub(*point, cylinder_origin);
        let cyl_radial = sub(
            cyl_offset,
            scale(cylinder_axis, dot(cyl_offset, cylinder_axis)),
        );
        let cone_residual = norm(sub(cone_eval, *point));
        let cylinder_residual = (norm(cyl_radial) - cylinder_radius).abs();
        if !cone_residual.is_finite()
            || !cylinder_residual.is_finite()
            || cone_residual > tolerance
            || cylinder_residual > tolerance
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support surfaces",
            ));
        }
    }
    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let mut ts = vec![base];
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let length = norm(chord);
        if !length.is_finite()
            || length <= tolerance
            || dot(tangent_at(points[index])?, chord) <= 0.0
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed cone/cylinder chart chord",
            ));
        }
        let next_parameter = ts[index] + length * scale_at;
        if !next_parameter.is_finite() || next_parameter <= ts[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "degenerate cone/cylinder chart parameter range",
            ));
        }
        ts.push(next_parameter);
        if index + 1 < points.len() - 1 {
            let tangent = tangent_at(points[index + 1])?;
            let previous = sub(points[index + 1], points[index]);
            let next = sub(points[index + 2], points[index + 1]);
            let cos_b = dot(tangent, previous) / norm(previous);
            let cos_a = dot(tangent, next) / norm(next);
            if !cos_a.is_finite() || !cos_b.is_finite() || cos_a <= 0.0 || cos_b <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate cone/cylinder chart tangent",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "degenerate cone/cylinder chart parameter scale",
                ));
            }
        }
    }
    if parameter < ts[0]
        || parameter
            > *ts.last().ok_or(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is empty",
            ))?
    {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = ts.iter().position(|value| *value == parameter) {
        return Ok(vec![IntersectionCurveEvaluation {
            point: points[index],
            tangent: tangent_at(points[index])?,
        }]);
    }
    let index = ts
        .windows(2)
        .position(|window| window[0] < parameter && parameter < window[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - ts[index]) / (ts[index + 1] - ts[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_point = add(points[index], scale(chord, fraction));
    let section_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length cone/cylinder chart chord",
    ))?;
    let uv_a = uv_at(points[index])?;
    let uv_b = uv_at(points[index + 1])?;
    let mut du = uv_b.0 - uv_a.0;
    if du > std::f64::consts::PI {
        du -= std::f64::consts::TAU;
    } else if du < -std::f64::consts::PI {
        du += std::f64::consts::TAU;
    }
    let seed = (
        uv_a.0 + fraction * du,
        uv_a.1 + fraction * (uv_b.1 - uv_a.1),
    );
    let solve = |mut uv: (f64, f64)| -> Option<[f64; 3]> {
        for _ in 0..40 {
            let point = cone_point(uv.0, uv.1);
            let (u_tangent, v_tangent, _, _) = cone_partials(uv.0, uv.1);
            let cylinder_offset = sub(point, cylinder_origin);
            let cylinder_radial = sub(
                cylinder_offset,
                scale(cylinder_axis, dot(cylinder_offset, cylinder_axis)),
            );
            let f1 = dot(sub(point, chord_point), section_normal);
            let f2 = dot(cylinder_radial, cylinder_radial) - cylinder_radius * cylinder_radius;
            let j11 = dot(u_tangent, section_normal);
            let j12 = dot(v_tangent, section_normal);
            let first_parameter_radial_component = sub(
                u_tangent,
                scale(cylinder_axis, dot(u_tangent, cylinder_axis)),
            );
            let second_parameter_radial_component = sub(
                v_tangent,
                scale(cylinder_axis, dot(v_tangent, cylinder_axis)),
            );
            let j21 = 2.0 * dot(cylinder_radial, first_parameter_radial_component);
            let j22 = 2.0 * dot(cylinder_radial, second_parameter_radial_component);
            let determinant = j11 * j22 - j12 * j21;
            let row1_norm = norm([j11, j12, 0.0]);
            let row2_norm = norm([j21, j22, 0.0]);
            let determinant_scale = row1_norm * row2_norm;
            if ![f1, f2, determinant, determinant_scale]
                .into_iter()
                .all(f64::is_finite)
                || determinant_scale == 0.0
                || determinant.abs() <= EPS_FACTOR * determinant_scale
            {
                return None;
            }
            if f1.abs() <= tolerance && (norm(cylinder_radial) - cylinder_radius).abs() <= tolerance
            {
                return point.into_iter().all(f64::is_finite).then_some(point);
            }
            uv.0 -= (f1 * j22 - f2 * j12) / determinant;
            uv.1 -= (j11 * f2 - j21 * f1) / determinant;
            if !uv.0.is_finite() || !uv.1.is_finite() {
                return None;
            }
        }
        None
    };
    let mut seeds = vec![seed, (seed.0 + std::f64::consts::PI, seed.1)];
    for point in &points {
        let uv = uv_at(*point)?;
        seeds.extend([uv, (uv.0 + std::f64::consts::PI, uv.1)]);
    }
    let mut roots = Vec::<[f64; 3]>::new();
    // Root uniqueness covers distinct support-valid roots from these bounded seeds only; use certified isolation for full branch coverage.
    // shortcut: Newton multi-start can miss disconnected roots; use certified root isolation when full branch coverage is needed.
    for seed in seeds {
        if let Some(root) = solve(seed) {
            if !roots.iter().any(|other| near(*other, root)) {
                roots.push(root);
            }
        }
    }
    let roots: Vec<_> = roots
        .into_iter()
        .filter(|point| norm(sub(*point, chord_point)) <= error + tolerance)
        .collect();
    let mut evaluations = Vec::new();
    for point in roots {
        let cone_uv = uv_at(point)?;
        let cone_residual = norm(sub(cone_point(cone_uv.0, cone_uv.1), point));
        let cylinder_offset = sub(point, cylinder_origin);
        let cylinder_radial = sub(
            cylinder_offset,
            scale(cylinder_axis, dot(cylinder_offset, cylinder_axis)),
        );
        let section_residual = dot(sub(point, chord_point), section_normal).abs();
        if !cone_residual.is_finite()
            || !section_residual.is_finite()
            || !norm(cylinder_radial).is_finite()
            || cone_residual > tolerance
            || section_residual > tolerance
            || (norm(cylinder_radial) - cylinder_radius).abs() > tolerance
        {
            continue;
        }
        evaluations.push(IntersectionCurveEvaluation {
            point,
            tangent: tangent_at(point)?,
        });
    }
    Ok(evaluations)
}

/// Evaluates a cylinder/cylinder intersection branch from XT chart chords.
///
/// Interior points solve both cylinder equations and the plane through the
/// interpolated chart chord point normal to that chord. Three nearby Newton
/// seeds must converge to one locally consistent root; this candidate does
/// not prove global root or native-kernel uniqueness. Terminator limits are
/// unsupported because they carry a second branch point whose selection
/// semantics are not established here.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies a stored chart station and endpoint locator"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The evaluator preserves coupled chart-root selection and geometric validation order."
)]
pub fn evaluate_cylinder_cylinder_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_limit_kind,
        start_points,
        end_limit_kind,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an X_T intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 || surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or support references",
        ));
    }
    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    let (
        SurfaceKind::Cylinder {
            point: origin_a,
            axis: axis_a,
            radius: radius_a,
            ..
        },
        SurfaceKind::Cylinder {
            point: origin_b,
            axis: axis_b,
            radius: radius_b,
            ..
        },
    ) = (&a.kind, &b.kind)
    else {
        return Err(IntersectionEvaluationError::Unsupported(
            "candidate supports only cylinder/cylinder intersections",
        ));
    };
    let (origin_a, origin_b) = (origin_a.to_array(), origin_b.to_array());
    let (Some(axis_a), Some(axis_b)) = (normalize(axis_a.to_array()), normalize(axis_b.to_array()))
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid cylinder axis",
        ));
    };
    if !origin_a
        .into_iter()
        .chain(origin_b)
        .chain([*radius_a, *radius_b])
        .all(f64::is_finite)
        || *radius_a <= 0.0
        || *radius_b <= 0.0
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid cylinder geometry",
        ));
    }

    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    let (Some(&first_point), Some(&last_point)) = (points.first(), points.last()) else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart is empty",
        ));
    };
    if points.iter().flatten().any(|value| !value.is_finite())
        || start_points.len() != 1
        || end_points.len() != 1
        || start_points
            .iter()
            .chain(end_points)
            .any(|point| !point.to_array().into_iter().all(f64::is_finite))
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection requires finite chart points and single-point limits matching chart endpoints",
        ));
    }
    let coordinate_scale = points
        .iter()
        .flatten()
        .chain(origin_a.iter())
        .chain(origin_b.iter())
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    let local_scale = radius_a.max(*radius_b);
    let tolerance = 4.0 * EPS_FACTOR * local_scale;
    if f64::EPSILON * coordinate_scale > tolerance {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/cylinder coordinates exceed the supported local precision",
        ));
    }
    let start_help = *start_limit_kind == Some(IntersectionLimitKind::Help);
    let end_help = *end_limit_kind == Some(IntersectionLimitKind::Help);
    if start_help || end_help {
        let start_locator = start_points[0].to_array();
        let end_locator = end_points[0].to_array();
        if !start_help
            || !end_help
            || start_locator != end_locator
            || norm(sub(start_locator, first_point)) > tolerance
            || norm(sub(end_locator, last_point)) > tolerance
            || norm(sub(first_point, last_point)) > tolerance
        {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder help limits must identify the same closed chart endpoint",
            ));
        }
    } else if *start_limit_kind != Some(IntersectionLimitKind::Limit)
        || *end_limit_kind != Some(IntersectionLimitKind::Limit)
        || norm(sub(start_points[0].to_array(), first_point)) > tolerance
        || norm(sub(end_points[0].to_array(), last_point)) > tolerance
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "intersection limits do not match the chart endpoints within local precision",
        ));
    }
    let radial_normal = |origin: [f64; 3], axis: [f64; 3], point: [f64; 3]| {
        let offset = sub(point, origin);
        let radial = sub(offset, scale(axis, dot(offset, axis)));
        normalize(radial).map(|normal| (normal, norm(radial)))
    };
    let geometric_tangent = |point: [f64; 3]| {
        let (normal_a, _) = radial_normal(origin_a, axis_a, point)?;
        let (normal_b, _) = radial_normal(origin_b, axis_b, point)?;
        normalize(cross(normal_a, normal_b))
    };
    let mut tangents = Vec::with_capacity(points.len());
    for index in 0..points.len() {
        let tangent =
            geometric_tangent(points[index]).ok_or(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder intersection is singular at a chart point",
            ))?;
        let incoming = if index > 0 {
            normalize(sub(points[index], points[index - 1]))
        } else {
            None
        };
        let outgoing = if index + 1 < points.len() {
            normalize(sub(points[index + 1], points[index]))
        } else {
            None
        };
        let direction = outgoing
            .or(incoming)
            .ok_or(IntersectionEvaluationError::InvalidInput(
                "degenerate intersection chart chord",
            ))?;
        let alignment = dot(tangent, direction);
        if !alignment.is_finite() || alignment.abs() <= 1e-8 {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder chart tangent is not aligned with its ordered chord",
            ));
        }
        tangents.push(if alignment > 0.0 {
            tangent
        } else {
            scale(tangent, -1.0)
        });
    }

    for point in &points {
        let (_, radial_a) = radial_normal(origin_a, axis_a, *point).ok_or(
            IntersectionEvaluationError::InvalidInput("invalid first cylinder radial"),
        )?;
        let (_, radial_b) = radial_normal(origin_b, axis_b, *point).ok_or(
            IntersectionEvaluationError::InvalidInput("invalid second cylinder radial"),
        )?;
        if (radial_a - radius_a).abs() > tolerance || (radial_b - radius_b).abs() > tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support cylinders",
            ));
        }
    }

    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let mut stations = vec![base];
    for index in 0..points.len() - 1 {
        let previous = sub(points[index + 1], points[index]);
        let previous_length = norm(previous);
        if !previous_length.is_finite() || previous_length <= tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart contains a degenerate chord",
            ));
        }
        if dot(tangents[index], previous) <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "chart order disagrees with its cylinder intersection tangent",
            ));
        }
        let next_station = stations[index] + previous_length * scale_at;
        if !next_station.is_finite() || next_station <= stations[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        stations.push(next_station);
        if index + 1 < points.len() - 1 {
            let next = sub(points[index + 2], points[index + 1]);
            let next_length = norm(next);
            let cos_b = dot(tangents[index + 1], previous) / previous_length;
            let cos_a = dot(tangents[index + 1], next) / next_length;
            if !next_length.is_finite()
                || next_length <= tolerance
                || !cos_a.is_finite()
                || cos_a <= 1e-8
                || !cos_b.is_finite()
                || cos_b <= 1e-8
            {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "intersection chart tangent recurrence is degenerate",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "intersection chart parameter scale is degenerate",
                ));
            }
        }
    }
    let last_station = stations
        .last()
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    if parameter < stations[0] || parameter > *last_station {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = stations.iter().position(|station| *station == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[index],
            tangent: tangents[index],
        });
    }
    let index = stations
        .windows(2)
        .position(|pair| pair[0] < parameter && parameter < pair[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - stations[index]) / (stations[index + 1] - stations[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_length = norm(chord);
    let chord_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length chart chord",
    ))?;
    let chord_point = add(points[index], scale(chord, fraction));
    let expected_tangent = normalize(add(
        scale(tangents[index], 1.0 - fraction),
        scale(tangents[index + 1], fraction),
    ))
    .ok_or(IntersectionEvaluationError::Unsupported(
        "interpolated cylinder intersection tangent is degenerate",
    ))?;
    let across = normalize(cross(chord_normal, expected_tangent)).ok_or(
        IntersectionEvaluationError::Unsupported(
            "chart plane and expected tangent do not define a seed direction",
        ),
    )?;
    let radius_scale = radius_a.max(*radius_b).max(chord_length);
    let seed_offset = (radius_scale * 1e-4).max(tolerance * 8.0);
    let roots = [0.0, seed_offset, -seed_offset].map(|offset| {
        solve_cylinder_pair_chart_root(
            add(chord_point, scale(across, offset)),
            chord_point,
            chord_normal,
            CylinderPairChartSupports {
                origin_a,
                axis_a,
                radius_a: *radius_a,
                origin_b,
                axis_b,
                radius_b: *radius_b,
            },
        )
    });
    let [Ok(root), Ok(root_b), Ok(root_c)] = roots else {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/cylinder chart-plane solves did not all converge",
        ));
    };
    let seed_spread = norm(sub(root, root_b)).max(norm(sub(root, root_c)));
    if seed_spread > 1e-7 * local_scale {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/cylinder chart-plane seeds reached distinct local roots",
        ));
    }
    let mut tangent = geometric_tangent(root).ok_or(IntersectionEvaluationError::Unsupported(
        "cylinder/cylinder intersection is singular at the evaluated point",
    ))?;
    if dot(tangent, expected_tangent) < 0.0 {
        tangent = scale(tangent, -1.0);
    }
    if dot(tangent, expected_tangent) <= 0.0 {
        return Err(IntersectionEvaluationError::Unsupported(
            "solved cylinder root is on the wrong chart branch",
        ));
    }
    let residual_a = radial_normal(origin_a, axis_a, root)
        .map(|(_, radius)| (radius - radius_a).abs())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "evaluated point has invalid first cylinder radial",
        ))?;
    let residual_b = radial_normal(origin_b, axis_b, root)
        .map(|(_, radius)| (radius - radius_b).abs())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "evaluated point has invalid second cylinder radial",
        ))?;
    if residual_a > tolerance
        || residual_b > tolerance
        || dot(sub(root, chord_point), chord_normal).abs() > tolerance
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "evaluated point failed chart-plane or support-surface consistency check",
        ));
    }
    Ok(IntersectionCurveEvaluation {
        point: root,
        tangent,
    })
}

/// Evaluates a regular cylinder/torus branch from its XT chart.
///
/// The chart recurrence follows the XT reference. Interior points solve the
/// two analytic support equations and the plane through the interpolated chart
/// chord point normal to that chord, using bounded damped Newton iterations.
/// `chordal_error` bounds candidates against the chart chord and selects one
/// local branch. This is a bounded local candidate, not a proof of global or
/// kernel-level uniqueness.
/// Only finite single-point `L` limits are supported; terminator, help, and
/// boundary limits fail closed.
///
/// # Errors
/// Returns an error when the support pair, chart, parameter, or selected root is invalid.
#[allow(
    clippy::float_cmp,
    reason = "exact parameter equality identifies a stored chart station"
)]
#[allow(
    clippy::too_many_lines,
    reason = "The chart solve preserves the ordered branch selection and support validation flow."
)]
pub fn evaluate_cylinder_torus_intersection_candidate(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    let CurveKind::Intersection {
        surfaces,
        chart_parameters,
        chart_points,
        start_limit_kind,
        start_points,
        end_limit_kind,
        end_points,
        ..
    } = curve
    else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "curve is not an XT intersection",
        ));
    };
    if !parameter.is_finite() || chart_points.len() < 2 || surfaces[0] == surfaces[1] {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid intersection parameter, chart, or support references",
        ));
    }
    if *start_limit_kind != Some(IntersectionLimitKind::Limit)
        || *end_limit_kind != Some(IntersectionLimitKind::Limit)
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus candidate requires ordinary L limits",
        ));
    }
    if start_points.len() != 1 || end_points.len() != 1 {
        return Err(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus candidate requires one point per L limit",
        ));
    }

    let a = unique_surface(brep, surfaces[0])?;
    let b = unique_surface(brep, surfaces[1])?;
    let (cylinder, torus, cylinder_first) = match (&a.kind, &b.kind) {
        (SurfaceKind::Cylinder { .. }, SurfaceKind::Torus { .. }) => (a, b, true),
        (SurfaceKind::Torus { .. }, SurfaceKind::Cylinder { .. }) => (b, a, false),
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only cylinder/torus intersections",
            ));
        }
    };
    let cylinder_sense =
        cylinder
            .sense
            .multiplier()
            .ok_or(IntersectionEvaluationError::Unsupported(
                "cylinder/torus support has unknown normal sense",
            ))?;
    let torus_sense = torus
        .sense
        .multiplier()
        .ok_or(IntersectionEvaluationError::Unsupported(
            "cylinder/torus support has unknown normal sense",
        ))?;
    let SurfaceKind::Cylinder {
        point: cylinder_origin,
        axis: cylinder_axis,
        radius: cylinder_radius,
        ..
    } = &cylinder.kind
    else {
        unreachable!();
    };
    let SurfaceKind::Torus {
        center: torus_center,
        axis: torus_axis,
        major_radius,
        minor_radius,
        ..
    } = &torus.kind
    else {
        unreachable!();
    };
    let cylinder_origin = cylinder_origin.to_array();
    let torus_center = torus_center.to_array();
    let cylinder_axis = normalize(cylinder_axis.to_array()).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid cylinder axis"),
    )?;
    let torus_axis = normalize(torus_axis.to_array()).ok_or(
        IntersectionEvaluationError::InvalidInput("invalid torus axis"),
    )?;
    if !cylinder_origin
        .into_iter()
        .chain(torus_center)
        .chain([*cylinder_radius, *major_radius, *minor_radius])
        .all(f64::is_finite)
        || *cylinder_radius <= 0.0
        || *major_radius <= 0.0
        || *minor_radius <= 0.0
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid cylinder/torus dimensions",
        ));
    }
    let local_scale = (*cylinder_radius).max(*major_radius).max(*minor_radius);
    let coordinate_scale = chart_points
        .iter()
        .flat_map(|point| point.to_array())
        .chain(cylinder_origin)
        .chain(torus_center)
        .fold(0.0_f64, |maximum, value| maximum.max(value.abs()));
    let tolerance = EPS_FACTOR * local_scale;
    if f64::EPSILON * coordinate_scale > tolerance {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus coordinates exceed supported local precision",
        ));
    }
    let chordal_error = chart_parameters
        .chordal_error
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or(IntersectionEvaluationError::Unsupported(
            "cylinder/torus candidate requires finite nonnegative chordal error",
        ))?;
    let chordal_bound = chordal_error + tolerance;
    if !chordal_bound.is_finite() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chordal bound overflows",
        ));
    }

    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    if points
        .iter()
        .flatten()
        .copied()
        .chain(start_points.iter().flat_map(|point| point.to_array()))
        .chain(end_points.iter().flat_map(|point| point.to_array()))
        .any(|value| !value.is_finite())
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chart or limit contains a nonfinite point",
        ));
    }
    let last_point = *points
        .last()
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chart has no points",
        ))?;
    if norm(sub(start_points[0].to_array(), points[0])) > tolerance
        || norm(sub(end_points[0].to_array(), last_point)) > tolerance
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus L limits do not match chart endpoints",
        ));
    }

    let support_at = |point: [f64; 3]| {
        let cylinder_offset = sub(point, cylinder_origin);
        let cylinder_radial = sub(
            cylinder_offset,
            scale(cylinder_axis, dot(cylinder_offset, cylinder_axis)),
        );
        let cylinder_radial_length = norm(cylinder_radial);
        let cylinder_normal =
            normalize(cylinder_radial).ok_or(IntersectionEvaluationError::Unsupported(
                "cylinder/torus intersection is singular on the cylinder axis",
            ))?;
        let cylinder_residual = (cylinder_radial_length - *cylinder_radius) * cylinder_sense;

        let torus_offset = sub(point, torus_center);
        let torus_axial = dot(torus_offset, torus_axis);
        let torus_radial = sub(torus_offset, scale(torus_axis, torus_axial));
        let torus_radial_length = norm(torus_radial);
        if !torus_radial_length.is_finite() || torus_radial_length <= 0.0 {
            return Err(IntersectionEvaluationError::Unsupported(
                "torus normal is undefined on its axis",
            ));
        }
        let torus_distance = (torus_radial_length - *major_radius).hypot(torus_axial);
        let torus_residual = (torus_distance - *minor_radius) * torus_sense;
        let torus_normal = torus_normal_at_point(
            point,
            torus_center,
            torus_axis,
            *major_radius,
            *minor_radius,
        )?;
        let cylinder_normal = scale(cylinder_normal, cylinder_sense);
        let torus_normal = scale(torus_normal, torus_sense);
        let (residual_a, residual_b, normal_a, normal_b) = if cylinder_first {
            (
                cylinder_residual,
                torus_residual,
                cylinder_normal,
                torus_normal,
            )
        } else {
            (
                torus_residual,
                cylinder_residual,
                torus_normal,
                cylinder_normal,
            )
        };
        if !residual_a.is_finite() || !residual_b.is_finite() {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus support residual is nonfinite",
            ));
        }
        Ok((residual_a, residual_b, normal_a, normal_b))
    };

    let mut tangents = Vec::with_capacity(points.len());
    for (index, point) in points.iter().copied().enumerate() {
        let (_, _, normal_a, normal_b) = support_at(point)?;
        let natural = cross(normal_a, normal_b);
        let magnitude = norm(natural);
        if !magnitude.is_finite() || magnitude <= 1.0e-8 {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/torus intersection is singular at a chart point",
            ));
        }
        let tangent = scale(natural, 1.0 / magnitude);
        let chord_direction = if index + 1 < points.len() {
            sub(points[index + 1], point)
        } else {
            sub(point, points[index - 1])
        };
        let chord_direction = normalize(chord_direction).ok_or(
            IntersectionEvaluationError::InvalidInput("degenerate cylinder/torus chart chord"),
        )?;
        let alignment = dot(tangent, chord_direction);
        if !alignment.is_finite() || alignment.abs() <= 1.0e-8 {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/torus chart tangent is not aligned with chart order",
            ));
        }
        tangents.push(if alignment > 0.0 {
            tangent
        } else {
            scale(tangent, -1.0)
        });
    }
    for (index, point) in points.iter().copied().enumerate() {
        let (residual_a, residual_b, _, _) = support_at(point)?;
        if residual_a.abs() > tolerance || residual_b.abs() > tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus chart point is off its support surfaces",
            ));
        }
        if index + 1 < points.len() && dot(tangents[index], sub(points[index + 1], point)) <= 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus chart order disagrees with support tangent",
            ));
        }
    }
    if let Some(angular_error) = chart_parameters.angular_error {
        if !angular_error.is_finite() || angular_error < 0.0 {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus chart angular error is invalid",
            ));
        }
        for pair in tangents.windows(2) {
            let angle = dot(pair[0], pair[1]).clamp(-1.0, 1.0).acos();
            if angle > angular_error + 64.0 * f64::EPSILON {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "cylinder/torus tangent turn exceeds chart angular error",
                ));
            }
        }
    }

    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chart base parameter is missing or nonfinite",
        ))?;
    let mut scale_at = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chart base scale must be finite and positive",
        ))?;
    let mut stations = vec![base];
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let chord_length = norm(chord);
        if !chord_length.is_finite() || chord_length <= tolerance {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus chart contains a degenerate chord",
            ));
        }
        let next_station = stations[index] + chord_length * scale_at;
        if !next_station.is_finite() || next_station <= stations[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "cylinder/torus chart parameter range is degenerate",
            ));
        }
        stations.push(next_station);
        if index + 1 < points.len() - 1 {
            let tangent = tangents[index + 1];
            let previous = sub(points[index + 1], points[index]);
            let next = sub(points[index + 2], points[index + 1]);
            let next_length = norm(next);
            let cos_b = dot(tangent, previous) / chord_length;
            let cos_a = dot(tangent, next) / next_length;
            if !next_length.is_finite()
                || next_length <= tolerance
                || !cos_a.is_finite()
                || cos_a <= 1.0e-8
                || !cos_b.is_finite()
                || cos_b <= 1.0e-8
            {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "cylinder/torus chart tangent recurrence is degenerate",
                ));
            }
            scale_at *= cos_b / cos_a;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "cylinder/torus chart parameter scale is degenerate",
                ));
            }
        }
    }
    let chart_start = stations[0];
    let chart_end = *stations
        .last()
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let parameter_roundoff =
        16.0 * f64::EPSILON * parameter.abs().max(chart_start.abs()).max(chart_end.abs());
    let parameter = if parameter < chart_start {
        if chart_start - parameter <= parameter_roundoff {
            chart_start
        } else {
            return Err(IntersectionEvaluationError::OutsideChartDomain);
        }
    } else if parameter > chart_end {
        if parameter - chart_end <= parameter_roundoff {
            chart_end
        } else {
            return Err(IntersectionEvaluationError::OutsideChartDomain);
        }
    } else {
        parameter
    };
    if let Some(index) = stations.iter().position(|station| *station == parameter) {
        return Ok(IntersectionCurveEvaluation {
            point: points[index],
            tangent: tangents[index],
        });
    }
    let index = stations
        .windows(2)
        .position(|window| window[0] < parameter && parameter < window[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - stations[index]) / (stations[index + 1] - stations[index]);
    let chord = sub(points[index + 1], points[index]);
    let chord_normal = normalize(chord).ok_or(IntersectionEvaluationError::InvalidInput(
        "zero-length cylinder/torus chart chord",
    ))?;
    let chord_point = add(points[index], scale(chord, fraction));
    let predictor = chord_point;
    let (chart_residual_a, chart_residual_b, normal_a, normal_b) = support_at(points[index])?;
    if chart_residual_a.abs() > tolerance || chart_residual_b.abs() > tolerance {
        return Err(IntersectionEvaluationError::InvalidInput(
            "cylinder/torus chart anchor is off its support surfaces",
        ));
    }
    let seed_offset = (local_scale * 1.0e-4).min(chordal_error * 0.25);
    let seed_offsets = [
        [0.0; 3],
        scale(normal_a, seed_offset),
        scale(normal_a, -seed_offset),
        scale(normal_b, seed_offset),
        scale(normal_b, -seed_offset),
    ];
    let solve_root = |mut point: [f64; 3]| -> Result<[f64; 3], IntersectionEvaluationError> {
        let max_step = 0.25 * local_scale;
        for _ in 0..32 {
            let (residual_a, residual_b, normal_a, normal_b) = support_at(point)?;
            let projection_residual = dot(sub(point, chord_point), chord_normal);
            let residual_norm = residual_a
                .abs()
                .max(residual_b.abs())
                .max(projection_residual.abs());
            if residual_norm <= tolerance {
                return Ok(point);
            }
            let determinant = dot(normal_a, cross(normal_b, chord_normal));
            if !determinant.is_finite() || determinant.abs() <= 1.0e-8 {
                return Err(IntersectionEvaluationError::Unsupported(
                    "cylinder/torus chart-plane Jacobian is singular",
                ));
            }
            let mut matrix = [normal_a, normal_b, chord_normal];
            let mut rhs = [-residual_a, -residual_b, -projection_residual];
            let step = solve_regular_3x3(&mut matrix, &mut rhs).ok_or(
                IntersectionEvaluationError::Unsupported(
                    "cylinder/torus chart-plane solve is singular",
                ),
            )?;
            let step_length = norm(step);
            if !step_length.is_finite() || step_length > max_step {
                return Err(IntersectionEvaluationError::Unsupported(
                    "cylinder/torus Newton step exceeded its bound",
                ));
            }
            let mut alpha = 1.0;
            let mut accepted = None;
            for _ in 0..12 {
                let candidate = add(point, scale(step, alpha));
                if candidate.into_iter().all(f64::is_finite) {
                    let (next_a, next_b, _, _) = support_at(candidate)?;
                    let next_projection = dot(sub(candidate, chord_point), chord_normal);
                    let next_norm = next_a.abs().max(next_b.abs()).max(next_projection.abs());
                    if next_norm < residual_norm {
                        accepted = Some(candidate);
                        break;
                    }
                }
                alpha *= 0.5;
            }
            point = accepted.ok_or(IntersectionEvaluationError::Unsupported(
                "cylinder/torus Newton solve did not descend",
            ))?;
        }
        Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus Newton solve exceeded its iteration bound",
        ))
    };

    let mut roots = Vec::with_capacity(seed_offsets.len());
    for offset in seed_offsets {
        roots.push(solve_root(add(predictor, offset))?);
    }
    let mut chord_roots = Vec::<[f64; 3]>::new();
    for root in roots {
        if norm(sub(root, chord_point)) > chordal_bound {
            continue;
        }
        if !chord_roots
            .iter()
            .any(|existing| norm(sub(root, *existing)) <= 16.0 * tolerance)
        {
            chord_roots.push(root);
        }
    }
    if chord_roots.len() != 1 {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus chart chord does not select one local root",
        ));
    }
    let point = chord_roots[0];
    let (residual_a, residual_b, normal_a, normal_b) = support_at(point)?;
    let projection_residual = dot(sub(point, chord_point), chord_normal);
    if residual_a.abs() > tolerance
        || residual_b.abs() > tolerance
        || projection_residual.abs() > tolerance
        || norm(sub(point, chord_point)) > chordal_bound
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus root fails chart or support bounds",
        ));
    }
    let natural = cross(normal_a, normal_b);
    let natural_length = norm(natural);
    if !natural_length.is_finite() || natural_length <= 1.0e-8 {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus intersection is singular at evaluated point",
        ));
    }
    let mut tangent = scale(natural, 1.0 / natural_length);
    if dot(tangent, tangents[index]) < 0.0 {
        tangent = scale(tangent, -1.0);
    }
    if dot(tangent, tangents[index]) <= 0.0 {
        return Err(IntersectionEvaluationError::Unsupported(
            "cylinder/torus root is on the wrong chart branch",
        ));
    }
    Ok(IntersectionCurveEvaluation { point, tangent })
}

#[derive(Clone, Copy)]
struct CylinderPairChartSupports {
    origin_a: [f64; 3],
    axis_a: [f64; 3],
    radius_a: f64,
    origin_b: [f64; 3],
    axis_b: [f64; 3],
    radius_b: f64,
}

fn solve_cylinder_pair_chart_root(
    mut point: [f64; 3],
    chart_point: [f64; 3],
    chart_normal: [f64; 3],
    supports: CylinderPairChartSupports,
) -> Result<[f64; 3], IntersectionEvaluationError> {
    let CylinderPairChartSupports {
        origin_a,
        axis_a,
        radius_a,
        origin_b,
        axis_b,
        radius_b,
    } = supports;
    let tolerance = 4.0 * EPS_FACTOR * radius_a.max(radius_b);
    let residual_at = |point: [f64; 3]| -> Option<[f64; 3]> {
        let offset_a = sub(point, origin_a);
        let radial_a = sub(offset_a, scale(axis_a, dot(offset_a, axis_a)));
        let offset_b = sub(point, origin_b);
        let radial_b = sub(offset_b, scale(axis_b, dot(offset_b, axis_b)));
        let length_a = norm(radial_a);
        let length_b = norm(radial_b);
        (length_a.is_finite() && length_a > 1e-12 && length_b.is_finite() && length_b > 1e-12)
            .then_some([
                length_a - radius_a,
                length_b - radius_b,
                dot(sub(point, chart_point), chart_normal),
            ])
    };
    let max_step = 0.1 * radius_a.max(radius_b);
    for _ in 0..32 {
        let residual = residual_at(point).ok_or(IntersectionEvaluationError::Unsupported(
            "cylinder/cylinder chart-plane solve became degenerate",
        ))?;
        let error = norm(residual);
        if error <= tolerance {
            return Ok(point);
        }
        let offset_a = sub(point, origin_a);
        let radial_a = sub(offset_a, scale(axis_a, dot(offset_a, axis_a)));
        let offset_b = sub(point, origin_b);
        let radial_b = sub(offset_b, scale(axis_b, dot(offset_b, axis_b)));
        let Some(normal_a) = normalize(radial_a) else {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder chart-plane solve has a degenerate first normal",
            ));
        };
        let Some(normal_b) = normalize(radial_b) else {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder chart-plane solve has a degenerate second normal",
            ));
        };
        let mut matrix = [normal_a, normal_b, chart_normal];
        let mut rhs = scale(residual, -1.0);
        let Some(step) = solve_regular_3x3(&mut matrix, &mut rhs) else {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder chart-plane Jacobian is singular",
            ));
        };
        let step_length = norm(step);
        if !step_length.is_finite() || step_length > max_step {
            return Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder Newton step exceeded its relative bound",
            ));
        }
        let mut alpha = 1.0;
        let mut accepted = None;
        for _ in 0..12 {
            let candidate = add(point, scale(step, alpha));
            if residual_at(candidate).is_some_and(|next| norm(next) < error) {
                accepted = Some(candidate);
                break;
            }
            alpha *= 0.5;
        }
        point = accepted.ok_or(IntersectionEvaluationError::Unsupported(
            "cylinder/cylinder Newton solve did not descend",
        ))?;
    }
    Err(IntersectionEvaluationError::Unsupported(
        "cylinder/cylinder Newton solve exceeded its iteration bound",
    ))
}

fn solve_regular_3x3(matrix: &mut [[f64; 3]; 3], rhs: &mut [f64; 3]) -> Option<[f64; 3]> {
    for column in 0..3 {
        let pivot = (column..3).max_by(|&left, &right| {
            matrix[left][column]
                .abs()
                .total_cmp(&matrix[right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1e-12 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let diagonal = matrix[column][column];
        for item in &mut matrix[column][column..] {
            *item /= diagonal;
        }
        rhs[column] /= diagonal;
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for index in column..3 {
                matrix[row][index] -= factor * matrix[column][index];
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    rhs.iter().all(|value| value.is_finite()).then_some(*rhs)
}

fn complete_uv(
    coordinates: Option<[Option<f64>; 2]>,
) -> Result<[f64; 2], IntersectionEvaluationError> {
    let [Some(u), Some(v)] = coordinates.ok_or(IntersectionEvaluationError::Unsupported(
        "torus/swept candidate requires both surface UV pairs",
    ))?
    else {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept candidate requires finite surface UV pairs",
        ));
    };
    if !u.is_finite() || !v.is_finite() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart UV is not finite",
        ));
    }
    Ok([u, v])
}

fn nurbs_section_tangent(
    curve: &NurbsCurve,
    parameter: f64,
) -> Result<[f64; 3], IntersectionEvaluationError> {
    if curve.degree < 2
        || curve.periodic
        || curve.closed
        || curve.rational
        || curve.vertex_dimension != 3
    {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept tangent requires a nonrational open NURBS section of degree at least two",
        ));
    }
    let degree = usize::from(curve.degree);
    let count = usize::try_from(curve.control_vertex_count).map_err(|_| {
        IntersectionEvaluationError::InvalidInput("invalid swept section control count")
    })?;
    let expected = count
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "swept section knot total overflow",
        ))?;
    if !is_clamped_nurbs_section(curve, degree, count) {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus/swept tangent requires a clamped NURBS section",
        ));
    }
    let mut knots = Vec::with_capacity(expected);
    let mut previous = f64::NEG_INFINITY;
    for (index, (&knot, &mult)) in curve
        .knots
        .iter()
        .zip(&curve.knot_multiplicities)
        .enumerate()
    {
        let mult = usize::from(mult);
        if !knot.is_finite()
            || knot <= previous
            || mult == 0
            || (index > 0 && index + 1 < curve.knots.len() && mult > degree - 1)
        {
            return Err(IntersectionEvaluationError::Unsupported(
                "swept section derivative is discontinuous at a knot",
            ));
        }
        previous = knot;
        let next =
            knots
                .len()
                .checked_add(mult)
                .ok_or(IntersectionEvaluationError::InvalidInput(
                    "swept section knot total overflow",
                ))?;
        if next > expected {
            return Err(IntersectionEvaluationError::InvalidInput(
                "swept section knot count exceeds its control layout",
            ));
        }
        knots.resize(next, knot);
    }
    if knots.len() != expected
        || curve
            .control_vertices
            .iter()
            .any(|control| control.len() != 3 || control.iter().any(|value| !value.is_finite()))
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid swept section NURBS layout",
        ));
    }
    let parameter = parameter.clamp(knots[degree], knots[count]);
    super::evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(curve, parameter)
        .map_err(|_| IntersectionEvaluationError::InvalidInput("invalid swept section curve"))?;
    let derivative_controls = nurbs_section_derivative_controls(curve, degree, count, &knots)?;
    let mut multiplicities = curve.knot_multiplicities.clone();
    multiplicities[0] -= 1;
    let last = multiplicities.len() - 1;
    multiplicities[last] -= 1;
    let derivative = super::evaluate_nurbs_curve_values_candidate(
        degree - 1,
        false,
        false,
        count - 1,
        3,
        &derivative_controls,
        &multiplicities,
        &curve.knots,
        parameter,
        3,
    )
    .map_err(|_| {
        IntersectionEvaluationError::InvalidInput("failed to evaluate swept section tangent")
    })?;
    normalize([derivative[0], derivative[1], derivative[2]]).ok_or(
        IntersectionEvaluationError::Unsupported("swept section tangent is degenerate"),
    )
}

fn is_clamped_nurbs_section(curve: &NurbsCurve, degree: usize, count: usize) -> bool {
    count > degree
        && curve.control_vertices.len() == count
        && curve.knots.len() == curve.knot_multiplicities.len()
        && !curve.knot_multiplicities.is_empty()
        && curve.knot_multiplicities[0] == u16::try_from(degree + 1).unwrap_or(u16::MAX)
        && *curve.knot_multiplicities.last().unwrap_or(&0)
            == u16::try_from(degree + 1).unwrap_or(u16::MAX)
}

fn nurbs_section_derivative_controls(
    curve: &NurbsCurve,
    degree: usize,
    count: usize,
    knots: &[f64],
) -> Result<Vec<Vec<f64>>, IntersectionEvaluationError> {
    let mut derivative_controls = Vec::with_capacity(count - 1);
    for index in 0..count - 1 {
        let denominator = knots[index + degree + 1] - knots[index + 1];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Err(IntersectionEvaluationError::Unsupported(
                "swept section derivative has a zero knot interval",
            ));
        }
        derivative_controls.push(
            (0..3)
                .map(|axis| {
                    f64::from(curve.degree)
                        * (curve.control_vertices[index + 1][axis]
                            - curve.control_vertices[index][axis])
                        / denominator
                })
                .collect::<Vec<_>>(),
        );
    }
    Ok(derivative_controls)
}

fn torus_normal_at_point(
    point: [f64; 3],
    center: [f64; 3],
    axis: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
) -> Result<[f64; 3], IntersectionEvaluationError> {
    let axis = normalize(axis).ok_or(IntersectionEvaluationError::InvalidInput(
        "invalid torus axis",
    ))?;
    if !major_radius.is_finite()
        || !minor_radius.is_finite()
        || major_radius <= 0.0
        || minor_radius <= 0.0
    {
        return Err(IntersectionEvaluationError::InvalidInput(
            "invalid torus radii",
        ));
    }
    let relative = sub(point, center);
    let axial = dot(relative, axis);
    let radial = sub(relative, scale(axis, axial));
    let radial_length = norm(radial);
    if !radial_length.is_finite() || radial_length <= 0.0 {
        return Err(IntersectionEvaluationError::Unsupported(
            "torus normal is undefined on its axis",
        ));
    }
    normalize(add(
        scale(radial, (radial_length - major_radius) / radial_length),
        scale(axis, axial),
    ))
    .ok_or(IntersectionEvaluationError::Unsupported(
        "torus normal is degenerate",
    ))
}

fn unique_curve(
    brep: &BrepModel,
    id: u32,
) -> Result<&super::CurveGeometry, IntersectionEvaluationError> {
    let mut matches = brep.curves.iter().filter(|curve| curve.id == id);
    let Some(curve) = matches.next() else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "swept intersection section curve is missing from the B-Rep",
        ));
    };
    if matches.next().is_some() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "swept intersection section curve ID is not unique in the B-Rep",
        ));
    }
    Ok(curve)
}

#[allow(
    clippy::float_cmp,
    reason = "exact geometric coordinate equality distinguishes duplicate support values"
)]
fn unique_surface(
    brep: &BrepModel,
    id: u32,
) -> Result<&super::SurfaceGeometry, IntersectionEvaluationError> {
    let mut matches = brep.surfaces.iter().filter(|surface| surface.id == id);
    let Some(surface) = matches.next() else {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection support surface is missing from the B-Rep",
        ));
    };
    if matches.next().is_some() {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection support surface ID is not unique in the B-Rep",
        ));
    }
    Ok(surface)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    let scale = a.into_iter().chain(b).fold(1.0_f64, |m, x| m.max(x.abs()));
    norm(sub(a, b)) <= EPS_FACTOR * scale
}

fn collinear(offset: [f64; 3], direction: [f64; 3], tolerance: f64) -> bool {
    norm(cross(offset, direction)) <= tolerance
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn norm(a: [f64; 3]) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(a);
    (length.is_finite() && length > 0.0).then(|| scale(a, 1.0 / length))
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
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

    #[test]
    fn chart_parameter_at_station_returns_stored_float_exactly() {
        let parameters = [-92.874_661_664_936_47, 38.571_897_211_814_92];
        assert_eq!(
            chart_parameter_at_fraction(&parameters, 0, 0.0),
            parameters[0]
        );
        assert_ne!(
            parameters[0] + (parameters[1] - parameters[0]),
            parameters[1]
        );
        assert_eq!(
            chart_parameter_at_fraction(&parameters, 0, 1.0),
            parameters[1]
        );
    }
    use crate::brep::{
        BrepMetrics, BrepSourceFormat, CurveGeometry, IntersectionChartParameters,
        IntersectionData, IntersectionUvCoordinates, NurbsCurve, Sense, SourceNodeRef,
        SurfaceGeometry, TopologyValidation, Vector3,
    };

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn stable_quadratic_roots_preserve_small_ill_conditioned_root() {
        let roots = stable_quadratic_roots(1.0, 1.0e16, 1.0, 1.0e16).unwrap();
        let small_root = roots[1];
        assert!((small_root / -1.0e-16 - 1.0).abs() < 1.0e-15);
        let residual = small_root * small_root + 1.0e16 * small_root + 1.0;
        assert!(residual.abs() < 1.0e-15);

        assert_eq!(
            stable_quadratic_roots(1.0, 1.0, 0.0, 1.0),
            Some([-1.0, -0.0])
        );
        assert_eq!(
            stable_quadratic_roots(1.0, 2.0, 1.0, 0.0),
            Some([-1.0, -1.0])
        );
        assert!(stable_quadratic_roots(1.0, 0.0, 1.0, f64::INFINITY).is_none());
    }

    fn source() -> SourceNodeRef {
        SourceNodeRef {
            node_index: 0,
            node_type: 1,
            type_name: "test".into(),
            node_id: None,
            byte_range: 0..0,
        }
    }

    #[allow(
        clippy::unwrap_used,
        reason = "test fixture construction requires nonempty point arrays"
    )]
    fn fixture(points: &[[f64; 3]], normals: [[f64; 3]; 2]) -> (BrepModel, CurveKind) {
        let point = |p| Vector3::from_array(p);
        let surfaces = normals
            .into_iter()
            .enumerate()
            .map(|(i, normal)| SurfaceGeometry {
                id: i as u32 + 1,
                sense: Sense::Positive,
                owner: None,
                kind: SurfaceKind::Plane {
                    point: point([0.0; 3]),
                    normal: point(normal),
                    x_axis: point([1.0, 0.0, 0.0]),
                },
                source: source(),
            })
            .collect();
        let points: Vec<_> = points.iter().copied().map(point).collect();
        let curve = CurveKind::Intersection {
            surfaces: [1, 2],
            chart: source(),
            chart_parameters: IntersectionChartParameters {
                base_parameter: Some(10.0),
                base_scale: Some(0.5),
                chordal_error: None,
                angular_error: None,
            },
            start: source(),
            start_limit_kind: Some(IntersectionLimitKind::Limit),
            start_term_use: None,
            start_points: vec![points[0]],
            end: source(),
            end_limit_kind: Some(IntersectionLimitKind::Limit),
            end_term_use: None,
            end_points: vec![*points.last().unwrap()],
            chart_points: points,
            intersection_uv: None,
            intersection_data: None,
        };
        let brep = BrepModel {
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
        };
        (brep, curve)
    }

    #[allow(
        clippy::unwrap_used,
        reason = "test fixture construction requires nonempty point arrays"
    )]
    fn torus_swept_fixture() -> (BrepModel, CurveKind) {
        let section = NurbsCurve {
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
        };
        let points = [[1.6, 0.0, 0.3], [1.7, 0.0, 0.4]];
        let torus_uv = [[0.0, (0.6_f64).atan2(-0.8)], [0.0, (0.8_f64).atan2(-0.6)]];
        let swept_uv = [[0.4, 0.3], [0.425, 0.4]];
        let chart_uv = torus_uv
            .iter()
            .zip(swept_uv)
            .map(|(torus, swept)| IntersectionUvCoordinates {
                first_surface: Some([Some(torus[0]), Some(torus[1])]),
                second_surface: Some([Some(swept[0]), Some(swept[1])]),
            })
            .collect::<Vec<_>>();
        let raw_values = torus_uv
            .iter()
            .zip(swept_uv)
            .flat_map(|(torus, swept)| {
                [
                    Some(torus[0]),
                    Some(torus[1]),
                    Some(swept[0]),
                    Some(swept[1]),
                ]
            })
            .collect();
        let curve = CurveKind::Intersection {
            surfaces: [1, 2],
            chart: source(),
            chart_parameters: IntersectionChartParameters {
                base_parameter: Some(10.0),
                base_scale: Some(1.0),
                chordal_error: Some(0.1),
                angular_error: None,
            },
            chart_points: points.map(Vector3::from_array).to_vec(),
            start: source(),
            start_limit_kind: Some(IntersectionLimitKind::Limit),
            start_term_use: None,
            start_points: vec![Vector3::from_array(points[0])],
            end: source(),
            end_limit_kind: Some(IntersectionLimitKind::Limit),
            end_term_use: None,
            end_points: vec![Vector3::from_array(points[1])],
            intersection_uv: Some(IntersectionData {
                source: source(),
                uv_type: IntersectionUvType::Both,
                raw_values,
                start_is_terminator: false,
                start_terminator: None,
                end_is_terminator: false,
                chart: Some(chart_uv),
                end_terminator: None,
            }),
            intersection_data: Some(source()),
        };
        let brep = BrepModel {
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
            curves: vec![CurveGeometry {
                id: 10,
                sense: Sense::Positive,
                owner: None,
                kind: CurveKind::Nurbs(section),
                source: source(),
            }],
            surfaces: vec![
                SurfaceGeometry {
                    id: 1,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Torus {
                        center: Vector3::from_array([0.0; 3]),
                        axis: Vector3::from_array([0.0, 0.0, 1.0]),
                        major_radius: 2.0,
                        minor_radius: 0.5,
                        x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                    },
                    source: source(),
                },
                SurfaceGeometry {
                    id: 2,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Swept {
                        section: 10,
                        direction: Vector3::from_array([0.0, 0.0, 1.0]),
                    },
                    source: source(),
                },
            ],
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
        (brep, curve)
    }

    #[allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "fixture construction and exact chart coordinates are intentional"
    )]
    fn cylinder_torus_fixture() -> (BrepModel, CurveKind) {
        let angles = [0.0_f64, 0.2, 0.5];
        let points = angles
            .map(|angle| [2.0 * angle.cos(), 2.0 * angle.sin(), 1.5])
            .map(Vector3::from_array)
            .to_vec();
        let curve = CurveKind::Intersection {
            surfaces: [2, 1],
            chart: source(),
            chart_parameters: IntersectionChartParameters {
                base_parameter: Some(10.0),
                base_scale: Some(1.0),
                chordal_error: Some(0.03),
                angular_error: Some(0.31),
            },
            chart_points: points.clone(),
            start: source(),
            start_limit_kind: Some(IntersectionLimitKind::Limit),
            start_term_use: None,
            start_points: vec![points[0]],
            end: source(),
            end_limit_kind: Some(IntersectionLimitKind::Limit),
            end_term_use: None,
            end_points: vec![*points.last().unwrap()],
            intersection_uv: None,
            intersection_data: None,
        };
        let brep = BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "cylinder-torus-test".into(),
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
                    id: 1,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Torus {
                        center: Vector3::from_array([0.0; 3]),
                        axis: Vector3::from_array([0.0, 0.0, 1.0]),
                        major_radius: 2.0,
                        minor_radius: 1.5,
                        x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                    },
                    source: source(),
                },
                SurfaceGeometry {
                    id: 2,
                    sense: Sense::Positive,
                    owner: None,
                    kind: SurfaceKind::Cylinder {
                        point: Vector3::from_array([0.0; 3]),
                        axis: Vector3::from_array([0.0, 0.0, 1.0]),
                        radius: 2.0,
                        x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                    },
                    source: source(),
                },
            ],
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
        (brep, curve)
    }

    #[allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "the fixture supplies a second segment and exact chart stations"
    )]
    fn cylinder_torus_second_segment_midpoint_parameter(curve: &CurveKind) -> f64 {
        let CurveKind::Intersection { chart_points, .. } = curve else {
            unreachable!();
        };
        let points = chart_points
            .iter()
            .map(|point| point.to_array())
            .collect::<Vec<_>>();
        let first = sub(points[1], points[0]);
        let second = sub(points[2], points[1]);
        let tangent = [-0.2_f64.sin(), 0.2_f64.cos(), 0.0];
        let first_length = norm(first);
        let second_length = norm(second);
        let scale_at_second =
            (dot(tangent, first) / first_length) / (dot(tangent, second) / second_length);
        let first_station = 10.0 + first_length;
        first_station + second_length * scale_at_second * 0.5
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn evaluates_exact_straight_branch_and_closed_chart_domain() {
        let (brep, curve) = fixture(
            &[[0.0, 0.0, 0.0], [0.0, 0.0, 2.0], [0.0, 0.0, 4.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        let evaluated = evaluate_plane_plane_intersection_candidate(&brep, &curve, 11.5).unwrap();
        assert_eq!(evaluated.point, [0.0, 0.0, 3.0]);
        assert_eq!(evaluated.tangent, [0.0, 0.0, 1.0]);
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &curve, 10.0)
                .unwrap()
                .point,
            [0.0, 0.0, 0.0]
        );
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &curve, 12.0)
                .unwrap()
                .point,
            [0.0, 0.0, 4.0]
        );
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &curve, 9.99),
            Err(IntersectionEvaluationError::OutsideChartDomain)
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn evaluates_plane_cylinder_chart_plane_and_rejects_ambiguous_roots() {
        let (mut brep, mut curve) = fixture(
            &[[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [0.0, 1.0, 2.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(0.1);
        }
        let value = evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25).unwrap();
        assert!((value.point[1] - 1.0).abs() < 1e-12);
        assert!((value.point[2] - 0.5).abs() < 1e-12);
        assert_eq!(value.tangent, [0.0, 0.0, 1.0]);

        brep.surfaces[1].sense = Sense::Negative;
        if let CurveKind::Intersection { surfaces, .. } = &mut curve {
            *surfaces = [2, 1];
        }
        let reversed =
            evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25).unwrap();
        assert_eq!(reversed.point, value.point);
        assert_eq!(reversed.tangent, value.tangent);

        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = None;
        }
        assert!(matches!(
            evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25),
            Err(IntersectionEvaluationError::Unsupported(
                "ambiguous plane/cylinder chart-plane roots without chordal error"
            ))
        ));

        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(2.1);
        }
        assert!(matches!(
            evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25),
            Err(IntersectionEvaluationError::Unsupported(
                "ambiguous plane/cylinder chart-plane roots"
            ))
        ));

        if let CurveKind::Intersection { end_points, .. } = &mut curve {
            end_points[0].y = 0.5;
        }
        assert!(matches!(
            evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25),
            Err(IntersectionEvaluationError::Unsupported(
                "intersection requires finite chart points and single-point limits matching chart endpoints"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn evaluates_plane_xt_cone_chart_plane_with_support_order_and_sense() {
        let (mut brep, mut curve) = fixture(
            &[[1.0, 0.0, 0.0], [0.625, 0.0, 0.5], [0.25, 0.0, 1.0]],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        );
        brep.surfaces[1].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(0.01);
        }
        let parameter = 10.15625;
        let value =
            evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, parameter).unwrap();
        assert!((value.point[0] - 0.8125).abs() < 1e-12);
        assert!(value.point[1].abs() < 1e-12);
        assert!((value.point[2] - 0.25).abs() < 1e-12);
        assert!((value.tangent[0] + 0.6).abs() < 1e-12);
        assert!(value.tangent[1].abs() < 1e-12);
        assert!((value.tangent[2] - 0.8).abs() < 1e-12);

        brep.surfaces[1].sense = Sense::Negative;
        if let CurveKind::Intersection { surfaces, .. } = &mut curve {
            *surfaces = [2, 1];
        }
        assert_eq!(
            evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, parameter),
            Ok(value)
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn inverse_plane_xt_cone_parameter_is_bounded_and_requires_one_section_match() {
        let cone_fixture = |ys: &[f64], chordal_error: f64| {
            let points = ys
                .iter()
                .map(|y| [0.3, *y, (1.0 - 0.3_f64.hypot(*y)) / 0.75])
                .collect::<Vec<_>>();
            let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
            brep.surfaces[0].kind = SurfaceKind::Plane {
                point: Vector3::from_array([0.3, 0.0, 0.0]),
                normal: Vector3::from_array([1.0, 0.0, 0.0]),
                x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
            };
            brep.surfaces[1].kind = SurfaceKind::XtCone {
                point: Vector3::from_array([0.0; 3]),
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                radius: 1.0,
                sin_half_angle: 0.6,
                cos_half_angle: 0.8,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            };
            if let CurveKind::Intersection {
                chart_parameters, ..
            } = &mut curve
            {
                chart_parameters.chordal_error = Some(chordal_error);
            }
            (brep, curve)
        };

        let ys = [0.1, 0.4, 0.8];
        let (brep, curve) = cone_fixture(&ys, 0.5);
        let expected = evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, 10.15)
            .expect("interior chart section")
            .point;
        let parameter = inverse_plane_xt_cone_intersection_parameter(&brep, &curve, expected, 1e-8)
            .expect("one interior section matches");
        assert!((parameter - 10.15).abs() < 1e-10);

        let (chart_points, chart_parameters) =
            plane_xt_cone_inverse_chart_parameters(&brep, &curve, false).unwrap();
        for (index, direction) in [(0, -1.0), (chart_points.len() - 1, 1.0)] {
            let endpoint = evaluate_plane_xt_cone_intersection_candidate(
                &brep,
                &curve,
                chart_parameters[index],
            )
            .unwrap();
            let beyond = add(endpoint.point, scale(endpoint.tangent, direction * 1e-4));
            assert_eq!(
                inverse_plane_xt_cone_intersection_parameter(&brep, &curve, beyond, 2e-4),
                Ok(chart_parameters[index]),
            );
            assert!(matches!(
                inverse_plane_xt_cone_intersection_parameter(&brep, &curve, beyond, 5e-5),
                Err(IntersectionEvaluationError::Unsupported(
                    "no chart section uniquely matches the requested endpoint within tolerance"
                ))
            ));
        }

        let station = [0.3, ys[1], (1.0 - 0.3_f64.hypot(ys[1])) / 0.75];
        let station_parameter =
            inverse_plane_xt_cone_intersection_parameter(&brep, &curve, station, 1e-8)
                .expect("shared chart station is deduplicated");
        assert!(station_parameter > 10.0 && station_parameter < 11.0);

        assert!(matches!(
            inverse_plane_xt_cone_intersection_parameter(&brep, &curve, [1.0, 1.0, 0.0], 1e-6),
            Err(IntersectionEvaluationError::Unsupported(
                "no chart section uniquely matches the requested endpoint within tolerance"
            ))
        ));

        let chart_points = [
            [0.3, ys[0], (1.0 - 0.3_f64.hypot(ys[0])) / 0.75],
            station,
            [0.3, ys[2], (1.0 - 0.3_f64.hypot(ys[2])) / 0.75],
        ];
        let chord0 = sub(chart_points[1], chart_points[0]);
        let chord1 = sub(chart_points[2], chart_points[1]);
        let direction0 = chord0[2].atan2(chord0[1]);
        let direction1 = chord1[2].atan2(chord1[1]);
        let perpendicular = (direction0 + direction1) * 0.5 - std::f64::consts::FRAC_PI_2;
        let outward = [0.0, perpendicular.cos(), perpendicular.sin()];
        let ambiguous = add(station, scale(outward, 1e-3));
        assert!(matches!(
            inverse_plane_xt_cone_intersection_parameter(&brep, &curve, ambiguous, 0.01),
            Err(IntersectionEvaluationError::Unsupported(
                "multiple chart sections match the requested endpoint within tolerance"
            ))
        ));

        let mut oversized = curve.clone();
        if let CurveKind::Intersection { chart_points, .. } = &mut oversized {
            chart_points.resize(257, chart_points[0]);
        }
        assert!(matches!(
            inverse_plane_xt_cone_intersection_parameter(&brep, &oversized, station, 1e-6),
            Err(IntersectionEvaluationError::Unsupported(
                "plane/XT-cone inverse requires a bounded chart of 2 to 256 points"
            ))
        ));
        assert!(matches!(
            inverse_plane_xt_cone_intersection_parameter(&brep, &curve, station, f64::INFINITY),
            Err(IntersectionEvaluationError::InvalidInput(_))
        ));
        let mut invalid_support = brep;
        invalid_support.surfaces[0].kind = SurfaceKind::Sphere {
            center: Vector3::from_array([0.0; 3]),
            radius: 1.0,
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        assert!(matches!(
            inverse_plane_xt_cone_intersection_parameter(&invalid_support, &curve, station, 1e-6),
            Err(IntersectionEvaluationError::Unsupported(
                "candidate supports only plane/XT-cone or profile-mapped plane/cone intersections"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn inverse_plane_xt_cone_parameter_supports_reversed_mapped_axis() {
        let (mut brep, mut curve) = fixture(
            &[[1.75, 0.0, 1.0], [1.375, 0.0, 0.5], [1.0, 0.0, 0.0]],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        );
        brep.surfaces[1].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(0.01);
        }
        let expected = [1.5625, 0.0, 0.75];
        let parameter = inverse_plane_xt_cone_intersection_parameter(&brep, &curve, expected, 1e-8)
            .expect("reversed mapped axis convention is uniquely valid");
        assert!((parameter - 10.15625).abs() < 1e-10);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn plane_xt_cone_candidate_supports_explicit_reversed_axis_convention() {
        let evaluate = |points: &[[f64; 3]], axis: [f64; 3], reversed: bool| {
            let (mut brep, mut curve) = fixture(points, [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
            brep.surfaces[1].kind = SurfaceKind::XtCone {
                point: Vector3::from_array([0.0; 3]),
                axis: Vector3::from_array(axis),
                radius: 1.0,
                sin_half_angle: 0.6,
                cos_half_angle: 0.8,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            };
            if let CurveKind::Intersection {
                chart_parameters, ..
            } = &mut curve
            {
                chart_parameters.chordal_error = Some(0.01);
            }
            let candidate: fn(
                &BrepModel,
                &CurveKind,
                f64,
            )
                -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> = if reversed {
                evaluate_plane_xt_cone_intersection_candidate_with_reversed_mapped_axis
            } else {
                evaluate_plane_xt_cone_intersection_candidate
            };
            candidate(&brep, &curve, 10.15625).unwrap().point
        };

        let stored_axis = evaluate(
            &[[1.0, 0.0, 0.0], [0.625, 0.0, 0.5], [0.25, 0.0, 1.0]],
            [0.0, 0.0, 1.0],
            false,
        );
        let reversed_axis = evaluate(
            &[[1.75, 0.0, 1.0], [1.375, 0.0, 0.5], [1.0, 0.0, 0.0]],
            [0.0, 0.0, 1.0],
            true,
        );
        assert!((stored_axis[0] - 0.8125).abs() < 1e-12);
        assert!((reversed_axis[0] - 1.5625).abs() < 1e-12);
        assert!((reversed_axis[2] - 0.75).abs() < 1e-12);
    }

    #[test]
    fn plane_xt_cone_dispatch_selects_axis_convention_in_either_support_order() {
        for (points, expected) in [
            (
                [[1.0, 0.0, 0.0], [0.625, 0.0, 0.5], [0.25, 0.0, 1.0]],
                [0.8125, 0.0, 0.25],
            ),
            (
                [[1.75, 0.0, 1.0], [1.375, 0.0, 0.5], [1.0, 0.0, 0.0]],
                [1.5625, 0.0, 0.75],
            ),
        ] {
            let (mut brep, mut curve) = fixture(&points, [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
            brep.surfaces[1].kind = SurfaceKind::XtCone {
                point: Vector3::from_array([0.0; 3]),
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                radius: 1.0,
                sin_half_angle: 0.6,
                cos_half_angle: 0.8,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            };
            if let CurveKind::Intersection {
                chart_parameters,
                surfaces,
                ..
            } = &mut curve
            {
                chart_parameters.chordal_error = Some(0.01);
                *surfaces = [2, 1];
            }
            brep.surfaces[1].sense = Sense::Negative;
            let reversed_order = evaluate_intersection_candidate(&brep, &curve, 10.15625).unwrap();
            brep.surfaces[1].sense = Sense::Positive;
            if let CurveKind::Intersection { surfaces, .. } = &mut curve {
                *surfaces = [1, 2];
            }
            let ordinary_order = evaluate_intersection_candidate(&brep, &curve, 10.15625).unwrap();
            assert_eq!(reversed_order, ordinary_order);
            for axis in 0..3 {
                assert!((ordinary_order.point[axis] - expected[axis]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn plane_xt_cone_axis_dispatch_reports_pair_specific_ambiguity() {
        let candidate = Ok(IntersectionCurveEvaluation {
            point: [1.0, 2.0, 3.0],
            tangent: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            select_unique_axis_candidate(
                candidate.clone(),
                candidate,
                "both plane/XT-cone axis conventions validate",
                "neither plane/XT-cone axis convention validates",
            ),
            Err(IntersectionEvaluationError::Unsupported(
                "both plane/XT-cone axis conventions validate"
            ))
        );
        assert_eq!(
            select_unique_axis_candidate(
                Err(IntersectionEvaluationError::OutsideChartDomain),
                Err(IntersectionEvaluationError::InvalidInput(
                    "invalid intersection parameter, chart, or support references"
                )),
                "both plane/XT-cone axis conventions validate",
                "neither plane/XT-cone axis convention validates",
            ),
            Err(IntersectionEvaluationError::Unsupported(
                "neither plane/XT-cone axis convention validates"
            ))
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn rejects_ambiguous_or_degenerate_plane_xt_cone_charts() {
        let (mut brep, mut curve) = fixture(
            &[[1.0, 0.0, 0.0], [0.625, 0.0, 0.5], [0.25, 0.0, 1.0]],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        );
        brep.surfaces[1].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(10.0);
        }
        if let CurveKind::Intersection {
            start_limit_kind, ..
        } = &mut curve
        {
            *start_limit_kind = Some(IntersectionLimitKind::Terminator);
        }
        assert!(matches!(
            evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, 10.15625),
            Err(IntersectionEvaluationError::InvalidInput(
                "plane/XT-cone limit has the wrong number of points or mismatched endpoint"
            ))
        ));
        if let CurveKind::Intersection {
            start_limit_kind, ..
        } = &mut curve
        {
            *start_limit_kind = Some(IntersectionLimitKind::Limit);
        }
        assert!(matches!(
            evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, 10.15625),
            Err(IntersectionEvaluationError::Unsupported(
                "ambiguous plane/XT-cone chart-plane roots"
            ))
        ));

        if let CurveKind::Intersection { chart_points, .. } = &mut curve {
            chart_points[1] = chart_points[0];
        }
        assert!(matches!(
            evaluate_plane_xt_cone_intersection_candidate(&brep, &curve, 10.0),
            Err(IntersectionEvaluationError::InvalidInput(
                "degenerate or reversed plane/XT-cone chart chord"
            ))
        ));

        let mut wrong_nappe = curve;
        if let CurveKind::Intersection {
            chart_points,
            start_points,
            end_points,
            ..
        } = &mut wrong_nappe
        {
            let points = [[0.5, 0.0, 2.0], [0.875, 0.0, 2.5], [1.25, 0.0, 3.0]];
            for (target, point) in chart_points.iter_mut().zip(points) {
                *target = Vector3::from_array(point);
            }
            start_points[0] = Vector3::from_array(points[0]);
            end_points[0] = Vector3::from_array(points[2]);
        }
        assert!(matches!(
            evaluate_plane_xt_cone_intersection_candidate(&brep, &wrong_nappe, 10.0),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart point is not on both support surfaces"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn dispatches_plane_xt_cone_generator_with_independent_terminator_limits() {
        let points = [[0.1, 0.0, 1.2], [0.04, 0.0, 1.28]];
        let apex = [0.0, 0.0, 4.0 / 3.0];
        let (mut brep, curve) = fixture(&points, [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
        brep.surfaces[1].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        for terminator_at_start in [true, false] {
            let mut curve = curve.clone();
            let CurveKind::Intersection {
                chart_parameters,
                start_limit_kind,
                start_points,
                end_limit_kind,
                end_points,
                ..
            } = &mut curve
            else {
                unreachable!();
            };
            chart_parameters.chordal_error = Some(0.01);
            if terminator_at_start {
                *start_limit_kind = Some(IntersectionLimitKind::Terminator);
                *start_points = vec![Vector3::from_array(apex), Vector3::from_array(points[0])];
            } else {
                *end_limit_kind = Some(IntersectionLimitKind::Terminator);
                *end_points = vec![Vector3::from_array(apex), Vector3::from_array(points[1])];
            }

            let parameter = 10.0 + 0.5 * norm(sub(points[1], points[0])) * 0.5;
            let value = evaluate_intersection_candidate(&brep, &curve, parameter).unwrap();
            assert!((value.point[0] - 0.07).abs() < 1e-12);
            assert!(value.point[1].abs() < 1e-12);
            assert!((value.point[2] - 1.24).abs() < 1e-12);
        }

        let mut malformed = curve;
        if let CurveKind::Intersection {
            end_limit_kind,
            end_points,
            ..
        } = &mut malformed
        {
            *end_limit_kind = Some(IntersectionLimitKind::Terminator);
            *end_points = vec![Vector3::from_array(apex), Vector3::from_array(points[1])];
            end_points[0] = end_points[1];
        }
        assert!(matches!(
            evaluate_intersection_candidate(&brep, &malformed, 10.1),
            Err(IntersectionEvaluationError::InvalidInput(
                "plane/XT-cone terminator must have a distinct apex and chart endpoint branch point"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn dispatches_plane_cylinder_candidate() {
        let (mut brep, curve) = fixture(
            &[[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [0.0, 1.0, 2.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let mut curve = curve;
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(0.1);
        }
        let dispatched = evaluate_intersection_candidate(&brep, &curve, 10.25).unwrap();
        let direct = evaluate_plane_cylinder_intersection_candidate(&brep, &curve, 10.25).unwrap();
        assert_eq!(dispatched, direct);
        assert!((dispatched.point[2] - 0.5).abs() < 1e-12);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn dispatches_cylinder_cylinder_and_rejects_unsupported_or_duplicate_supports() {
        let (brep, curve, stations) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let parameter = (stations[0] + stations[1]) * 0.5;
        let dispatched = evaluate_intersection_candidate(&brep, &curve, parameter).unwrap();
        let direct =
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, parameter).unwrap();
        assert_eq!(dispatched, direct);

        let mut unsupported = brep.clone();
        unsupported.surfaces[1].kind = SurfaceKind::Sphere {
            center: Vector3::from_array([0.0; 3]),
            radius: 1.0,
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        assert!(matches!(
            evaluate_intersection_candidate(&unsupported, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let mut duplicate = brep;
        let duplicate_surface = duplicate.surfaces[0].clone();
        duplicate.surfaces.push(duplicate_surface);
        assert!(matches!(
            evaluate_intersection_candidate(&duplicate, &curve, parameter),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection support surface ID is not unique in the B-Rep"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn curved_plane_cylinder_chart_uses_interior_station_tangent_for_scale() {
        let point_at = |theta: f64| [theta.cos(), theta.sin(), 0.5 * theta.cos()];
        let tangent_at =
            |theta: f64| normalize([-theta.sin(), theta.cos(), -0.5 * theta.sin()]).unwrap();
        let angles = [0.15, 0.8, 1.9];
        let points = angles.map(point_at);
        let (mut brep, mut curve) = fixture(&points, [[-0.5, 0.0, 1.0], [0.0, 1.0, 0.0]]);
        brep.surfaces[0].kind = SurfaceKind::Plane {
            point: Vector3::from_array([0.0; 3]),
            normal: Vector3::from_array([-0.5, 0.0, 1.0]),
            x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let (base, initial_scale) = (3.0, 0.7);
        let chord0 = sub(points[1], points[0]);
        let chord1 = sub(points[2], points[1]);
        let length0 = norm(chord0);
        let length1 = norm(chord1);
        let tangent1 = tangent_at(angles[1]);
        let next_scale =
            initial_scale * (dot(tangent1, chord0) / length0) / (dot(tangent1, chord1) / length1);
        let t1 = base + initial_scale * length0;
        let t2 = t1 + next_scale * length1;
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.base_parameter = Some(base);
            chart_parameters.base_scale = Some(initial_scale);
            chart_parameters.chordal_error = Some(0.2);
        }

        let parameter = (t1 + t2) * 0.5;
        let chord_point = add(points[1], scale(chord1, 0.5));
        let chord_normal = normalize(chord1).unwrap();
        let plane_residual = |theta: f64| dot(chord_normal, sub(point_at(theta), chord_point));
        let (mut lo, mut hi) = (angles[1], angles[2]);
        for _ in 0..80 {
            let mid = (lo + hi) * 0.5;
            if plane_residual(lo).signum() == plane_residual(mid).signum() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let expected = point_at((lo + hi) * 0.5);
        let evaluated =
            evaluate_plane_cylinder_intersection_candidate(&brep, &curve, parameter).unwrap();
        for (axis, expected_coordinate) in expected.iter().enumerate() {
            assert!((evaluated.point[axis] - *expected_coordinate).abs() < 1e-12);
        }
        let expected_tangent = tangent_at((lo + hi) * 0.5);
        for (axis, expected_component) in expected_tangent.iter().enumerate() {
            assert!((evaluated.tangent[axis] - *expected_component).abs() < 1e-12);
        }
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn solves_xt_cone_cylinder_section_instead_of_returning_chord_point() {
        let cone_point = |u: f64| [1.5 * u.cos(), 1.5 * u.sin(), -1.0];
        let points = [0.15, 0.8, 1.9].map(cone_point);
        let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        brep.surfaces[0].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.447_213_595_499_957_9,
            cos_half_angle: 0.894_427_190_999_915_9,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        brep.surfaces[0].sense = Sense::Negative;
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.5,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.base_parameter = Some(2.0);
            chart_parameters.base_scale = Some(1.0);
            chart_parameters.chordal_error = Some(0.2);
        }
        let parameter = 2.0 + norm(sub(points[1], points[0])) * 0.5;
        let result = evaluate_cone_cylinder_intersection_candidate(&brep, &curve, parameter)
            .expect("solve the chart section on both supports");
        assert_eq!(
            evaluate_intersection_candidate(&brep, &curve, parameter),
            Ok(result),
            "dispatch accepts the sole axis convention validated by this fixture"
        );
        let radial = [result.point[0], result.point[1], 0.0];
        assert!((norm(radial) - 1.5).abs() < 1e-12);
        assert!((result.point[2] + 1.0).abs() < 1e-12);
        let chord_midpoint = scale(add(points[0], points[1]), 0.5);
        assert!(norm(sub(result.point, chord_midpoint)) > 0.01);
        assert!((norm(result.tangent) - 1.0).abs() < 1e-12);

        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = None;
        }
        assert!(matches!(
            evaluate_cone_cylinder_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(
                "cone/cylinder branch requires finite nonnegative chart chordal error"
            ))
        ));
        assert_eq!(
            evaluate_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(
                "cone/cylinder branch requires finite nonnegative chart chordal error"
            ))
        );
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.chordal_error = Some(10.0);
        }
        assert!(matches!(
            evaluate_cone_cylinder_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(
                "ambiguous cone/cylinder chart-plane roots"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn inverse_cone_cylinder_handles_axis_conventions_support_order_and_unique_matches() {
        for reverse_axis in [false, true] {
            for reverse_order in [false, true] {
                let z = if reverse_axis { 1.0 } else { -1.0 };
                let points = [0.15_f64, 0.8, 1.9].map(|u| [1.5 * u.cos(), 1.5 * u.sin(), z]);
                let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
                brep.surfaces[0].kind = SurfaceKind::XtCone {
                    point: Vector3::from_array([0.0; 3]),
                    axis: Vector3::from_array([0.0, 0.0, 1.0]),
                    radius: 1.0,
                    sin_half_angle: 0.447_213_595_499_957_9,
                    cos_half_angle: 0.894_427_190_999_915_9,
                    x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                };
                brep.surfaces[0].sense = Sense::Negative;
                brep.surfaces[1].kind = SurfaceKind::Cylinder {
                    point: Vector3::from_array([0.0; 3]),
                    axis: Vector3::from_array([0.0, 0.0, 1.0]),
                    radius: 1.5,
                    x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                };
                if let CurveKind::Intersection {
                    chart_parameters, ..
                } = &mut curve
                {
                    chart_parameters.base_parameter = Some(2.0);
                    chart_parameters.base_scale = Some(1.0);
                    chart_parameters.chordal_error = Some(0.3);
                }
                if !reverse_axis && !reverse_order {
                    let mut oversized_curve = curve.clone();
                    if let CurveKind::Intersection { chart_points, .. } = &mut oversized_curve {
                        let first = chart_points[0];
                        chart_points.resize(33, first);
                    }
                    assert_eq!(
                        inverse_cone_cylinder_intersection_parameter(
                            &brep,
                            &oversized_curve,
                            [0.0; 3],
                            1e-8,
                        ),
                        Err(IntersectionEvaluationError::Unsupported(
                            "cone/cylinder inverse requires a bounded chart of 2 to 32 points"
                        ))
                    );
                }
                if reverse_order {
                    if let CurveKind::Intersection {
                        surfaces,
                        chart_points,
                        start_points,
                        end_points,
                        ..
                    } = &mut curve
                    {
                        *surfaces = [2, 1];
                        chart_points.reverse();
                        start_points[0] = *chart_points.first().unwrap();
                        end_points[0] = *chart_points.last().unwrap();
                    }
                }
                let (_chart, parameters) =
                    cone_cylinder_inverse_chart_parameters(&brep, &curve, reverse_axis)
                        .expect("one mapped axis convention validates");
                let parameter = (parameters[0] + parameters[1]) * 0.5;
                let expected = evaluate_cone_cylinder_intersection_candidate_with_axis(
                    &brep,
                    &curve,
                    parameter,
                    reverse_axis,
                )
                .unwrap()
                .point;
                let (recovered, recovered_point) =
                    inverse_cone_cylinder_intersection_parameter(&brep, &curve, expected, 1e-8)
                        .expect("the evaluated section has one chart match");
                assert!((recovered - parameter).abs() < 1e-10);
                assert!(norm(sub(recovered_point, expected)) <= 1e-8);

                let off_curve = add(expected, [0.0, 0.0, 0.5]);
                assert!(matches!(
                    inverse_cone_cylinder_intersection_parameter(&brep, &curve, off_curve, 1e-8),
                    Err(IntersectionEvaluationError::Unsupported(
                        "no chart section uniquely matches the requested endpoint within tolerance"
                    ))
                ));
            }
        }
    }

    #[allow(
        clippy::unwrap_used,
        reason = "fixture construction requires a nonempty endpoint list"
    )]
    fn cone_cylinder_endpoint_fixture() -> (BrepModel, CurveKind) {
        let point_at = |u: f64| [1.5 * u.cos(), 1.5 * u.sin(), -1.0];
        let points = [0.15, 0.8, 1.9].map(point_at);
        let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        brep.surfaces[0].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.447_213_595_499_957_9,
            cos_half_angle: 0.894_427_190_999_915_9,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        brep.surfaces[0].sense = Sense::Negative;
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.5,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.base_parameter = Some(2.0);
            chart_parameters.base_scale = Some(1.0);
            chart_parameters.chordal_error = Some(10.0);
        }
        (brep, curve)
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn inverse_cone_cylinder_selects_one_of_two_chord_qualified_roots_by_endpoint() {
        let (brep, curve) = cone_cylinder_endpoint_fixture();
        let (_, parameters) = cone_cylinder_inverse_chart_parameters(&brep, &curve, false).unwrap();
        let parameter = (parameters[0] + parameters[1]) * 0.5;
        let roots =
            cone_cylinder_intersection_roots_with_axis(&brep, &curve, parameter, false).unwrap();
        assert_eq!(
            roots.len(),
            2,
            "both discovered roots satisfy CHART's chord error"
        );
        assert!(matches!(
            evaluate_cone_cylinder_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(
                "ambiguous cone/cylinder chart-plane roots"
            ))
        ));
        let (recovered, recovered_point) =
            inverse_cone_cylinder_intersection_parameter(&brep, &curve, roots[0].point, 1e-8)
                .expect("the endpoint tolerance selects exactly one discovered root");
        assert!(near(recovered_point, roots[0].point));
        assert!((recovered - parameter).abs() < 1e-10);
    }

    #[test]
    fn inverse_cone_cylinder_rejects_multiple_endpoint_matched_roots()
    -> Result<(), IntersectionEvaluationError> {
        let (brep, curve) = cone_cylinder_endpoint_fixture();
        let (_, parameters) = cone_cylinder_inverse_chart_parameters(&brep, &curve, false)?;
        let parameter = (parameters[0] + parameters[1]) * 0.5;
        let roots = cone_cylinder_intersection_roots_with_axis(&brep, &curve, parameter, false)?;
        assert_eq!(roots.len(), 2);
        let midpoint = scale(add(roots[0].point, roots[1].point), 0.5);
        let tolerance = norm(sub(roots[0].point, roots[1].point)) * 0.5 + 1e-8;
        assert_eq!(
            inverse_cone_cylinder_intersection_parameter(&brep, &curve, midpoint, tolerance),
            Err(IntersectionEvaluationError::Unsupported(
                "multiple chart sections match the requested endpoint within tolerance"
            ))
        );
        Ok(())
    }

    #[test]
    fn inverse_cone_cylinder_rejects_a_repeated_chart_endpoint_as_ambiguous() {
        let point_at = |u: f64| [1.5 * u.cos(), 1.5 * u.sin(), -1.0];
        let first = point_at(0.15);
        let points = [
            first,
            point_at(0.8),
            point_at(1.9),
            point_at(2.8),
            point_at(3.8),
            point_at(4.8),
            point_at(5.7),
            first,
            point_at(0.8),
        ];
        let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        brep.surfaces[0].kind = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.0,
            sin_half_angle: 0.447_213_595_499_957_9,
            cos_half_angle: 0.894_427_190_999_915_9,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        brep.surfaces[0].sense = Sense::Negative;
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 1.5,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.base_parameter = Some(2.0);
            chart_parameters.base_scale = Some(1.0);
            chart_parameters.chordal_error = Some(0.3);
        }
        assert!(matches!(
            inverse_cone_cylinder_intersection_parameter(&brep, &curve, first, 1e-8),
            Err(IntersectionEvaluationError::Unsupported(
                "multiple chart sections match the requested endpoint within tolerance"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn cone_axis_dispatch_rejects_two_successful_conventions() {
        let mapped = IntersectionCurveEvaluation {
            point: [1.0, 2.0, 3.0],
            tangent: [1.0, 0.0, 0.0],
        };
        let reversed = IntersectionCurveEvaluation {
            point: [4.0, 5.0, 6.0],
            tangent: [0.0, 1.0, 0.0],
        };
        assert_eq!(
            select_unique_cone_axis_candidate(Ok(mapped), Ok(reversed)),
            Err(IntersectionEvaluationError::Unsupported(
                "both cone/cylinder axis conventions validate"
            ))
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn cone_axis_dispatch_preserves_matching_errors_and_hides_mixed_errors() {
        assert_eq!(
            select_unique_cone_axis_candidate(
                Err(IntersectionEvaluationError::OutsideChartDomain),
                Err(IntersectionEvaluationError::OutsideChartDomain),
            ),
            Err(IntersectionEvaluationError::OutsideChartDomain)
        );
        assert_eq!(
            select_unique_cone_axis_candidate(
                Err(IntersectionEvaluationError::OutsideChartDomain),
                Err(IntersectionEvaluationError::InvalidInput(
                    "invalid intersection parameter, chart, or support references"
                )),
            ),
            Err(IntersectionEvaluationError::Unsupported(
                "neither cone/cylinder axis convention validates"
            ))
        );
    }

    #[allow(
        clippy::unwrap_used,
        reason = "fixture construction seeds stations before extending them"
    )]
    fn perpendicular_cylinder_fixture(z_values: &[f64]) -> (BrepModel, CurveKind, Vec<f64>) {
        let points: Vec<_> = z_values
            .iter()
            .map(|z| {
                let radial = (1.0 - z * z).sqrt();
                [radial, radial, *z]
            })
            .collect();
        let (mut brep, mut curve) = fixture(&points, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        brep.surfaces[0].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 1.0, 0.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        if let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut curve
        {
            chart_parameters.base_parameter = Some(10.0);
            chart_parameters.base_scale = Some(1.0);
        }
        let mut stations = vec![10.0];
        for pair in points.windows(2) {
            stations.push(stations.last().unwrap() + norm(sub(pair[1], pair[0])));
        }
        (brep, curve, stations)
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn evaluates_cylinder_cylinder_chart_plane_and_checks_domain_and_limits() {
        let (brep, curve, stations) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let parameter = (stations[0] + stations[1]) * 0.5;
        let result = evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, parameter)
            .expect("evaluate the locally selected cylinder intersection branch");
        assert!((result.point[1].powi(2) + result.point[2].powi(2) - 1.0).abs() < 1e-12);
        assert!((result.point[0].powi(2) + result.point[2].powi(2) - 1.0).abs() < 1e-12);
        assert!(result.point[0] > 0.0 && result.point[1] > 0.0);
        assert!(
            dot(
                result.tangent,
                sub(
                    [
                        (1.0_f64 - 0.4 * 0.4).sqrt(),
                        (1.0_f64 - 0.4 * 0.4).sqrt(),
                        0.4,
                    ],
                    [
                        (1.0_f64 - 0.1 * 0.1).sqrt(),
                        (1.0_f64 - 0.1 * 0.1).sqrt(),
                        0.1,
                    ],
                )
            ) > 0.0
        );
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, f64::NAN),
            Err(IntersectionEvaluationError::InvalidInput(_))
        ));
        assert_eq!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, stations[0] - 1e-6),
            Err(IntersectionEvaluationError::OutsideChartDomain)
        );

        let (brep, mut terminator_curve, _) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let CurveKind::Intersection { start_points, .. } = &mut terminator_curve else {
            unreachable!();
        };
        start_points.push(start_points[0]);
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &terminator_curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn cylinder_cylinder_accepts_only_closed_help_limits() {
        let (mut brep, mut curve, _) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        brep.surfaces[0].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([0.0, 1.0, 0.0]),
            radius: 1.0,
            x_axis: Vector3::from_array([0.0, 0.0, 1.0]),
        };
        brep.surfaces[1].kind = SurfaceKind::Cylinder {
            point: Vector3::from_array([0.0; 3]),
            axis: Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 2.0,
            x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
        };
        let points: Vec<_> = (0..=16)
            .map(|index| {
                let angle = -std::f64::consts::TAU * f64::from(index) / 16.0;
                Vector3::from_array([angle.sin(), (4.0 - angle.cos().powi(2)).sqrt(), angle.cos()])
            })
            .collect();
        let locator = points[0];
        let CurveKind::Intersection {
            chart_points,
            start_limit_kind,
            start_points,
            end_limit_kind,
            end_points,
            ..
        } = &mut curve
        else {
            unreachable!();
        };
        *chart_points = points;
        *start_limit_kind = Some(IntersectionLimitKind::Help);
        *end_limit_kind = Some(IntersectionLimitKind::Help);
        *start_points = vec![locator];
        *end_points = vec![locator];

        let result = evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, 10.0)
            .expect("closed cylinder intersection help point identifies the chart branch");
        assert_eq!(result.point, locator.to_array());

        let mut mixed_limits = curve.clone();
        let CurveKind::Intersection { end_limit_kind, .. } = &mut mixed_limits else {
            unreachable!();
        };
        *end_limit_kind = Some(IntersectionLimitKind::Limit);
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &mixed_limits, 10.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        for unsupported_kind in [None, Some(IntersectionLimitKind::Boundary)] {
            let mut unsupported_limits = curve.clone();
            let CurveKind::Intersection {
                start_limit_kind,
                end_limit_kind,
                ..
            } = &mut unsupported_limits
            else {
                unreachable!();
            };
            *start_limit_kind = unsupported_kind;
            *end_limit_kind = unsupported_kind;
            assert!(matches!(
                evaluate_cylinder_cylinder_intersection_candidate(&brep, &unsupported_limits, 10.0),
                Err(IntersectionEvaluationError::Unsupported(_))
            ));
        }

        let mut mismatched_locators = curve.clone();
        let CurveKind::Intersection { end_points, .. } = &mut mismatched_locators else {
            unreachable!();
        };
        end_points[0] = Vector3::from_array([0.0, 0.0, 1.0]);
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &mismatched_locators, 10.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let mut open_chart = curve.clone();
        let CurveKind::Intersection { chart_points, .. } = &mut open_chart else {
            unreachable!();
        };
        let next = chart_points[1];
        *chart_points.last_mut().unwrap() = next;
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &open_chart, 10.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn rejects_off_cylinder_chart_when_translation_exceeds_local_precision() {
        let (mut brep, mut curve, stations) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let translation = [1.0e12; 3];
        for surface in &mut brep.surfaces {
            let SurfaceKind::Cylinder { point, .. } = &mut surface.kind else {
                unreachable!("fixture uses cylinders")
            };
            *point = Vector3::from_array(translation);
        }
        let CurveKind::Intersection {
            chart_points,
            start_points,
            end_points,
            ..
        } = &mut curve
        else {
            unreachable!("fixture uses an intersection")
        };
        for point in chart_points.iter_mut() {
            *point = Vector3::from_array(add(point.to_array(), translation));
        }
        let mut off_surface = chart_points[0].to_array();
        off_surface[1] += 0.01;
        chart_points[0] = Vector3::from_array(off_surface);
        start_points[0] = chart_points[0];
        *end_points.last_mut().unwrap() = *chart_points.last().unwrap();

        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, stations[0]),
            Err(IntersectionEvaluationError::Unsupported(
                "cylinder/cylinder coordinates exceed the supported local precision"
            ))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn evaluates_small_cylinder_chart_near_the_origin() {
        let (mut brep, mut curve, _) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let radius = 1.0e-6;
        for surface in &mut brep.surfaces {
            let SurfaceKind::Cylinder { radius: value, .. } = &mut surface.kind else {
                unreachable!("fixture uses cylinders")
            };
            *value = radius;
        }
        {
            let CurveKind::Intersection {
                chart_points,
                start_points,
                end_points,
                ..
            } = &mut curve
            else {
                unreachable!("fixture uses an intersection")
            };
            for point in chart_points.iter_mut() {
                *point = Vector3::from_array(scale(point.to_array(), radius));
            }
            start_points[0] = chart_points[0];
            *end_points.last_mut().unwrap() = *chart_points.last().unwrap();
        }

        let result = evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, 10.0)
            .expect("small near-origin chart remains representable");
        let CurveKind::Intersection { chart_points, .. } = curve else {
            unreachable!("fixture uses an intersection")
        };
        assert_eq!(result.point, chart_points[0].to_array());
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn rejects_limit_mismatch_at_modest_translation() {
        let (mut brep, mut curve, _) = perpendicular_cylinder_fixture(&[0.1, 0.4, 0.7]);
        let translation = [100.0; 3];
        for surface in &mut brep.surfaces {
            let SurfaceKind::Cylinder { point, .. } = &mut surface.kind else {
                unreachable!("fixture uses cylinders")
            };
            *point = Vector3::from_array(translation);
        }
        let CurveKind::Intersection {
            chart_points,
            start_points,
            end_points,
            ..
        } = &mut curve
        else {
            unreachable!("fixture uses an intersection")
        };
        for point in chart_points.iter_mut() {
            *point = Vector3::from_array(add(point.to_array(), translation));
        }
        start_points[0] = chart_points[0];
        *end_points.last_mut().unwrap() = *chart_points.last().unwrap();
        let mut misplaced_limit = start_points[0].to_array();
        misplaced_limit[0] += 1.0e-12;
        start_points[0] = Vector3::from_array(misplaced_limit);

        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, 10.0),
            Err(IntersectionEvaluationError::Unsupported(
                "intersection limits do not match the chart endpoints within local precision"
            ))
        ));
    }

    #[test]
    fn rejects_degenerate_cylinder_cylinder_chart_tangent() {
        let (mut brep, curve) = fixture(
            &[[1.0, 0.0, 0.0], [0.8, 0.6, 0.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        for surface in &mut brep.surfaces {
            surface.kind = SurfaceKind::Cylinder {
                point: Vector3::from_array([0.0; 3]),
                axis: Vector3::from_array([0.0, 0.0, 1.0]),
                radius: 1.0,
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            };
        }
        assert!(matches!(
            evaluate_cylinder_cylinder_intersection_candidate(&brep, &curve, 10.5),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    fn honors_reversed_surface_order_and_rejects_bad_or_unsupported_data() {
        let (brep, curve) = fixture(
            &[[0.0, 0.0, 4.0], [0.0, 0.0, 2.0], [0.0, 0.0, 0.0]],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        );
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &curve, 11.5)
                .unwrap()
                .tangent,
            [0.0, 0.0, -1.0]
        );
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &curve, f64::NAN),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection parameter is not finite"
            ))
        );
        let (parallel, parallel_curve) = fixture(
            &[[0.0, 0.0, 0.0], [0.0, 0.0, 2.0]],
            [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        );
        assert!(matches!(
            evaluate_plane_plane_intersection_candidate(&parallel, &parallel_curve, 10.5),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "fixture setup and exact floating-point outputs are intentional in this focused test"
    )]
    fn rejects_nonfinite_geometry_and_nonunique_or_missing_supports() {
        let points = [[0.0, 0.0, 0.0], [0.0, 0.0, 2.0]];
        let normals = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

        let (mut nonfinite_origin, curve) = fixture(&points, normals);
        let SurfaceKind::Plane { point, .. } = &mut nonfinite_origin.surfaces[0].kind else {
            unreachable!();
        };
        point.x = f64::NAN;
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&nonfinite_origin, &curve, 10.5),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection support origin is not finite"
            ))
        );

        let (brep, mut nonfinite_chart) = fixture(&points, normals);
        let CurveKind::Intersection { chart_points, .. } = &mut nonfinite_chart else {
            unreachable!();
        };
        chart_points[1].z = f64::INFINITY;
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &nonfinite_chart, 10.5),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart or limit contains a nonfinite point"
            ))
        );

        let (brep, mut nonfinite_limit) = fixture(&points, normals);
        let CurveKind::Intersection { end_points, .. } = &mut nonfinite_limit else {
            unreachable!();
        };
        end_points[0].x = f64::NEG_INFINITY;
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&brep, &nonfinite_limit, 10.5),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart or limit contains a nonfinite point"
            ))
        );

        let (mut duplicate_support, curve) = fixture(&points, normals);
        duplicate_support
            .surfaces
            .push(duplicate_support.surfaces[0].clone());
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&duplicate_support, &curve, 10.5),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection support surface ID is not unique in the B-Rep"
            ))
        );

        let (mut missing_support, curve) = fixture(&points, normals);
        missing_support.surfaces.retain(|surface| surface.id != 2);
        assert_eq!(
            evaluate_plane_plane_intersection_candidate(&missing_support, &curve, 10.5),
            Err(IntersectionEvaluationError::InvalidInput(
                "intersection support surface is missing from the B-Rep"
            ))
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn evaluates_torus_swept_chart_interval_and_oriented_tangent() {
        let (brep, curve) = torus_swept_fixture();
        let chart_length = norm([0.1, 0.0, 0.1]);
        let parameter = 10.0 + chart_length * 0.5;
        let result = evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter)
            .expect("regular torus/swept chart interval");
        assert!(result.point.into_iter().all(f64::is_finite));
        assert!(result.point[1].abs() < 1e-12);
        assert!(((result.point[0] - 2.0).powi(2) + result.point[2].powi(2) - 0.25).abs() < 1e-12);
        let expected_tangent = normalize([result.point[2], 0.0, 2.0 - result.point[0]])
            .expect("regular support-normal cross product");
        for (actual, expected) in result.tangent.into_iter().zip(expected_tangent) {
            assert!((actual - expected).abs() < 1e-12);
        }
        let CurveKind::Intersection { chart_points, .. } = &curve else {
            unreachable!();
        };
        assert_eq!(
            evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, 10.0)
                .unwrap()
                .point,
            chart_points[0].to_array()
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        reason = "the regular fixture guarantees its chord and evaluation; exact endpoint equality is intentional"
    )]
    fn evaluates_bounded_cylinder_torus_chart_interval_with_support_tangent() {
        let (brep, curve) = cylinder_torus_fixture();
        let parameter = cylinder_torus_second_segment_midpoint_parameter(&curve);
        let CurveKind::Intersection { chart_points, .. } = &curve else {
            unreachable!();
        };
        let points = chart_points
            .iter()
            .map(|point| point.to_array())
            .collect::<Vec<_>>();
        let second_chord = sub(points[2], points[1]);
        let expected_angle = 0.35_f64;
        let result = evaluate_cylinder_torus_intersection_candidate(&brep, &curve, parameter)
            .expect("regular cylinder/torus chart interval");
        assert!((result.point[0] - 2.0 * expected_angle.cos()).abs() < 1.0e-11);
        assert!((result.point[1] - 2.0 * expected_angle.sin()).abs() < 1.0e-11);
        assert!((result.point[2] - 1.5).abs() < 1.0e-11);
        let expected_tangent = [-expected_angle.sin(), expected_angle.cos(), 0.0];
        for (actual, expected) in result.tangent.into_iter().zip(expected_tangent) {
            assert!((actual - expected).abs() < 1.0e-11);
        }
        let chord_point = scale(add(points[1], points[2]), 0.5);
        let chord_normal = normalize(second_chord).unwrap();
        assert!(dot(sub(result.point, chord_point), chord_normal).abs() < 1.0e-11);
        assert_eq!(
            evaluate_cylinder_torus_intersection_candidate(&brep, &curve, 10.0)
                .unwrap()
                .point,
            points[0]
        );
        assert_eq!(
            evaluate_cylinder_torus_intersection_candidate(&brep, &curve, 9.99),
            Err(IntersectionEvaluationError::OutsideChartDomain)
        );
    }

    #[test]
    fn cylinder_torus_newton_handles_negative_support_sense() {
        let parameter = {
            let (_, curve) = cylinder_torus_fixture();
            cylinder_torus_second_segment_midpoint_parameter(&curve)
        };
        for negative_surface_id in [1, 2] {
            let (mut brep, curve) = cylinder_torus_fixture();
            brep.surfaces
                .iter_mut()
                .find(|surface| surface.id == negative_surface_id)
                .unwrap()
                .sense = Sense::Negative;
            let result = evaluate_cylinder_torus_intersection_candidate(&brep, &curve, parameter)
                .expect("negative-sense cylinder/torus support");
            let expected_angle = 0.35_f64;
            assert!((result.point[0] - 2.0 * expected_angle.cos()).abs() < 1.0e-11);
            assert!((result.point[1] - 2.0 * expected_angle.sin()).abs() < 1.0e-11);
            assert!((result.point[2] - 1.5).abs() < 1.0e-11);
            let expected_tangent = [-expected_angle.sin(), expected_angle.cos(), 0.0];
            for (actual, expected) in result.tangent.into_iter().zip(expected_tangent) {
                assert!((actual - expected).abs() < 1.0e-11);
            }
        }
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn cylinder_torus_evaluator_fails_closed_on_limits_singularities_and_chordal_ambiguity() {
        let (brep, mut terminator_curve) = cylinder_torus_fixture();
        let CurveKind::Intersection {
            start_limit_kind, ..
        } = &mut terminator_curve
        else {
            unreachable!();
        };
        *start_limit_kind = Some(IntersectionLimitKind::Terminator);
        let parameter = cylinder_torus_second_segment_midpoint_parameter(&terminator_curve);
        assert!(matches!(
            evaluate_cylinder_torus_intersection_candidate(&brep, &terminator_curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let (brep, mut tight_chordal_curve) = cylinder_torus_fixture();
        let CurveKind::Intersection {
            chart_parameters, ..
        } = &mut tight_chordal_curve
        else {
            unreachable!();
        };
        chart_parameters.chordal_error = Some(1.0e-8);
        assert!(matches!(
            evaluate_cylinder_torus_intersection_candidate(&brep, &tight_chordal_curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let (mut singular_brep, mut singular_curve) = cylinder_torus_fixture();
        let SurfaceKind::Torus { minor_radius, .. } = &mut singular_brep.surfaces[0].kind else {
            unreachable!();
        };
        *minor_radius = 1.0;
        let SurfaceKind::Cylinder { radius, .. } = &mut singular_brep.surfaces[1].kind else {
            unreachable!();
        };
        *radius = 1.0;
        let angles = [0.0_f64, 0.2, 0.5];
        let points = angles
            .map(|angle| Vector3::from_array([angle.cos(), angle.sin(), 0.0]))
            .to_vec();
        let CurveKind::Intersection {
            chart_points,
            start_points,
            end_points,
            ..
        } = &mut singular_curve
        else {
            unreachable!();
        };
        *chart_points = points.clone();
        *start_points = vec![points[0]];
        *end_points = vec![*points.last().unwrap()];
        assert!(matches!(
            evaluate_cylinder_torus_intersection_candidate(
                &singular_brep,
                &singular_curve,
                parameter
            ),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "this focused test uses unwrap and expect for established fixture invariants"
    )]
    fn evaluates_torus_swept_regular_intervals_with_start_or_end_terminator() {
        let chart_length = norm([0.1, 0.0, 0.1]);
        let parameter = 10.0 + chart_length * 0.5;
        for start_terminator in [true, false] {
            let (brep, mut curve) = torus_swept_fixture();
            let baseline =
                evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter)
                    .expect("ordinary-limit baseline");
            let CurveKind::Intersection {
                start_limit_kind,
                start_points,
                end_limit_kind,
                end_points,
                ..
            } = &mut curve
            else {
                unreachable!();
            };
            let (singularity, endpoint) = if start_terminator {
                *start_limit_kind = Some(IntersectionLimitKind::Terminator);
                let branch_point = start_points[0];
                let singularity = Vector3::from_array([1.5, 0.25, 0.25]);
                *start_points = vec![singularity, branch_point];
                (singularity, start_points[1])
            } else {
                *end_limit_kind = Some(IntersectionLimitKind::Terminator);
                let branch_point = end_points[0];
                let singularity = Vector3::from_array([1.8, -0.25, 0.5]);
                *end_points = vec![singularity, branch_point];
                (singularity, end_points[1])
            };
            assert!(endpoint.to_array().into_iter().all(f64::is_finite));
            assert!(norm(sub(singularity.to_array(), endpoint.to_array())) > 0.1);
            let result =
                evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter)
                    .expect("terminator branch is regular inside chart");
            assert_eq!(result, baseline);
            assert_eq!(
                evaluate_torus_ruled_sweep_intersection_candidate(
                    &brep,
                    &curve,
                    if start_terminator {
                        10.0 - 1.0e-6
                    } else {
                        10.0 + chart_length + 1.0e-6
                    }
                ),
                Err(IntersectionEvaluationError::OutsideChartDomain)
            );
        }
    }

    #[test]
    fn torus_swept_terminator_rejects_mismatched_branch_point() {
        let (brep, mut curve) = torus_swept_fixture();
        let parameter = 10.0 + norm([0.1, 0.0, 0.1]) * 0.5;
        let CurveKind::Intersection {
            start_limit_kind,
            start_points,
            ..
        } = &mut curve
        else {
            unreachable!();
        };
        *start_limit_kind = Some(IntersectionLimitKind::Terminator);
        *start_points = vec![
            Vector3::from_array([1.5, 0.25, 0.25]),
            Vector3::from_array([0.0, 0.0, 0.0]),
        ];
        assert_eq!(
            evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::InvalidInput(
                "torus/swept limit branch point disagrees with chart endpoint"
            ))
        );
    }

    #[test]
    fn torus_swept_evaluator_rejects_unknown_limits_and_incomplete_uv() {
        let (brep, mut curve) = torus_swept_fixture();
        let parameter = 10.0 + norm([0.1, 0.0, 0.1]) * 0.5;
        let CurveKind::Intersection {
            start_limit_kind, ..
        } = &mut curve
        else {
            unreachable!();
        };
        *start_limit_kind = None;
        assert!(matches!(
            evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let (brep, mut curve) = torus_swept_fixture();
        let CurveKind::Intersection {
            intersection_uv: Some(data),
            ..
        } = &mut curve
        else {
            unreachable!();
        };
        data.chart.as_mut().unwrap()[0].first_surface = Some([None, Some(0.0)]);
        assert!(matches!(
            evaluate_torus_ruled_sweep_intersection_candidate(&brep, &curve, parameter),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }
}
