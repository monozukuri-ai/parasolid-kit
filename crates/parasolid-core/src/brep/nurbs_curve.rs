//! Bounded evaluation of mapped NURBS curve data.

use std::io;

use super::NurbsCurve;

const MAX_CONTROLS: usize = 1_048_576;
const MAX_PRECISE_PERIODIC_WRAPS: f64 = 4_503_599_627_370_496.0;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Evaluates one mapped Parasolid 3D NURBS curve in its raw frame.
///
/// # Errors
/// Returns an error when the curve layout or parameter is invalid or unsupported.
pub fn evaluate_mapped_nurbs_curve_candidate(
    curve: &NurbsCurve,
    parameter: f64,
) -> io::Result<[f64; 3]> {
    evaluate_mapped_nurbs_curve_inner(curve, parameter, 3)?
        .try_into()
        .map_err(|_| invalid("unsupported NURBS layout"))
}

/// Evaluates a mapped 3D NURBS curve with linear endpoint-tangent extension.
///
/// Extension is available only for open, nonperiodic, nonrational curves with
/// clamped endpoints. The tangent uses the curve's stored knot parameter.
///
/// # Errors
/// Returns an error when the curve layout, endpoint interval, or parameter is invalid.
pub fn evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(
    curve: &NurbsCurve,
    parameter: f64,
) -> io::Result<[f64; 3]> {
    if curve.closed || curve.periodic || curve.rational || curve.vertex_dimension != 3 {
        return Err(invalid("unsupported NURBS endpoint extension profile"));
    }

    let degree = usize::from(curve.degree);
    let count = usize::try_from(curve.control_vertex_count)
        .map_err(|_| invalid("invalid NURBS vertex count"))?;
    let knot_count = count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| invalid("knot total overflow"))?;
    let clamped_multiplicity = u16::try_from(degree + 1)
        .map_err(|_| invalid("unsupported NURBS endpoint extension degree"))?;
    if degree == 0
        || count <= degree
        || count > 1_048_576
        || curve.knots.len() != curve.knot_multiplicities.len()
        || curve.knot_multiplicities.first().copied() != Some(clamped_multiplicity)
        || curve.knot_multiplicities.last().copied() != Some(clamped_multiplicity)
    {
        return Err(invalid("unsupported NURBS endpoint extension profile"));
    }

    let mut knots = Vec::with_capacity(knot_count);
    for (&knot, &multiplicity) in curve.knots.iter().zip(&curve.knot_multiplicities) {
        let next = knots
            .len()
            .checked_add(usize::from(multiplicity))
            .ok_or_else(|| invalid("knot count overflow"))?;
        if next > knot_count {
            return Err(invalid("knot multiplicity sum exceeds expected total"));
        }
        knots.resize(next, knot);
    }
    if knots.len() != knot_count {
        return Err(invalid("invalid NURBS knot total"));
    }
    let start = knots[degree];
    let end = knots[count];
    if !parameter.is_finite() {
        return Err(invalid("nonfinite NURBS parameter"));
    }
    if (start..=end).contains(&parameter) {
        return evaluate_mapped_nurbs_curve_candidate(curve, parameter);
    }

    let at_start = parameter < start;
    let endpoint_parameter = if at_start { start } else { end };
    let endpoint = evaluate_mapped_nurbs_curve_candidate(curve, endpoint_parameter)?;
    let (edge_index, adjacent_index, denominator) = if at_start {
        (1, 0, knots[degree + 1] - knots[1])
    } else {
        (
            count - 1,
            count - 2,
            knots[count + degree - 1] - knots[count - 1],
        )
    };
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(invalid("invalid clamped NURBS endpoint interval"));
    }
    let parameter_delta = parameter - endpoint_parameter;
    if !parameter_delta.is_finite() {
        return Err(invalid("nonfinite NURBS endpoint extension"));
    }
    let mut result = endpoint;
    for (axis, result_component) in result.iter_mut().enumerate() {
        let control_delta =
            curve.control_vertices[edge_index][axis] - curve.control_vertices[adjacent_index][axis];
        let tangent = control_delta / denominator * f64::from(curve.degree);
        let extension = parameter_delta * tangent;
        *result_component += extension;
        if !control_delta.is_finite()
            || !tangent.is_finite()
            || !extension.is_finite()
            || !result_component.is_finite()
        {
            return Err(invalid("nonfinite NURBS endpoint extension"));
        }
    }
    Ok(result)
}

/// Evaluates one mapped 2D Parasolid NURBS curve in its raw frame.
///
/// # Errors
/// Returns an error when the curve layout or parameter is invalid or unsupported.
pub fn evaluate_mapped_nurbs_curve_candidate_2d(
    curve: &NurbsCurve,
    parameter: f64,
) -> io::Result<[f64; 2]> {
    evaluate_mapped_nurbs_curve_inner(curve, parameter, 2)?
        .try_into()
        .map_err(|_| invalid("unsupported NURBS layout"))
}

fn evaluate_mapped_nurbs_curve_inner(
    curve: &NurbsCurve,
    parameter: f64,
    output_dimension: usize,
) -> io::Result<Vec<f64>> {
    if curve.periodic && !curve.closed {
        return Err(invalid("periodic NURBS curve is not closed"));
    }
    let count = usize::try_from(curve.control_vertex_count)
        .map_err(|_| invalid("invalid NURBS vertex count"))?;
    if curve.control_vertices.len() != count
        || curve
            .control_vertices
            .iter()
            .any(|vertex| vertex.len() != usize::from(curve.vertex_dimension))
    {
        return Err(invalid("NURBS component length mismatch"));
    }
    evaluate_nurbs_curve_values_candidate(
        usize::from(curve.degree),
        curve.rational,
        curve.periodic,
        count,
        usize::from(curve.vertex_dimension),
        &curve.control_vertices,
        &curve.knot_multiplicities,
        &curve.knots,
        parameter,
        output_dimension,
    )
}

/// Evaluates raw mapped NURBS values in the supplied dimension and parameter frame.
///
/// Rational control points are interpreted as homogeneous values. Periodic
/// curves require the XT repeated-control and matching-knot-extension form.
///
/// # Errors
/// Returns an error when the NURBS data, periodic representation, or parameter is invalid.
#[allow(
    clippy::too_many_arguments,
    reason = "This public evaluator preserves its established parameter-based API for compatibility."
)]
pub fn evaluate_nurbs_curve_values_candidate(
    degree: usize,
    rational: bool,
    periodic: bool,
    count: usize,
    dim: usize,
    vertices: &[Vec<f64>],
    multiplicities: &[u16],
    distinct_knots: &[f64],
    parameter: f64,
    output_dimension: usize,
) -> io::Result<Vec<f64>> {
    let expected_dimension = if rational {
        output_dimension
            .checked_add(1)
            .ok_or_else(|| invalid("NURBS output dimension overflow"))?
    } else {
        output_dimension
    };
    if degree == 0
        || count <= degree
        || count > MAX_CONTROLS
        || distinct_knots.is_empty()
        || dim != expected_dimension
        || vertices.len() != count
        || vertices.iter().any(|vertex| vertex.len() != dim)
        || multiplicities.len() != distinct_knots.len()
    {
        return Err(invalid("unsupported NURBS layout"));
    }
    let mut knots = Vec::new();
    let mut last = f64::NEG_INFINITY;
    let distinct_count = distinct_knots.len();
    for (index, (&value, &multiplicity)) in distinct_knots.iter().zip(multiplicities).enumerate() {
        let mult = usize::from(multiplicity);
        if !value.is_finite() || value <= last {
            return Err(invalid("distinct knots are not finite and increasing"));
        }
        if mult == 0 {
            return Err(invalid("invalid knot multiplicity"));
        }
        last = value;
        let next = knots
            .len()
            .checked_add(mult)
            .ok_or_else(|| invalid("knot count overflow"))?;
        let max_mult = if index == 0 || index + 1 == distinct_count {
            degree + 1
        } else {
            degree
        };
        if mult > max_mult || next > count + degree + 1 {
            return Err(invalid("knot multiplicity sum exceeds expected total"));
        }
        knots.resize(next, value);
    }
    if knots.len()
        != count
            .checked_add(degree)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| invalid("knot total overflow"))?
        || !parameter.is_finite()
    {
        return Err(invalid("invalid NURBS knot total or parameter range"));
    }
    let start = knots[degree];
    let end = knots[count];
    let parameter = if periodic {
        let unique_count = count
            .checked_sub(degree)
            .ok_or_else(|| invalid("invalid periodic NURBS control count"))?;
        if unique_count <= degree {
            return Err(invalid("invalid periodic NURBS control count"));
        }
        let period = end - start;
        if !period.is_finite() || period <= 0.0 {
            return Err(invalid("invalid periodic NURBS knot extension"));
        }
        let check_periodic_continuity = (0..=degree)
            .any(|index| knots[index + unique_count] - knots[index] != period)
            || (0..degree).any(|index| vertices[index] != vertices[unique_count + index]);
        if degree == 1 || check_periodic_continuity {
            validate_periodic_endpoint_continuity(
                degree,
                count,
                &knots,
                vertices,
                rational,
                output_dimension,
            )?;
        }
        let offset = parameter - start;
        let wraps = offset / period;
        if !wraps.is_finite() || wraps.abs() > MAX_PRECISE_PERIODIC_WRAPS {
            return Err(invalid("periodic NURBS wrap count exceeds precise range"));
        }
        let wrapped = start + offset.rem_euclid(period);
        if wrapped >= end { start } else { wrapped }
    } else {
        if parameter < start || parameter > end {
            return Err(invalid("invalid NURBS knot total or parameter range"));
        }
        parameter
    };
    let mut points = Vec::with_capacity(count);
    for vertex in vertices {
        if vertex.iter().any(|v| !v.is_finite()) || (rational && vertex[output_dimension] <= 0.0) {
            return Err(invalid("invalid control vertex or rational weight"));
        }
        let point = if rational {
            vertex.clone()
        } else {
            let mut point = vertex.clone();
            point.push(1.0);
            point
        };
        if point.iter().any(|value| !value.is_finite()) {
            return Err(invalid("nonfinite homogeneous control vertex"));
        }
        points.push(point);
    }
    let span = if !periodic && parameter == knots[count] {
        count - 1
    } else {
        (degree..count)
            .find(|&i| parameter >= knots[i] && parameter < knots[i + 1])
            .ok_or_else(|| invalid("parameter has no knot span"))?
    };
    let mut d = points[span - degree..=span].to_vec();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = span - degree + j;
            let den = knots[i + degree + 1 - r] - knots[i];
            if den == 0.0 || !den.is_finite() {
                return Err(invalid("zero NURBS denominator"));
            }
            let alpha = (parameter - knots[i]) / den;
            if !alpha.is_finite() {
                return Err(invalid("nonfinite NURBS interpolation factor"));
            }
            for k in 0..points[0].len() {
                d[j][k] = (1.0 - alpha) * d[j - 1][k] + alpha * d[j][k];
                if !d[j][k].is_finite() {
                    return Err(invalid("nonfinite NURBS intermediate"));
                }
            }
        }
    }
    let p = &d[degree];
    let divisor = if rational { p[output_dimension] } else { 1.0 };
    if divisor == 0.0 || !divisor.is_finite() {
        return Err(invalid("nonfinite NURBS result or zero weight"));
    }
    let result: Vec<f64> = (0..output_dimension).map(|i| p[i] / divisor).collect();
    if result.iter().any(|v| !v.is_finite()) {
        return Err(invalid("nonfinite NURBS result or zero weight"));
    }
    Ok(result)
}

fn validate_periodic_endpoint_continuity(
    degree: usize,
    count: usize,
    knots: &[f64],
    vertices: &[Vec<f64>],
    rational: bool,
    output_dimension: usize,
) -> io::Result<()> {
    let degree_as_f64 = f64::from(
        u32::try_from(degree).map_err(|_| invalid("NURBS degree exceeds supported range"))?,
    );
    let mut points = Vec::with_capacity(count);
    for vertex in vertices {
        if vertex.iter().any(|value| !value.is_finite())
            || (rational && vertex[output_dimension] <= 0.0)
        {
            return Err(invalid("invalid control vertex or rational weight"));
        }
        let mut point = vertex.clone();
        if !rational {
            point.push(1.0);
        }
        points.push(point);
    }

    let start = evaluate_homogeneous(&points, knots, degree, degree, knots[degree])?;
    let end = evaluate_homogeneous(&points, knots, degree, count - 1, knots[count])?;
    if !same_cartesian_point(&start, &end, rational, output_dimension)? {
        return Err(invalid(
            "periodic NURBS endpoints exceed XT linear resolution",
        ));
    }

    let derivative_degree = degree - 1;
    let derivative_points: Vec<Vec<f64>> = points
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let denominator = knots[index + degree + 1] - knots[index + 1];
            if denominator <= 0.0 || !denominator.is_finite() {
                return Err(invalid("invalid periodic NURBS endpoint tangent interval"));
            }
            let derivative: Vec<f64> = pair[0]
                .iter()
                .zip(&pair[1])
                .map(|(left, right)| degree_as_f64 * (right - left) / denominator)
                .collect();
            if derivative.iter().any(|value| !value.is_finite()) {
                return Err(invalid("nonfinite periodic NURBS endpoint tangent"));
            }
            Ok(derivative)
        })
        .collect::<io::Result<_>>()?;
    let derivative_knots = &knots[1..knots.len() - 1];
    let start_derivative = evaluate_homogeneous(
        &derivative_points,
        derivative_knots,
        derivative_degree,
        derivative_degree,
        knots[degree],
    )?;
    let end_derivative = evaluate_homogeneous(
        &derivative_points,
        derivative_knots,
        derivative_degree,
        count - 2,
        knots[count],
    )?;
    let tangent = |point: &[f64], derivative: &[f64]| -> io::Result<Vec<f64>> {
        let weight = point[output_dimension];
        let weight_derivative = derivative[output_dimension];
        let tangent: Vec<f64> = (0..output_dimension)
            .map(|axis| derivative[axis] * weight - point[axis] * weight_derivative)
            .collect();
        if tangent.iter().any(|value| !value.is_finite()) {
            return Err(invalid("nonfinite periodic NURBS endpoint tangent"));
        }
        Ok(tangent)
    };
    let start_tangent = tangent(&start, &start_derivative)?;
    let end_tangent = tangent(&end, &end_derivative)?;
    if !same_positive_direction(&start_tangent, &end_tangent) {
        return Err(invalid(
            "periodic NURBS tangents exceed XT angular resolution",
        ));
    }
    Ok(())
}

fn evaluate_homogeneous(
    points: &[Vec<f64>],
    knots: &[f64],
    degree: usize,
    span: usize,
    parameter: f64,
) -> io::Result<Vec<f64>> {
    let mut values = points[span - degree..=span].to_vec();
    for order in 1..=degree {
        for index in (order..=degree).rev() {
            let knot_index = span - degree + index;
            let denominator = knots[knot_index + degree + 1 - order] - knots[knot_index];
            if denominator <= 0.0 || !denominator.is_finite() {
                return Err(invalid("invalid periodic NURBS endpoint knot interval"));
            }
            let alpha = (parameter - knots[knot_index]) / denominator;
            if !alpha.is_finite() {
                return Err(invalid("nonfinite periodic NURBS endpoint interpolation"));
            }
            for axis in 0..values[0].len() {
                values[index][axis] =
                    (1.0 - alpha) * values[index - 1][axis] + alpha * values[index][axis];
                if !values[index][axis].is_finite() {
                    return Err(invalid("nonfinite periodic NURBS endpoint value"));
                }
            }
        }
    }
    Ok(values[degree].clone())
}

fn same_cartesian_point(
    left: &[f64],
    right: &[f64],
    rational: bool,
    output_dimension: usize,
) -> io::Result<bool> {
    let left_weight = if rational {
        left[output_dimension]
    } else {
        1.0
    };
    let right_weight = if rational {
        right[output_dimension]
    } else {
        1.0
    };
    if left_weight <= 0.0 || right_weight <= 0.0 {
        return Err(invalid("invalid periodic NURBS endpoint weight"));
    }
    let mut distance = 0.0_f64;
    for axis in 0..output_dimension {
        let left_value = left[axis] / left_weight;
        let right_value = right[axis] / right_weight;
        if !left_value.is_finite() || !right_value.is_finite() {
            return Err(invalid("nonfinite periodic NURBS endpoint"));
        }
        distance = distance.hypot(left_value - right_value);
    }
    Ok(distance <= 1.0e-8)
}

fn same_positive_direction(left: &[f64], right: &[f64]) -> bool {
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let left_scale = left
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    let right_scale = right
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if left_scale == 0.0
        || right_scale == 0.0
        || !left_scale.is_finite()
        || !right_scale.is_finite()
    {
        return false;
    }
    let left_norm = left
        .iter()
        .map(|value| (value / left_scale).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = right
        .iter()
        .map(|value| (value / right_scale).powi(2))
        .sum::<f64>()
        .sqrt();
    let left_unit: Vec<f64> = left
        .iter()
        .map(|value| value / left_scale / left_norm)
        .collect();
    let right_unit: Vec<f64> = right
        .iter()
        .map(|value| value / right_scale / right_norm)
        .collect();
    let half_chord = left_unit
        .iter()
        .zip(&right_unit)
        .fold(0.0_f64, |norm, (left, right)| {
            norm.hypot((left - right) * 0.5)
        });
    2.0 * half_chord.asin() <= 1.0e-11
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_mapped_nurbs_curve_candidate,
        evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension,
    };
    use crate::brep::NurbsCurve;

    fn line_curve() -> NurbsCurve {
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
                vec![0.0, 2.0, -1.0],
                vec![4.0, 4.0, 1.0],
                vec![8.0, 6.0, 3.0],
            ],
            knots: vec![2.0, 6.0],
            knot_multiplicities: vec![3, 3],
            sources: vec![],
        }
    }

    fn periodic_curve(rational: bool) -> NurbsCurve {
        let planar = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
        let mut control_vertices: Vec<Vec<f64>> = planar
            .iter()
            .chain(&planar[..2])
            .enumerate()
            .map(|(index, point)| {
                let weight = if rational {
                    1.0 + index as f64 / 4.0
                } else {
                    1.0
                };
                let mut vertex = vec![point[0] * weight, point[1] * weight, 0.0];
                if rational {
                    vertex.push(weight);
                }
                vertex
            })
            .collect();
        if rational {
            // The duplicate suffix must repeat the homogeneous control vertices.
            control_vertices[4] = control_vertices[0].clone();
            control_vertices[5] = control_vertices[1].clone();
        }
        NurbsCurve {
            degree: 2,
            control_vertex_count: 6,
            vertex_dimension: if rational { 4 } else { 3 },
            knot_type: 0,
            periodic: true,
            closed: true,
            rational,
            curve_form: 0,
            control_vertices,
            knots: vec![-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            knot_multiplicities: vec![1; 9],
            sources: vec![],
        }
    }

    fn periodic_curve_with_exact_tangent_join() -> NurbsCurve {
        NurbsCurve {
            degree: 3,
            control_vertex_count: 8,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: true,
            closed: true,
            rational: false,
            curve_form: 0,
            control_vertices: vec![
                vec![1.0, 0.0, 0.0],
                vec![0.0, 1.0, 0.0],
                vec![-1.0, 0.0, 0.0],
                vec![0.0, -1.0, 0.0],
                vec![0.0, 0.0, 0.0],
                vec![1.0, 2.0, 0.0],
                vec![0.0, 0.0, 0.0],
                vec![-1.0, 2.0, 0.0],
            ],
            knots: (-3..=8).map(f64::from).collect(),
            knot_multiplicities: vec![1; 12],
            sources: vec![],
        }
    }

    fn periodic_degree_one_curve(control_vertices: Vec<Vec<f64>>) -> NurbsCurve {
        NurbsCurve {
            degree: 1,
            control_vertex_count: 5,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: true,
            closed: true,
            rational: false,
            curve_form: 0,
            control_vertices,
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            knot_multiplicities: vec![1; 7],
            sources: vec![],
        }
    }

    #[test]
    fn periodic_repeated_control_degree_one_validates_tangent_continuity() {
        // The square's closure point is repeated, but its seam directions differ.
        let square_seam = periodic_degree_one_curve(vec![
            vec![0.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![1.0, 1.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 0.0],
        ]);
        assert!(evaluate_mapped_nurbs_curve_candidate(&square_seam, 2.0).is_err());

        let matching_tangent = periodic_degree_one_curve(vec![
            vec![0.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![2.0, 0.0, 0.0],
            vec![-1.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0],
        ]);
        assert!(evaluate_mapped_nurbs_curve_candidate(&matching_tangent, 2.0).is_ok());
    }

    fn clamped_periodic_curve(rational: bool) -> NurbsCurve {
        let control_vertices = if rational {
            vec![
                vec![0.0, 0.0, 0.0, 1.0],
                vec![2.0, 0.0, 0.0, 2.0],
                vec![0.0, 2.0, 0.0, 1.0],
                vec![-3.0, 0.0, 0.0, 3.0],
                vec![0.0, 0.0, 0.0, 1.0],
            ]
        } else {
            vec![
                vec![0.0, 0.0, 0.0],
                vec![1.0, 0.0, 0.0],
                vec![0.0, 2.0, 0.0],
                vec![-1.0, 0.0, 0.0],
                vec![0.0, 0.0, 0.0],
            ]
        };
        NurbsCurve {
            degree: 2,
            control_vertex_count: 5,
            vertex_dimension: if rational { 4 } else { 3 },
            knot_type: 0,
            periodic: true,
            closed: true,
            rational,
            curve_form: 0,
            control_vertices,
            knots: vec![0.0, 1.0, 3.0],
            knot_multiplicities: vec![3, 2, 3],
            sources: vec![],
        }
    }

    #[test]
    fn periodic_curve_wraps_across_positive_and_negative_periods() {
        for rational in [false, true] {
            let curve = periodic_curve(rational);
            let at = evaluate_mapped_nurbs_curve_candidate(&curve, 0.5).unwrap();
            assert_eq!(
                at,
                evaluate_mapped_nurbs_curve_candidate(&curve, 4.5).unwrap()
            );
            assert_eq!(
                at,
                evaluate_mapped_nurbs_curve_candidate(&curve, -3.5).unwrap()
            );
            assert_eq!(
                evaluate_mapped_nurbs_curve_candidate(&curve, 0.0).unwrap(),
                evaluate_mapped_nurbs_curve_candidate(&curve, 4.0).unwrap()
            );
            if rational {
                assert_eq!(
                    evaluate_mapped_nurbs_curve_candidate(&curve, 0.0).unwrap(),
                    [4.0 / 9.0, 5.0 / 9.0, 0.0]
                );
            }
        }
    }

    #[test]
    fn periodic_curve_accepts_exact_tangent_join_without_repeated_controls() {
        let curve = periodic_curve_with_exact_tangent_join();
        assert_ne!(curve.control_vertices[0..3], curve.control_vertices[5..8]);
        let at = evaluate_mapped_nurbs_curve_candidate(&curve, 1.0).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_curve_candidate(&curve, 6.0).unwrap()
        );
        assert_eq!(
            at,
            evaluate_mapped_nurbs_curve_candidate(&curve, -4.0).unwrap()
        );
    }

    #[test]
    fn periodic_clamped_endpoints_with_exact_tangents_and_internal_zero_span() {
        for rational in [false, true] {
            let curve = clamped_periodic_curve(rational);
            let start = evaluate_mapped_nurbs_curve_candidate(&curve, 0.0).unwrap();
            assert_eq!(
                start,
                evaluate_mapped_nurbs_curve_candidate(&curve, 3.0).unwrap()
            );
            assert!(evaluate_mapped_nurbs_curve_candidate(&curve, 1.0).is_ok());
        }

        let mut opposite = clamped_periodic_curve(false);
        opposite.control_vertices[3] = vec![1.0, 0.0, 0.0];
        assert!(evaluate_mapped_nurbs_curve_candidate(&opposite, 1.0).is_err());

        let mut zero = clamped_periodic_curve(false);
        zero.control_vertices[3] = zero.control_vertices[4].clone();
        assert!(evaluate_mapped_nurbs_curve_candidate(&zero, 1.0).is_err());
    }

    #[test]
    fn periodic_curve_honors_xt_default_seam_resolutions() {
        let mut within_both = clamped_periodic_curve(false);
        within_both.control_vertices[3][1] = 5.0e-9 - 1.0e-12;
        within_both.control_vertices[4][1] = 5.0e-9;
        assert!(evaluate_mapped_nurbs_curve_candidate(&within_both, 1.0).is_ok());

        let mut endpoint_exceeds = clamped_periodic_curve(false);
        endpoint_exceeds.control_vertices[3][1] = 2.0e-8 - 1.0e-12;
        endpoint_exceeds.control_vertices[4][1] = 2.0e-8;
        assert!(evaluate_mapped_nurbs_curve_candidate(&endpoint_exceeds, 1.0).is_err());

        let mut tangent_exceeds = clamped_periodic_curve(false);
        tangent_exceeds.control_vertices[3][1] = -5.0e-11;
        assert!(evaluate_mapped_nurbs_curve_candidate(&tangent_exceeds, 1.0).is_err());
    }

    #[test]
    fn rational_output_dimension_overflow_is_rejected() {
        assert!(
            super::evaluate_nurbs_curve_values_candidate(
                2,
                true,
                false,
                3,
                0,
                &[],
                &[],
                &[],
                0.0,
                usize::MAX,
            )
            .is_err()
        );
    }

    #[test]
    fn periodic_curve_rejects_invalid_period_representation_and_unbounded_wraps() {
        let mut curve = periodic_curve(false);
        curve.control_vertices[4][0] += 1.0;
        assert!(evaluate_mapped_nurbs_curve_candidate(&curve, 0.5).is_err());

        let mut curve = periodic_curve(false);
        curve.knots[0] = -3.0;
        assert_eq!(
            evaluate_mapped_nurbs_curve_candidate(&curve, 0.5).unwrap(),
            evaluate_mapped_nurbs_curve_candidate(&curve, 4.5).unwrap()
        );

        let mut curve = periodic_curve(false);
        curve.closed = false;
        assert!(evaluate_mapped_nurbs_curve_candidate(&curve, 0.5).is_err());

        let curve = periodic_curve(false);
        assert!(evaluate_mapped_nurbs_curve_candidate(&curve, f64::MAX).is_err());
    }

    #[test]
    fn extension_uses_stored_parameter_and_endpoint_tangents_at_both_ends() {
        let curve = line_curve();
        assert_eq!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .unwrap(),
            [-4.0, 0.0, -3.0]
        );
        assert_eq!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 8.0)
                .unwrap(),
            [12.0, 8.0, 5.0]
        );
    }

    #[test]
    fn extension_opt_in_preserves_strict_default_range() {
        let curve = line_curve();
        assert_eq!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 4.0)
                .unwrap(),
            evaluate_mapped_nurbs_curve_candidate(&curve, 4.0).unwrap()
        );
        assert!(evaluate_mapped_nurbs_curve_candidate(&curve, 0.0).is_err());
    }

    #[test]
    fn extension_rejects_unsupported_profiles_and_nonfinite_arithmetic() {
        let mut curve = line_curve();
        curve.closed = true;
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );

        let mut curve = line_curve();
        curve.periodic = true;
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );

        let mut curve = line_curve();
        curve.rational = true;
        curve.vertex_dimension = 4;
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );

        let mut curve = line_curve();
        curve.knot_multiplicities = vec![1, 3];
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );

        let mut curve = line_curve();
        curve.knots = vec![2.0, 3.0, 6.0];
        curve.knot_multiplicities = vec![3, u16::MAX, 3];
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );

        let mut curve = line_curve();
        curve.control_vertices = vec![
            vec![-1e308, 0.0, 0.0],
            vec![1e308, 0.0, 0.0],
            vec![1e308, 0.0, 0.0],
        ];
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(&curve, 0.0)
                .is_err()
        );
        assert!(
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(
                &line_curve(),
                f64::INFINITY
            )
            .is_err()
        );
    }
}
