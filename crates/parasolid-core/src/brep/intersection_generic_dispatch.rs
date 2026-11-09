//! Bounded CHART evaluation for otherwise-unhandled regular support pairs.

use super::evaluate::plane_uv_from_point;
use super::intersection_generic::{
    GenericIntersectionEvaluation, evaluate_generic_chart_intersection,
};
use super::{
    BrepModel, CurveKind, IntersectionCurveEvaluation, IntersectionEvaluationError,
    IntersectionLimitKind, IntersectionUvType, SurfaceGeometry, evaluate_brep_surface,
};

/// Evaluates only the regular interval represented by an XT `INTERSECTION` CHART.
pub(crate) fn evaluate_generic_intersection(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<IntersectionCurveEvaluation, IntersectionEvaluationError> {
    evaluate_generic_intersection_with_uv(brep, curve, parameter).map(|result| {
        IntersectionCurveEvaluation {
            point: result.point,
            tangent: result.tangent,
        }
    })
}

#[allow(
    clippy::float_cmp,
    reason = "Exact station equality returns the stored chart sample without interpolation."
)]
#[allow(
    clippy::too_many_lines,
    reason = "The support-pair dispatch keeps candidate eligibility and evaluation fallback order explicit."
)]
pub(crate) fn evaluate_generic_intersection_with_uv(
    brep: &BrepModel,
    curve: &CurveKind,
    parameter: f64,
) -> Result<GenericIntersectionEvaluation, IntersectionEvaluationError> {
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
            "invalid intersection parameter, chart, or support references",
        ));
    }
    let points: Vec<_> = chart_points.iter().map(|point| point.to_array()).collect();
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection chart contains a nonfinite point",
        ));
    }
    validate_limits(
        *start_limit_kind,
        start_points,
        *end_limit_kind,
        end_points,
        points[0],
        points[points.len() - 1],
    )?;
    let surface_a = unique_surface(brep, surfaces[0])?;
    let surface_b = unique_surface(brep, surfaces[1])?;
    let intersection_data =
        intersection_uv
            .as_ref()
            .ok_or(IntersectionEvaluationError::Unsupported(
                "generic intersection requires supported chart UV data",
            ))?;
    if matches!(
        intersection_data.uv_type,
        IntersectionUvType::Unsupported(_)
    ) {
        return Err(IntersectionEvaluationError::Unsupported(
            "generic intersection requires complete two-surface chart UV data",
        ));
    }
    let uv_chart = intersection_data
        .chart
        .as_ref()
        .filter(|chart| chart.len() == points.len())
        .ok_or(IntersectionEvaluationError::Unsupported(
            "generic intersection requires chart UV data for every CHART point",
        ))?;
    let uv_chart: Vec<([f64; 2], [f64; 2])> = match intersection_data.uv_type {
        IntersectionUvType::Both => uv_chart
            .iter()
            .map(|uv| {
                let first = uv
                    .first_surface
                    .ok_or(IntersectionEvaluationError::Unsupported(
                        "generic intersection chart has incomplete first-surface UV data",
                    ))?;
                let second = uv
                    .second_surface
                    .ok_or(IntersectionEvaluationError::Unsupported(
                        "generic intersection chart has incomplete second-surface UV data",
                    ))?;
                let [Some(u0), Some(v0)] = first else {
                    return Err(IntersectionEvaluationError::Unsupported(
                        "generic intersection chart has incomplete first-surface UV pair",
                    ));
                };
                let [Some(u1), Some(v1)] = second else {
                    return Err(IntersectionEvaluationError::Unsupported(
                        "generic intersection chart has incomplete second-surface UV pair",
                    ));
                };
                let pair = ([u0, v0], [u1, v1]);
                if pair
                    .0
                    .into_iter()
                    .chain(pair.1)
                    .any(|value| !value.is_finite())
                {
                    return Err(IntersectionEvaluationError::InvalidInput(
                        "generic intersection chart UV data is nonfinite",
                    ));
                }
                Ok(pair)
            })
            .collect::<Result<_, _>>()?,
        IntersectionUvType::Second
            if matches!(&surface_a.kind, super::SurfaceKind::Plane { .. }) =>
        {
            uv_chart
                .iter()
                .zip(&points)
                .map(|(uv, point)| {
                    if uv.first_surface.is_some() {
                        return Err(IntersectionEvaluationError::Unsupported(
                            "generic intersection Second UV mode has unexpected first-surface data",
                        ));
                    }
                    let second =
                        uv.second_surface
                            .ok_or(IntersectionEvaluationError::Unsupported(
                                "generic intersection chart has incomplete second-surface UV data",
                            ))?;
                    let [Some(u), Some(v)] = second else {
                        return Err(IntersectionEvaluationError::Unsupported(
                            "generic intersection chart has incomplete second-surface UV pair",
                        ));
                    };
                    if !u.is_finite() || !v.is_finite() {
                        return Err(IntersectionEvaluationError::InvalidInput(
                            "generic intersection chart UV data is nonfinite",
                        ));
                    }
                    let first = plane_uv_from_point(&surface_a.kind, *point).ok_or(
                        IntersectionEvaluationError::Unsupported(
                            "generic intersection first-surface plane UV cannot be recovered",
                        ),
                    )?;
                    Ok((first, [u, v]))
                })
                .collect::<Result<_, _>>()?
        }
        IntersectionUvType::First
            if matches!(&surface_b.kind, super::SurfaceKind::Plane { .. }) =>
        {
            uv_chart
                .iter()
                .zip(&points)
                .map(|(uv, point)| {
                    if uv.second_surface.is_some() {
                        return Err(IntersectionEvaluationError::Unsupported(
                            "generic intersection First UV mode has unexpected second-surface data",
                        ));
                    }
                    let first =
                        uv.first_surface
                            .ok_or(IntersectionEvaluationError::Unsupported(
                                "generic intersection chart has incomplete first-surface UV data",
                            ))?;
                    let [Some(u), Some(v)] = first else {
                        return Err(IntersectionEvaluationError::Unsupported(
                            "generic intersection chart has incomplete first-surface UV pair",
                        ));
                    };
                    if !u.is_finite() || !v.is_finite() {
                        return Err(IntersectionEvaluationError::InvalidInput(
                            "generic intersection chart UV data is nonfinite",
                        ));
                    }
                    let second = plane_uv_from_point(&surface_b.kind, *point).ok_or(
                        IntersectionEvaluationError::Unsupported(
                            "generic intersection second-surface plane UV cannot be recovered",
                        ),
                    )?;
                    Ok(([u, v], second))
                })
                .collect::<Result<_, _>>()?
        }
        _ => {
            return Err(IntersectionEvaluationError::Unsupported(
                "generic intersection supports Both UV data or single-surface UV data with a plane on the missing side",
            ));
        }
    };
    let base = chart_parameters
        .base_parameter
        .filter(|value| value.is_finite())
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base parameter is missing or nonfinite",
        ))?;
    let base_scale = chart_parameters
        .base_scale
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(IntersectionEvaluationError::InvalidInput(
            "intersection chart base scale must be finite and positive",
        ))?;
    let chordal_error = match chart_parameters.chordal_error {
        Some(value) if value.is_finite() && value >= 0.0 => Some(value),
        None => None,
        Some(_) => {
            return Err(IntersectionEvaluationError::Unsupported(
                "generic intersection requires finite nonnegative chart chordal error",
            ));
        }
    };
    let mut tangents = Vec::with_capacity(points.len());
    for (uv_a, uv_b) in &uv_chart {
        let eval_a = evaluate_brep_surface(brep, &surface_a.kind, uv_a[0], uv_a[1]).ok_or(
            IntersectionEvaluationError::Unsupported(
                "generic intersection first-surface UV cannot be evaluated",
            ),
        )?;
        let eval_b = evaluate_brep_surface(brep, &surface_b.kind, uv_b[0], uv_b[1]).ok_or(
            IntersectionEvaluationError::Unsupported(
                "generic intersection second-surface UV cannot be evaluated",
            ),
        )?;
        if !eval_a
            .point
            .into_iter()
            .chain(eval_a.normal)
            .chain(eval_b.point)
            .chain(eval_b.normal)
            .all(f64::is_finite)
        {
            return Err(IntersectionEvaluationError::InvalidInput(
                "generic intersection UV evaluation is nonfinite",
            ));
        }
        let coordinate_scale = points
            .iter()
            .flatten()
            .chain(eval_a.point.iter())
            .chain(eval_b.point.iter())
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
        let support_tolerance = 1.0e-8 * coordinate_scale;
        if norm(sub(eval_a.point, points[tangents.len()])) > support_tolerance
            || norm(sub(eval_b.point, points[tangents.len()])) > support_tolerance
            || norm(sub(eval_a.point, eval_b.point)) > support_tolerance
        {
            return Err(IntersectionEvaluationError::Unsupported(
                "generic intersection chart UV does not match its support point",
            ));
        }
        let sense_a =
            surface_a
                .sense
                .multiplier()
                .ok_or(IntersectionEvaluationError::Unsupported(
                    "generic intersection first support has unknown normal sense",
                ))?;
        let sense_b =
            surface_b
                .sense
                .multiplier()
                .ok_or(IntersectionEvaluationError::Unsupported(
                    "generic intersection second support has unknown normal sense",
                ))?;
        tangents.push(
            normalize(cross(
                scale(eval_a.normal, sense_a),
                scale(eval_b.normal, sense_b),
            ))
            .ok_or(IntersectionEvaluationError::Unsupported(
                "generic intersection support normals are singular",
            ))?,
        );
    }

    let mut stations = Vec::with_capacity(points.len());
    stations.push(base);
    let mut scale_at = base_scale;
    for index in 0..points.len() - 1 {
        let chord = sub(points[index + 1], points[index]);
        let chord_length = norm(chord);
        if !chord_length.is_finite() || chord_length <= f64::EPSILON {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart contains a zero-length chord",
            ));
        }
        if dot(tangents[index], chord) < 0.0 {
            tangents[index] = scale(tangents[index], -1.0);
        }
        let previous_cos = dot(tangents[index], chord) / chord_length;
        if !previous_cos.is_finite() || previous_cos <= f64::EPSILON {
            return Err(IntersectionEvaluationError::Unsupported(
                "chart order disagrees with UV-derived support tangent",
            ));
        }
        let next_station = stations[index] + chord_length * scale_at;
        if !next_station.is_finite() || next_station <= stations[index] {
            return Err(IntersectionEvaluationError::InvalidInput(
                "intersection chart parameter range is degenerate",
            ));
        }
        stations.push(next_station);
        if index + 1 < points.len() - 1 {
            let next_chord = sub(points[index + 2], points[index + 1]);
            let next_length = norm(next_chord);
            if !next_length.is_finite() || next_length <= f64::EPSILON {
                return Err(IntersectionEvaluationError::Unsupported(
                    "chart order disagrees with UV-derived support tangent",
                ));
            }
            tangents[index + 1] = if dot(tangents[index + 1], next_chord) < 0.0 {
                scale(tangents[index + 1], -1.0)
            } else {
                tangents[index + 1]
            };
            let corner_previous_cos = dot(tangents[index + 1], chord) / chord_length;
            let next_cos = dot(tangents[index + 1], next_chord) / next_length;
            if !corner_previous_cos.is_finite()
                || corner_previous_cos <= f64::EPSILON
                || !next_cos.is_finite()
                || next_cos <= f64::EPSILON
            {
                return Err(IntersectionEvaluationError::Unsupported(
                    "chart order disagrees with UV-derived support tangent",
                ));
            }
            scale_at *= corner_previous_cos / next_cos;
            if !scale_at.is_finite() || scale_at <= 0.0 {
                return Err(IntersectionEvaluationError::InvalidInput(
                    "intersection chart parameter scale is degenerate",
                ));
            }
        }
    }
    let last_chord = sub(points[points.len() - 1], points[points.len() - 2]);
    if dot(tangents[points.len() - 1], last_chord).abs() <= f64::EPSILON {
        return Err(IntersectionEvaluationError::Unsupported(
            "chart order disagrees with UV-derived support tangent",
        ));
    }
    if dot(tangents[points.len() - 1], last_chord) < 0.0 {
        let last = points.len() - 1;
        tangents[last] = scale(tangents[last], -1.0);
    }
    let (Some(&first_station), Some(&last_station)) = (stations.first(), stations.last()) else {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    };
    // Reconstructed CHART stations can drift a few ULPs past their interval endpoints.
    let endpoint_tolerance = 64.0 * f64::EPSILON * first_station.abs().max(last_station.abs());
    let parameter = if parameter < first_station && first_station - parameter <= endpoint_tolerance
    {
        first_station
    } else if parameter > last_station && parameter - last_station <= endpoint_tolerance {
        last_station
    } else {
        parameter
    };
    if parameter < first_station || parameter > last_station {
        return Err(IntersectionEvaluationError::OutsideChartDomain);
    }
    if let Some(index) = stations.iter().position(|station| *station == parameter) {
        return Ok(GenericIntersectionEvaluation {
            point: points[index],
            tangent: tangents[index],
            uv_a: uv_chart[index].0,
            uv_b: uv_chart[index].1,
        });
    }
    let index = stations
        .windows(2)
        .position(|interval| interval[0] < parameter && parameter < interval[1])
        .ok_or(IntersectionEvaluationError::OutsideChartDomain)?;
    let fraction = (parameter - stations[index]) / (stations[index + 1] - stations[index]);
    let (first_surface_uv_start, second_surface_uv_start) = uv_chart[index];
    let (first_surface_uv_end, second_surface_uv_end) = uv_chart[index + 1];
    let result = evaluate_generic_chart_intersection(
        brep,
        &surface_a.kind,
        &surface_b.kind,
        [
            [first_surface_uv_start, first_surface_uv_end],
            [second_surface_uv_start, second_surface_uv_end],
        ],
        [points[index], points[index + 1]],
        fraction,
        chordal_error,
    )?;
    Ok(GenericIntersectionEvaluation {
        point: result.point,
        tangent: result.tangent,
        uv_a: result.uv_a,
        uv_b: result.uv_b,
    })
}

fn validate_limits(
    start_kind: Option<IntersectionLimitKind>,
    start: &[super::Vector3],
    end_kind: Option<IntersectionLimitKind>,
    end: &[super::Vector3],
    chart_start: [f64; 3],
    chart_end: [f64; 3],
) -> Result<(), IntersectionEvaluationError> {
    let finite = start
        .iter()
        .chain(end)
        .all(|point| point.to_array().into_iter().all(f64::is_finite));
    if !finite {
        return Err(IntersectionEvaluationError::InvalidInput(
            "intersection limit contains a nonfinite point",
        ));
    }
    match (start_kind, end_kind) {
        (
            Some(start_kind @ (IntersectionLimitKind::Limit | IntersectionLimitKind::Terminator)),
            Some(end_kind @ (IntersectionLimitKind::Limit | IntersectionLimitKind::Terminator)),
        ) if regular_limit_matches(start_kind, start, chart_start)
            && regular_limit_matches(end_kind, end, chart_end) =>
        {
            Ok(())
        }
        (Some(IntersectionLimitKind::Help), Some(IntersectionLimitKind::Help))
            if start.len() == 1
                && end.len() == 1
                && near(start[0].to_array(), chart_start)
                && near(end[0].to_array(), chart_end)
                && near(chart_start, chart_end) =>
        {
            Ok(())
        }
        (Some(IntersectionLimitKind::Boundary), _) | (_, Some(IntersectionLimitKind::Boundary)) => {
            Err(IntersectionEvaluationError::Unsupported(
                "XT B limit is not applicable to intersection curves",
            ))
        }
        _ => Err(IntersectionEvaluationError::Unsupported(
            "generic intersection requires valid L/T endpoint limits or closed H/H limits",
        )),
    }
}

fn regular_limit_matches(
    kind: IntersectionLimitKind,
    points: &[super::Vector3],
    endpoint: [f64; 3],
) -> bool {
    match kind {
        IntersectionLimitKind::Limit => points.len() == 1 && near(points[0].to_array(), endpoint),
        IntersectionLimitKind::Terminator => {
            points.len() == 2
                && !near(points[0].to_array(), endpoint)
                && near(points[1].to_array(), endpoint)
        }
        _ => false,
    }
}

fn unique_surface(
    brep: &BrepModel,
    id: u32,
) -> Result<&SurfaceGeometry, IntersectionEvaluationError> {
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
    norm(sub(a, b))
        <= 1.0e-8
            * a.into_iter()
                .chain(b)
                .fold(1.0_f64, |scale, v| scale.max(v.abs()))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|value| value * s)
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(a);
    (length.is_finite() && length > f64::EPSILON).then(|| scale(a, 1.0 / length))
}

#[cfg(test)]
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
        BrepMetrics, BrepSourceFormat, IntersectionChartParameters, IntersectionData,
        IntersectionUvCoordinates, Sense, SourceNodeRef, SurfaceGeometry, SurfaceKind,
        TopologyValidation, Vector3,
    };

    fn source() -> SourceNodeRef {
        SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: "test".into(),
            node_id: None,
            byte_range: 0..0,
        }
    }

    fn vector(value: [f64; 3]) -> Vector3 {
        Vector3::from_array(value)
    }

    fn fixture() -> (BrepModel, CurveKind) {
        let a = 0.1_f64;
        let chart_points = vec![
            vector([a.cos(), -a.sin(), 0.0]),
            vector([a.cos(), a.sin(), 0.0]),
        ];
        let uv = vec![
            IntersectionUvCoordinates {
                first_surface: Some([Some(a.cos()), Some(-a.sin())]),
                second_surface: Some([Some(-a), Some(0.0)]),
            },
            IntersectionUvCoordinates {
                first_surface: Some([Some(a.cos()), Some(a.sin())]),
                second_surface: Some([Some(a), Some(0.0)]),
            },
        ];
        let make_surface = |id, kind| SurfaceGeometry {
            id,
            sense: Sense::Positive,
            owner: None,
            kind,
            source: source(),
        };
        let brep = BrepModel {
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
            surfaces: vec![
                make_surface(
                    1,
                    SurfaceKind::Plane {
                        point: vector([0.0; 3]),
                        normal: vector([0.0, 0.0, 1.0]),
                        x_axis: vector([1.0, 0.0, 0.0]),
                    },
                ),
                make_surface(
                    2,
                    SurfaceKind::Sphere {
                        center: vector([0.0; 3]),
                        radius: 1.0,
                        axis: vector([0.0, 0.0, 1.0]),
                        x_axis: vector([1.0, 0.0, 0.0]),
                    },
                ),
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
        let curve = CurveKind::Intersection {
            surfaces: [1, 2],
            chart: source(),
            chart_parameters: IntersectionChartParameters {
                base_parameter: Some(2.0),
                base_scale: Some(1.0),
                chordal_error: Some(0.02),
                angular_error: None,
            },
            chart_points: chart_points.clone(),
            start: source(),
            start_limit_kind: Some(IntersectionLimitKind::Limit),
            start_term_use: Some(b'F'),
            start_points: vec![chart_points[0]],
            end: source(),
            end_limit_kind: Some(IntersectionLimitKind::Limit),
            end_term_use: Some(b'S'),
            end_points: vec![chart_points[1]],
            intersection_uv: Some(IntersectionData {
                source: source(),
                uv_type: IntersectionUvType::Both,
                raw_values: vec![],
                start_is_terminator: false,
                start_terminator: None,
                end_is_terminator: false,
                chart: Some(uv),
                end_terminator: None,
            }),
            intersection_data: None,
        };
        (brep, curve)
    }

    #[test]
    fn generic_dispatch_evaluates_regular_uv_chart_segment() {
        let (brep, curve) = fixture();
        let midpoint = 2.0 + (0.1_f64).sin();
        let result = evaluate_generic_intersection(&brep, &curve, midpoint).unwrap();
        assert!((result.point[0] - 1.0).abs() < 1e-8);
        assert!(result.point[1].abs() < 1e-8 && result.point[2].abs() < 1e-8);
        assert!(result.tangent[1] > 0.99);
        let result_with_uv =
            evaluate_generic_intersection_with_uv(&brep, &curve, midpoint).unwrap();
        assert_eq!(
            result_with_uv.point.map(f64::to_bits),
            result.point.map(f64::to_bits)
        );
        assert_eq!(
            result_with_uv.tangent.map(f64::to_bits),
            result.tangent.map(f64::to_bits)
        );
        for (surface, uv) in brep
            .surfaces
            .iter()
            .zip([result_with_uv.uv_a, result_with_uv.uv_b])
        {
            let support_point = evaluate_brep_surface(&brep, &surface.kind, uv[0], uv[1])
                .expect("solver UV evaluates on its support")
                .point;
            assert!(norm(sub(support_point, result.point)) < 1e-8);
        }
        let exact_station = evaluate_generic_intersection_with_uv(&brep, &curve, 2.0).unwrap();
        assert_eq!(
            exact_station.uv_a.map(f64::to_bits),
            [0.1_f64.cos(), -0.1_f64.sin()].map(f64::to_bits)
        );
        assert_eq!(
            exact_station.uv_b.map(f64::to_bits),
            [-0.1_f64, 0.0].map(f64::to_bits)
        );
        let dispatched = crate::brep::evaluate_intersection_candidate(&brep, &curve, midpoint)
            .expect("generic support-pair fallback evaluates the chart midpoint");
        assert_eq!(dispatched.point, result.point);

        let mut mismatched = curve.clone();
        if let CurveKind::Intersection {
            intersection_uv: Some(data),
            ..
        } = &mut mismatched
        {
            data.chart.as_mut().unwrap()[0].first_surface = Some([Some(0.5), Some(-0.1_f64.sin())]);
        }
        assert!(matches!(
            evaluate_generic_intersection(&brep, &mismatched, 2.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));

        let mut t_curve = curve.clone();
        if let CurveKind::Intersection {
            start_limit_kind,
            start_points,
            ..
        } = &mut t_curve
        {
            *start_limit_kind = Some(IntersectionLimitKind::Terminator);
            start_points.clear();
            start_points.extend([
                vector([0.0, 0.0, 1.0]),
                vector([0.1_f64.cos(), -0.1_f64.sin(), 0.0]),
            ]);
        }
        assert!(evaluate_generic_intersection(&brep, &t_curve, midpoint).is_ok());

        let mut b_curve = curve;
        if let CurveKind::Intersection {
            start_limit_kind, ..
        } = &mut b_curve
        {
            *start_limit_kind = Some(IntersectionLimitKind::Boundary);
        }
        assert!(matches!(
            evaluate_generic_intersection(&brep, &b_curve, midpoint),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    fn public_dispatch_recovers_missing_plane_uv_from_second_mode() {
        let (brep, curve) = fixture();
        let mut second_only = curve.clone();
        let CurveKind::Intersection {
            intersection_uv: Some(data),
            ..
        } = &mut second_only
        else {
            unreachable!();
        };
        data.uv_type = IntersectionUvType::Second;
        for uv in data.chart.as_mut().unwrap() {
            uv.first_surface = None;
        }

        let angle = 0.1_f64;
        for parameter in [2.0, 2.0 + angle.sin(), 2.0 + 2.0 * angle.sin()] {
            let baseline =
                crate::brep::evaluate_intersection_candidate(&brep, &curve, parameter).unwrap();
            let recovered =
                crate::brep::evaluate_intersection_candidate(&brep, &second_only, parameter)
                    .unwrap();
            assert_eq!(recovered, baseline);
        }

        let mut off_plane = second_only;
        let CurveKind::Intersection {
            chart_points,
            start_points,
            ..
        } = &mut off_plane
        else {
            unreachable!();
        };
        chart_points[0].z = 1.0e-3;
        start_points[0] = chart_points[0];
        assert!(matches!(
            crate::brep::evaluate_intersection_candidate(&brep, &off_plane, 2.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    fn public_dispatch_recovers_missing_plane_uv_from_first_mode() {
        let (brep, curve) = fixture();
        let mut swapped_both = curve.clone();
        let CurveKind::Intersection {
            surfaces,
            intersection_uv: Some(data),
            ..
        } = &mut swapped_both
        else {
            unreachable!();
        };
        surfaces.swap(0, 1);
        for uv in data.chart.as_mut().unwrap() {
            std::mem::swap(&mut uv.first_surface, &mut uv.second_surface);
        }

        let mut first_only = swapped_both.clone();
        let CurveKind::Intersection {
            intersection_uv: Some(data),
            ..
        } = &mut first_only
        else {
            unreachable!();
        };
        data.uv_type = IntersectionUvType::First;
        for uv in data.chart.as_mut().unwrap() {
            uv.second_surface = None;
        }

        let angle = 0.1_f64;
        for parameter in [2.0, 2.0 + angle.sin(), 2.0 + 2.0 * angle.sin()] {
            let baseline =
                crate::brep::evaluate_intersection_candidate(&brep, &swapped_both, parameter)
                    .unwrap();
            let recovered =
                crate::brep::evaluate_intersection_candidate(&brep, &first_only, parameter)
                    .unwrap();
            for axis in 0..3 {
                assert!((recovered.point[axis] - baseline.point[axis]).abs() < 1.0e-8);
                assert!((recovered.tangent[axis] - baseline.tangent[axis]).abs() < 1.0e-8);
            }
        }

        let mut off_plane = first_only;
        let CurveKind::Intersection {
            chart_points,
            start_points,
            ..
        } = &mut off_plane
        else {
            unreachable!();
        };
        chart_points[0].z = 1.0e-3;
        start_points[0] = chart_points[0];
        assert!(matches!(
            crate::brep::evaluate_intersection_candidate(&brep, &off_plane, 2.0),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }

    #[test]
    fn generic_limits_accept_independently_valid_mixed_l_t_endpoints() {
        let chart_start = [1.0, 0.0, 0.0];
        let chart_end = [2.0, 0.0, 0.0];
        let singularity = [0.0, 0.0, 0.0];
        let start_l = [vector(chart_start)];
        let start_t = [vector(singularity), vector(chart_start)];
        let end_l = [vector(chart_end)];
        let end_t = [vector(singularity), vector(chart_end)];

        assert!(
            validate_limits(
                Some(IntersectionLimitKind::Limit),
                &start_l,
                Some(IntersectionLimitKind::Terminator),
                &end_t,
                chart_start,
                chart_end,
            )
            .is_ok()
        );
        assert!(
            validate_limits(
                Some(IntersectionLimitKind::Terminator),
                &start_t,
                Some(IntersectionLimitKind::Limit),
                &end_l,
                chart_start,
                chart_end,
            )
            .is_ok()
        );

        let invalid_end_t = [vector(chart_end), vector(chart_end)];
        assert!(
            validate_limits(
                Some(IntersectionLimitKind::Limit),
                &start_l,
                Some(IntersectionLimitKind::Terminator),
                &invalid_end_t,
                chart_start,
                chart_end,
            )
            .is_err()
        );
    }

    #[test]
    fn public_dispatch_accepts_missing_chordal_error_for_unique_generic_root() {
        let (mut brep, mut curve) = fixture();
        let angle = 0.1_f64;
        brep.surfaces[0].kind = SurfaceKind::Cylinder {
            point: vector([0.0; 3]),
            axis: vector([0.0, 0.0, 1.0]),
            radius: 1.0,
            x_axis: vector([1.0, 0.0, 0.0]),
        };
        brep.surfaces[1].kind = SurfaceKind::Sphere {
            center: vector([0.0; 3]),
            radius: 2.0_f64.sqrt(),
            axis: vector([0.0, 0.0, 1.0]),
            x_axis: vector([1.0, 0.0, 0.0]),
        };
        let chart_points = vec![
            vector([angle.cos(), -angle.sin(), 1.0]),
            vector([angle.cos(), angle.sin(), 1.0]),
        ];
        let uv = vec![
            IntersectionUvCoordinates {
                first_surface: Some([Some(-angle), Some(1.0)]),
                second_surface: Some([Some(-angle), Some(std::f64::consts::FRAC_PI_4)]),
            },
            IntersectionUvCoordinates {
                first_surface: Some([Some(angle), Some(1.0)]),
                second_surface: Some([Some(angle), Some(std::f64::consts::FRAC_PI_4)]),
            },
        ];
        let CurveKind::Intersection {
            chart_parameters,
            chart_points: points,
            start_points,
            end_points,
            intersection_uv: Some(data),
            ..
        } = &mut curve
        else {
            unreachable!();
        };
        chart_parameters.chordal_error = None;
        *points = chart_points.clone();
        *start_points = vec![chart_points[0]];
        *end_points = vec![chart_points[1]];
        data.chart = Some(uv);

        let parameter = 2.0 + angle.sin();
        let result = crate::brep::evaluate_intersection_candidate(&brep, &curve, parameter)
            .expect("the bounded generic UV search finds the unique local root");
        assert!((result.point[0] - 1.0).abs() < 1.0e-8);
        assert!(result.point[1].abs() < 1.0e-8);
        assert!((result.point[2] - 1.0).abs() < 1.0e-8);
    }

    #[test]
    fn paired_help_limits_require_one_shared_chart_locator() {
        let locator = vector([1.0, 2.0, 3.0]);
        assert!(
            validate_limits(
                Some(IntersectionLimitKind::Help),
                &[locator],
                Some(IntersectionLimitKind::Help),
                &[locator],
                locator.to_array(),
                locator.to_array(),
            )
            .is_ok()
        );
        assert!(matches!(
            validate_limits(
                Some(IntersectionLimitKind::Help),
                &[locator],
                Some(IntersectionLimitKind::Help),
                &[locator],
                locator.to_array(),
                [1.0, 2.0, 4.0],
            ),
            Err(IntersectionEvaluationError::Unsupported(_))
        ));
    }
}
