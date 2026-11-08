//! Bounded evaluation of mapped NURBS surface data.

use std::io;

use super::NurbsSurface;

const MAX_CONTROLS: usize = 1_048_576;
const MAX_EVALUATION_WORK: usize = 16_777_216;
const MAX_PRECISE_PERIODIC_WRAPS: f64 = 4_503_599_627_370_496.0;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn expand_knots(
    knots: &[f64],
    multiplicities: &[u16],
    control_count: usize,
    degree: usize,
    periodic: bool,
) -> io::Result<Vec<f64>> {
    if knots.is_empty() || knots.len() != multiplicities.len() || knots.len() > MAX_CONTROLS {
        return Err(invalid(
            "NURBS surface knot count mismatch or limit exceeded",
        ));
    }
    let expected = control_count
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| invalid("NURBS surface knot total overflow"))?;
    let mut expanded = Vec::new();
    expanded
        .try_reserve_exact(expected)
        .map_err(|_| invalid("NURBS surface knot allocation failed"))?;
    let mut previous = f64::NEG_INFINITY;
    for (index, (&knot, &multiplicity)) in knots.iter().zip(multiplicities).enumerate() {
        let multiplicity = usize::from(multiplicity);
        let endpoint = index == 0 || index + 1 == knots.len();
        if !knot.is_finite()
            || knot <= previous
            || multiplicity == 0
            || !periodic && endpoint && multiplicity != degree + 1
            || !endpoint && multiplicity > degree
            || periodic && endpoint && multiplicity > degree + 1
        {
            return Err(invalid("invalid or unclamped NURBS surface knots"));
        }
        previous = knot;
        let size = expanded
            .len()
            .checked_add(multiplicity)
            .ok_or_else(|| invalid("NURBS surface knot total overflow"))?;
        if size > expected {
            return Err(invalid("NURBS surface expanded knot limit exceeded"));
        }
        expanded.resize(size, knot);
    }
    if expanded.len() != expected {
        return Err(invalid("NURBS surface knot total mismatch"));
    }
    Ok(expanded)
}

#[allow(
    clippy::float_cmp,
    reason = "The final knot span is selected only by exact endpoint equality, preserving IEEE comparison behavior."
)]
fn evaluate_curve(
    points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    parameter: f64,
) -> io::Result<[f64; 4]> {
    let count = points.len();
    let span = if parameter == knots[count] {
        count - 1
    } else {
        (degree..count)
            .find(|&index| parameter >= knots[index] && parameter < knots[index + 1])
            .ok_or_else(|| invalid("NURBS surface parameter has no knot span"))?
    };
    let mut values = points[span - degree..=span].to_vec();
    for order in 1..=degree {
        for j in (order..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - order] - knots[index];
            if denominator <= 0.0 || !denominator.is_finite() {
                return Err(invalid("invalid NURBS surface interpolation denominator"));
            }
            let alpha = (parameter - knots[index]) / denominator;
            if !alpha.is_finite() {
                return Err(invalid("nonfinite NURBS surface interpolation factor"));
            }
            for axis in 0..4 {
                values[j][axis] = (1.0 - alpha) * values[j - 1][axis] + alpha * values[j][axis];
            }
            if values[j].iter().any(|value| !value.is_finite()) {
                return Err(invalid("nonfinite NURBS surface intermediate"));
            }
        }
    }
    Ok(values[degree])
}

fn f64_dyadic_parts(value: f64) -> Option<(bool, u64, i32)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if exponent == 0 {
        Some((negative, fraction, -1074))
    } else {
        Some((negative, (1u64 << 52) | fraction, exponent - 1023 - 52))
    }
}

#[allow(
    clippy::cast_possible_wrap,
    reason = "For nonzero u128 mantissas, trailing_zeros is at most 127 and safely fits i32."
)]
fn exact_products_equal(left_a: f64, left_b: f64, right_a: f64, right_b: f64) -> bool {
    let (
        Some((left_sign_a, left_mantissa_a, left_exponent_a)),
        Some((left_sign_b, left_mantissa_b, left_exponent_b)),
        Some((right_sign_a, right_mantissa_a, right_exponent_a)),
        Some((right_sign_b, right_mantissa_b, right_exponent_b)),
    ) = (
        f64_dyadic_parts(left_a),
        f64_dyadic_parts(left_b),
        f64_dyadic_parts(right_a),
        f64_dyadic_parts(right_b),
    )
    else {
        return false;
    };
    let left_mantissa = u128::from(left_mantissa_a) * u128::from(left_mantissa_b);
    let right_mantissa = u128::from(right_mantissa_a) * u128::from(right_mantissa_b);
    if left_mantissa == 0 || right_mantissa == 0 {
        return left_mantissa == 0 && right_mantissa == 0;
    }
    let left_zeroes = left_mantissa.trailing_zeros();
    let right_zeroes = right_mantissa.trailing_zeros();
    (left_sign_a == left_sign_b) == (right_sign_a == right_sign_b)
        && (left_mantissa >> left_zeroes) == (right_mantissa >> right_zeroes)
        && left_exponent_a + left_exponent_b + left_zeroes as i32
            == right_exponent_a + right_exponent_b + right_zeroes as i32
}

fn homogeneous_sequence_matches_scale(
    start: &[[f64; 4]],
    end: &[[f64; 4]],
    start_weight: f64,
    end_weight: f64,
) -> bool {
    start.len() == end.len()
        && start_weight.is_finite()
        && end_weight.is_finite()
        && start_weight > 0.0
        && end_weight > 0.0
        && start.iter().zip(end).all(|(start, end)| {
            (0..4)
                .all(|axis| exact_products_equal(end[axis], start_weight, start[axis], end_weight))
        })
}

#[allow(
    clippy::float_cmp,
    reason = "The error-free subtraction check requires exact equality of the computed roundoff terms."
)]
fn exact_product_difference(a: f64, b: f64, c: f64, d: f64) -> Option<f64> {
    let left = a * b;
    let right = c * d;
    if !left.is_finite()
        || !right.is_finite()
        || !exact_products_equal(left, 1.0, a, b)
        || !exact_products_equal(right, 1.0, c, d)
    {
        return None;
    }

    let difference = left - right;
    let right_virtual = left - difference;
    let left_virtual = difference + right_virtual;
    let right_roundoff = right_virtual - right;
    let left_roundoff = left - left_virtual;
    if !difference.is_finite() || left_roundoff != -right_roundoff {
        return None;
    }
    Some(difference)
}

fn exact_difference(a: f64, b: f64) -> Option<f64> {
    exact_product_difference(a, 1.0, b, 1.0)
}

fn exact_evaluate_curve(
    points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    parameter: f64,
    span: usize,
) -> Option<[f64; 4]> {
    let count = points.len();
    if span < degree
        || span >= count
        || parameter < *knots.get(span)?
        || parameter > *knots.get(span + 1)?
    {
        return None;
    }
    let mut values = points.get(span.checked_sub(degree)?..=span)?.to_vec();
    for order in 1..=degree {
        for j in (order..=degree).rev() {
            let index = span.checked_sub(degree)?.checked_add(j)?;
            let denominator = exact_difference(knots[index + degree + 1 - order], knots[index])?;
            if denominator <= 0.0 {
                return None;
            }
            let numerator = exact_difference(parameter, knots[index])?;
            let alpha = numerator / denominator;
            if !alpha.is_finite() || !exact_products_equal(alpha, denominator, numerator, 1.0) {
                return None;
            }
            let complement = exact_difference(1.0, alpha)?;
            for axis in 0..4 {
                let left = exact_product_difference(complement, values[j - 1][axis], 0.0, 0.0)?;
                let right = exact_product_difference(alpha, values[j][axis], 0.0, 0.0)?;
                values[j][axis] = exact_sum(left, right)?;
            }
        }
    }
    Some(values[degree])
}

#[allow(
    clippy::float_cmp,
    reason = "Exact knot equality determines the span at repeated endpoint knots without changing knot ownership."
)]
fn exact_endpoint_curve_value(
    points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    at_end: bool,
) -> Option<[f64; 4]> {
    let count = points.len();
    let parameter = *knots.get(if at_end { count } else { degree })?;
    let span = if at_end {
        (degree..count).find(|&span| knots[span] < parameter && knots[span + 1] == parameter)?
    } else {
        (degree..count)
            .rev()
            .find(|&span| knots[span] == parameter && knots[span + 1] > parameter)?
    };
    exact_evaluate_curve(points, knots, degree, parameter, span)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "The only production callers pass degrees sourced from NurbsSurface u16 degree fields; test callers use small literals."
)]
fn exact_endpoint_curve_jet(
    points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    at_end: bool,
) -> Option<([f64; 4], [f64; 4])> {
    if degree == 0 || knots.len() != points.len().checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let point = exact_endpoint_curve_value(points, knots, degree, at_end)?;
    let mut derivatives = Vec::new();
    derivatives
        .try_reserve_exact(points.len().checked_sub(1)?)
        .ok()?;
    for (index, pair) in points.windows(2).enumerate() {
        let denominator = exact_difference(knots[index + degree + 1], knots[index + 1])?;
        if denominator <= 0.0 {
            return None;
        }
        let mut derivative = [0.0; 4];
        for (axis, derivative_component) in derivative.iter_mut().enumerate() {
            let delta = exact_difference(pair[1][axis], pair[0][axis])?;
            let numerator = exact_product_difference(degree as f64, delta, 0.0, 0.0)?;
            *derivative_component = numerator / denominator;
            if !derivative_component.is_finite()
                || !exact_products_equal(*derivative_component, denominator, numerator, 1.0)
            {
                return None;
            }
        }
        derivatives.push(derivative);
    }
    let derivative_knots = knots.get(1..knots.len().checked_sub(1)?)?;
    let derivative =
        exact_endpoint_curve_value(&derivatives, derivative_knots, degree - 1, at_end)?;
    Some((point, derivative))
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Callers bound the numerator to 2*u16::MAX+1 and denominator to its next power of two, both exactly representable in f64."
)]
fn exact_dyadic_sample(start: f64, end: f64, numerator: usize, denominator: usize) -> Option<f64> {
    let width = exact_difference(end, start)?;
    let scaled = exact_product_difference(width, numerator as f64, 0.0, 0.0)?;
    let offset = scaled / denominator as f64;
    if !offset.is_finite() || !exact_products_equal(offset, denominator as f64, scaled, 1.0) {
        return None;
    }
    let sample = exact_sum(start, offset)?;
    (sample > start && sample < end).then_some(sample)
}

fn validate_sampled_projective_seam(
    start: &[[f64; 4]],
    end: &[[f64; 4]],
    start_derivative: &[[f64; 4]],
    end_derivative: &[[f64; 4]],
    transverse_knots: &[f64],
    transverse_degree: usize,
) -> bool {
    if start.len() <= transverse_degree
        || start.len() != end.len()
        || start.len() != start_derivative.len()
        || start.len() != end_derivative.len()
        || transverse_knots.len() != start.len() + transverse_degree + 1
    {
        return false;
    }
    let Some(sample_count) = transverse_degree
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
    else {
        return false;
    };
    let denominator = sample_count
        .checked_add(1)
        .and_then(usize::checked_next_power_of_two);
    let Some(denominator) = denominator else {
        return false;
    };
    let spans = (transverse_degree..start.len())
        .filter(|&span| transverse_knots[span] < transverse_knots[span + 1]);
    let mut found_span = false;
    for span in spans {
        found_span = true;
        let (low, high) = (transverse_knots[span], transverse_knots[span + 1]);
        for numerator in 1..=sample_count {
            let Some(parameter) = exact_dyadic_sample(low, high, numerator, denominator) else {
                return false;
            };
            let (Some(s), Some(e), Some(ds), Some(de)) = (
                exact_evaluate_curve(start, transverse_knots, transverse_degree, parameter, span),
                exact_evaluate_curve(end, transverse_knots, transverse_degree, parameter, span),
                exact_evaluate_curve(
                    start_derivative,
                    transverse_knots,
                    transverse_degree,
                    parameter,
                    span,
                ),
                exact_evaluate_curve(
                    end_derivative,
                    transverse_knots,
                    transverse_degree,
                    parameter,
                    span,
                ),
            ) else {
                return false;
            };
            if [s[3], e[3]]
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
                || !exact_wedges_equal(e, s, [0.0; 4], [0.0; 4])
                || !exact_wedges_equal(s, de, e, ds)
            {
                return false;
            }
        }
    }
    found_span
}

fn sampled_seam_work(control_count: usize, degree: usize) -> Option<usize> {
    let sample_work = degree
        .checked_add(1)?
        .checked_mul(degree.checked_add(1)?)?
        .checked_mul(32)?
        .checked_add(256)?;
    control_count
        .checked_mul(degree.checked_mul(2)?.checked_add(1)?)?
        .checked_mul(sample_work)
}

#[derive(Clone, Copy)]
struct SurfaceControlGrid<'a> {
    homogeneous: &'a [[f64; 4]],
    u_count: usize,
    v_count: usize,
}

#[derive(Clone, Copy)]
struct SeamEndpointData<'a> {
    start_points: &'a [[f64; 4]],
    end_points: &'a [[f64; 4]],
    start_derivatives: &'a [[f64; 4]],
    end_derivatives: &'a [[f64; 4]],
}

#[derive(Clone, Copy)]
struct AxisPeriodicity {
    periodic: bool,
    validate_seam: bool,
}

#[allow(
    clippy::float_cmp,
    reason = "This predicate proves the degree-one seam data is exactly the raw endpoint and derivative data."
)]
fn degree_one_periodic_data_is_exact(
    controls: SurfaceControlGrid<'_>,
    knots: &[f64],
    periodic_u: bool,
    endpoints: SeamEndpointData<'_>,
) -> bool {
    let (curve_count, transverse_count) = if periodic_u {
        (controls.u_count, controls.v_count)
    } else {
        (controls.v_count, controls.u_count)
    };
    if curve_count < 2
        || endpoints.start_points.len() != transverse_count
        || endpoints.end_points.len() != transverse_count
        || endpoints.start_derivatives.len() != transverse_count
        || endpoints.end_derivatives.len() != transverse_count
    {
        return false;
    }
    let (start_denominator, end_denominator) = (
        knots[2] - knots[1],
        knots[curve_count] - knots[curve_count - 1],
    );
    let (Some(exact_start_denominator), Some(exact_end_denominator)) = (
        exact_difference(knots[2], knots[1]),
        exact_difference(knots[curve_count], knots[curve_count - 1]),
    ) else {
        return false;
    };
    if exact_start_denominator != start_denominator
        || exact_end_denominator != end_denominator
        || start_denominator <= 0.0
        || end_denominator <= 0.0
    {
        return false;
    }
    let (start_scale, end_scale) = (1.0 / start_denominator, 1.0 / end_denominator);
    if !exact_products_equal(start_scale, start_denominator, 1.0, 1.0)
        || !exact_products_equal(end_scale, end_denominator, 1.0, 1.0)
    {
        return false;
    }

    for transverse in 0..transverse_count {
        let control = |index: usize| {
            if periodic_u {
                controls.homogeneous[index * controls.v_count + transverse]
            } else {
                controls.homogeneous[transverse * controls.v_count + index]
            }
        };
        let (first, next, previous, last) = (
            control(0),
            control(1),
            control(curve_count - 2),
            control(curve_count - 1),
        );
        if endpoints.start_points[transverse] != first || endpoints.end_points[transverse] != last {
            return false;
        }
        for axis in 0..4 {
            let (Some(start_delta), Some(end_delta)) = (
                exact_difference(next[axis], first[axis]),
                exact_difference(last[axis], previous[axis]),
            ) else {
                return false;
            };
            if exact_product_difference(
                start_delta,
                start_scale,
                endpoints.start_derivatives[transverse][axis],
                1.0,
            ) != Some(0.0)
                || exact_product_difference(
                    end_delta,
                    end_scale,
                    endpoints.end_derivatives[transverse][axis],
                    1.0,
                ) != Some(0.0)
            {
                return false;
            }
        }
    }
    true
}

#[allow(
    clippy::float_cmp,
    reason = "Clamped endpoint multiplicity and homogeneous control equality are exact schema conditions here."
)]
#[allow(
    clippy::cast_precision_loss,
    reason = "The only caller supplies a degree originating from NurbsSurface's u16 degree fields."
)]
fn clamped_periodic_data_is_exact(
    controls: SurfaceControlGrid<'_>,
    knots: &[f64],
    degree: usize,
    periodic_u: bool,
    endpoints: SeamEndpointData<'_>,
) -> bool {
    // shortcut: clamped endpoint jets only; upgrade with checked knot insertion.
    let (curve_count, transverse_count) = if periodic_u {
        (controls.u_count, controls.v_count)
    } else {
        (controls.v_count, controls.u_count)
    };
    if degree <= 1
        || curve_count <= degree
        || knots.len() != curve_count + degree + 1
        || endpoints.start_points.len() != transverse_count
        || endpoints.end_points.len() != transverse_count
        || endpoints.start_derivatives.len() != transverse_count
        || endpoints.end_derivatives.len() != transverse_count
    {
        return false;
    }
    let start = knots[degree];
    let end = knots[curve_count];
    if knots[..=degree].iter().any(|&knot| knot != start)
        || knots[curve_count..].iter().any(|&knot| knot != end)
    {
        return false;
    }

    let Some(start_denominator) = exact_difference(knots[degree + 1], knots[1]) else {
        return false;
    };
    let Some(end_denominator) =
        exact_difference(knots[curve_count + degree - 1], knots[curve_count - 1])
    else {
        return false;
    };
    if start_denominator <= 0.0 || end_denominator <= 0.0 {
        return false;
    }
    let degree = degree as f64;
    let (start_scale, end_scale) = (degree / start_denominator, degree / end_denominator);
    if !start_scale.is_finite()
        || !end_scale.is_finite()
        || !exact_products_equal(start_scale, start_denominator, degree, 1.0)
        || !exact_products_equal(end_scale, end_denominator, degree, 1.0)
    {
        return false;
    }

    for transverse in 0..transverse_count {
        let control = |index: usize| {
            if periodic_u {
                controls.homogeneous[index * controls.v_count + transverse]
            } else {
                controls.homogeneous[transverse * controls.v_count + index]
            }
        };
        let (first, next, previous, last) = (
            control(0),
            control(1),
            control(curve_count - 2),
            control(curve_count - 1),
        );
        if endpoints.start_points[transverse] != first || endpoints.end_points[transverse] != last {
            return false;
        }
        for axis in 0..4 {
            let (Some(start_delta), Some(end_delta)) = (
                exact_difference(next[axis], first[axis]),
                exact_difference(last[axis], previous[axis]),
            ) else {
                return false;
            };
            if exact_product_difference(
                start_delta,
                start_scale,
                endpoints.start_derivatives[transverse][axis],
                1.0,
            ) != Some(0.0)
                || exact_product_difference(
                    end_delta,
                    end_scale,
                    endpoints.end_derivatives[transverse][axis],
                    1.0,
                ) != Some(0.0)
            {
                return false;
            }
        }
    }
    true
}

fn homogeneous_derivatives_match_projective_gauge(
    start_points: &[[f64; 4]],
    start_derivatives: &[[f64; 4]],
    end_derivatives: &[[f64; 4]],
    start_weight: f64,
    end_weight: f64,
) -> bool {
    if start_points.len() != start_derivatives.len() || start_points.len() != end_derivatives.len()
    {
        return false;
    }
    let mut pivot = None;
    let mut nonzero_difference = false;
    for (point, (start, end)) in start_points
        .iter()
        .zip(start_derivatives.iter().zip(end_derivatives))
    {
        for axis in 0..4 {
            let Some(difference) =
                exact_product_difference(end[axis], start_weight, start[axis], end_weight)
            else {
                return false;
            };
            nonzero_difference |= difference != 0.0;
            if difference != 0.0 && point[axis] != 0.0 && pivot.is_none() {
                pivot = Some((difference, point[axis]));
            }
        }
    }
    let Some((pivot_difference, pivot_point)) = pivot else {
        return !nonzero_difference;
    };

    start_points
        .iter()
        .zip(start_derivatives.iter().zip(end_derivatives))
        .all(|(point, (start, end))| {
            (0..4).all(|axis| {
                exact_product_difference(end[axis], start_weight, start[axis], end_weight)
                    .is_some_and(|difference| {
                        exact_products_equal(difference, pivot_point, pivot_difference, point[axis])
                    })
            })
        })
}

fn exact_sum(a: f64, b: f64) -> Option<f64> {
    let sum = a + b;
    if !sum.is_finite() {
        return None;
    }
    let b_virtual = sum - a;
    let a_virtual = sum - b_virtual;
    let error = (a - a_virtual) + (b - b_virtual);
    (error == 0.0).then_some(sum)
}

fn exact_product_terms_sum_to_zero<const N: usize>(
    positive: [(f64, f64); N],
    negative: [(f64, f64); N],
) -> bool {
    let mut sum = 0.0;
    for ((a, b), (c, d)) in positive.into_iter().zip(negative) {
        let Some(difference) = exact_product_difference(a, b, c, d) else {
            return false;
        };
        let Some(next) = exact_sum(sum, difference) else {
            return false;
        };
        sum = next;
    }
    sum == 0.0
}

fn exact_wedges_equal(
    left_a: [f64; 4],
    left_b: [f64; 4],
    right_a: [f64; 4],
    right_b: [f64; 4],
) -> bool {
    (0..4).all(|i| {
        (i + 1..4).all(|j| {
            exact_product_terms_sum_to_zero(
                [(left_a[i], left_b[j]), (right_a[j], right_b[i])],
                [(left_a[j], left_b[i]), (right_a[i], right_b[j])],
            )
        })
    })
}

fn validate_degree_one_projective_seam(
    start: &[[f64; 4]],
    end: &[[f64; 4]],
    start_derivative: &[[f64; 4]],
    end_derivative: &[[f64; 4]],
    transverse_degree: usize,
) -> bool {
    if transverse_degree != 1
        || start.len() < 2
        || start.len() != end.len()
        || start.len() != start_derivative.len()
        || start.len() != end_derivative.len()
        || start
            .len()
            .checked_mul(96)
            .is_none_or(|work| work > MAX_EVALUATION_WORK)
    {
        return false;
    }

    // shortcut: only degree-one periodic and transverse axes; upgrade by exact endpoint extraction.
    // Degree-one transverse spans are linear between adjacent control ordinates.
    for index in 0..start.len() - 1 {
        let (s0, s1) = (start[index], start[index + 1]);
        let (e0, e1) = (end[index], end[index + 1]);
        let (start_derivative_0, start_derivative_1) =
            (start_derivative[index], start_derivative[index + 1]);
        let (end_derivative_0, end_derivative_1) =
            (end_derivative[index], end_derivative[index + 1]);
        if [s0[3], s1[3], e0[3], e1[3]]
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return false;
        }

        // In the quadratic Bernstein product, the middle coefficient is the
        // sum of the two cross-products; checking all three coefficients proves
        // the identities on the full span without parameter samples.
        if !exact_wedges_equal(e0, s0, [0.0; 4], [0.0; 4])
            || !exact_wedges_equal(e1, s1, [0.0; 4], [0.0; 4])
            || !exact_wedges_equal(s0, end_derivative_0, e0, start_derivative_0)
            || !exact_wedges_equal(s1, end_derivative_1, e1, start_derivative_1)
        {
            return false;
        }
        for i in 0..4 {
            for j in i + 1..4 {
                if !exact_product_terms_sum_to_zero(
                    [(e0[i], s1[j]), (e1[i], s0[j])],
                    [(e0[j], s1[i]), (e1[j], s0[i])],
                ) || !exact_product_terms_sum_to_zero(
                    [
                        (s0[i], end_derivative_1[j]),
                        (s1[i], end_derivative_0[j]),
                        (e0[j], start_derivative_1[i]),
                        (e1[j], start_derivative_0[i]),
                    ],
                    [
                        (s0[j], end_derivative_1[i]),
                        (s1[j], end_derivative_0[i]),
                        (e0[i], start_derivative_1[j]),
                        (e1[i], start_derivative_0[j]),
                    ],
                ) {
                    return false;
                }
            }
        }
    }
    true
}

fn cartesian_point_and_tangent(
    point: [f64; 4],
    derivative: [f64; 4],
) -> Option<([f64; 3], [f64; 3])> {
    let weight = point[3];
    if !weight.is_finite()
        || weight <= 0.0
        || point.iter().chain(&derivative).any(|v| !v.is_finite())
    {
        return None;
    }
    let cartesian = std::array::from_fn(|axis| point[axis] / weight);
    let tangent =
        std::array::from_fn(|axis| (derivative[axis] - cartesian[axis] * derivative[3]) / weight);
    (cartesian.iter().chain(&tangent).all(|v| v.is_finite()) && tangent.iter().any(|v| *v != 0.0))
        .then_some((cartesian, tangent))
}

fn seam_matches_at_parameter(
    start_points: &[[f64; 4]],
    end_points: &[[f64; 4]],
    start_derivatives: &[[f64; 4]],
    end_derivatives: &[[f64; 4]],
    transverse_knots: &[f64],
    transverse_degree: usize,
    parameter: f64,
) -> bool {
    let Ok(start_point) =
        evaluate_curve(start_points, transverse_knots, transverse_degree, parameter)
    else {
        return false;
    };
    let Ok(end_point) = evaluate_curve(end_points, transverse_knots, transverse_degree, parameter)
    else {
        return false;
    };
    let Ok(start_derivative) = evaluate_curve(
        start_derivatives,
        transverse_knots,
        transverse_degree,
        parameter,
    ) else {
        return false;
    };
    let Ok(end_derivative) = evaluate_curve(
        end_derivatives,
        transverse_knots,
        transverse_degree,
        parameter,
    ) else {
        return false;
    };
    let (Some((start, start_tangent)), Some((end, end_tangent))) = (
        cartesian_point_and_tangent(start_point, start_derivative),
        cartesian_point_and_tangent(end_point, end_derivative),
    ) else {
        return false;
    };
    let distance = (start[0] - end[0])
        .hypot(start[1] - end[1])
        .hypot(start[2] - end[2]);
    if !distance.is_finite() || distance > 1e-8 {
        return false;
    }
    let normalize = |tangent: [f64; 3]| {
        let scale = tangent.iter().map(|value| value.abs()).fold(0.0, f64::max);
        (scale > 0.0 && scale.is_finite()).then(|| {
            let scaled = tangent.map(|value| value / scale);
            let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
            scaled.map(|value| value / length)
        })
    };
    let (Some(a), Some(b)) = (normalize(start_tangent), normalize(end_tangent)) else {
        return false;
    };
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let angle = cross[0]
        .hypot(cross[1])
        .hypot(cross[2])
        .atan2(a[0] * b[0] + a[1] * b[1] + a[2] * b[2]);
    angle.is_finite() && angle <= 1e-11
}

#[allow(
    clippy::float_cmp,
    reason = "Projective seam recognition requires exact unit endpoint weights and zero derivative weights."
)]
fn degree_one_transverse_seam_within_tolerance(
    start_points: &[[f64; 4]],
    end_points: &[[f64; 4]],
    start_derivatives: &[[f64; 4]],
    end_derivatives: &[[f64; 4]],
    transverse_knots: &[f64],
) -> bool {
    if start_points.len() < 2
        || start_points.len() != end_points.len()
        || start_points.len() != start_derivatives.len()
        || start_points.len() != end_derivatives.len()
        || transverse_knots.len() != start_points.len() + 2
        || start_points
            .iter()
            .chain(end_points)
            .any(|point| point[3] != 1.0)
        || start_derivatives
            .iter()
            .chain(end_derivatives)
            .any(|derivative| derivative[3] != 0.0)
    {
        return false;
    }

    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |vector: [f64; 3]| vector[0].hypot(vector[1]).hypot(vector[2]);
    let mut active_span = false;
    for knot_span in 1..=start_points.len() {
        if transverse_knots[knot_span] >= transverse_knots[knot_span + 1] {
            continue;
        }
        active_span = true;
        let index = knot_span - 1;
        let Some((start_a, tangent_a)) =
            cartesian_point_and_tangent(start_points[index], start_derivatives[index])
        else {
            return false;
        };
        let Some((end_a, tangent_b)) =
            cartesian_point_and_tangent(end_points[index], end_derivatives[index])
        else {
            return false;
        };
        let Some((start_b, next_tangent_a)) =
            cartesian_point_and_tangent(start_points[index + 1], start_derivatives[index + 1])
        else {
            return false;
        };
        let Some((end_b, next_tangent_b)) =
            cartesian_point_and_tangent(end_points[index + 1], end_derivatives[index + 1])
        else {
            return false;
        };

        let distance = |a: [f64; 3], b: [f64; 3]| norm([a[0] - b[0], a[1] - b[1], a[2] - b[2]]);
        if distance(start_a, end_a) > 1e-8 || distance(start_b, end_b) > 1e-8 {
            return false;
        }

        // Bernstein coefficients bound the quadratic cross and dot products on this span.
        let cross_coefficients = [
            cross(tangent_a, tangent_b),
            {
                let first = cross(tangent_a, next_tangent_b);
                let second = cross(next_tangent_a, tangent_b);
                [
                    (first[0] + second[0]) * 0.5,
                    (first[1] + second[1]) * 0.5,
                    (first[2] + second[2]) * 0.5,
                ]
            },
            cross(next_tangent_a, next_tangent_b),
        ];
        let dot_coefficients = [
            dot(tangent_a, tangent_b),
            (dot(tangent_a, next_tangent_b) + dot(next_tangent_a, tangent_b)) * 0.5,
            dot(next_tangent_a, next_tangent_b),
        ];
        let max_cross = cross_coefficients.into_iter().map(norm).fold(0.0, f64::max);
        let min_dot = dot_coefficients.into_iter().fold(f64::INFINITY, f64::min);
        let cross_limit = min_dot * 1e-11 * (1.0 + 64.0 * f64::EPSILON);
        if !max_cross.is_finite()
            || !min_dot.is_finite()
            || min_dot <= 0.0
            || max_cross > cross_limit
        {
            return false;
        }
    }
    active_span
}

#[allow(
    clippy::float_cmp,
    reason = "Clamped knot detection must preserve exact endpoint-knot equality semantics."
)]
#[allow(
    clippy::cast_precision_loss,
    reason = "The only callers pass degrees originating from NurbsSurface's u16 degree fields."
)]
#[allow(
    clippy::too_many_lines,
    reason = "Periodic seam validation keeps its ordered endpoint evaluation and rejection checks together."
)]
fn validate_periodic_surface_seam(
    controls: SurfaceControlGrid<'_>,
    knots: &[f64],
    transverse_knots: &[f64],
    degree: usize,
    periodic_u: bool,
    transverse_degree: usize,
    transverse_parameter: f64,
) -> io::Result<()> {
    let (curve_count, transverse_count) = if periodic_u {
        (controls.u_count, controls.v_count)
    } else {
        (controls.v_count, controls.u_count)
    };
    let start = knots[degree];
    let end = knots[curve_count];
    let derivative_degree = degree - 1;
    let derivative_knots = &knots[1..knots.len() - 1];
    let mut curve = Vec::new();
    curve
        .try_reserve_exact(curve_count)
        .map_err(|_| invalid("NURBS surface seam curve allocation failed"))?;
    let mut derivative = Vec::new();
    derivative
        .try_reserve_exact(curve_count - 1)
        .map_err(|_| invalid("NURBS surface seam derivative allocation failed"))?;
    let mut start_points = Vec::new();
    start_points
        .try_reserve_exact(transverse_count)
        .map_err(|_| invalid("NURBS surface seam point allocation failed"))?;
    let mut end_points = Vec::new();
    end_points
        .try_reserve_exact(transverse_count)
        .map_err(|_| invalid("NURBS surface seam point allocation failed"))?;
    let mut start_derivatives = Vec::new();
    start_derivatives
        .try_reserve_exact(transverse_count)
        .map_err(|_| invalid("NURBS surface seam derivative allocation failed"))?;
    let mut end_derivatives = Vec::new();
    end_derivatives
        .try_reserve_exact(transverse_count)
        .map_err(|_| invalid("NURBS surface seam derivative allocation failed"))?;
    let mut exact_start_points = Vec::new();
    let mut exact_end_points = Vec::new();
    let mut exact_start_derivatives = Vec::new();
    let mut exact_end_derivatives = Vec::new();
    if degree > 1 {
        for values in [
            &mut exact_start_points,
            &mut exact_end_points,
            &mut exact_start_derivatives,
            &mut exact_end_derivatives,
        ] {
            values
                .try_reserve_exact(transverse_count)
                .map_err(|_| invalid("NURBS surface exact seam allocation failed"))?;
        }
    }
    let mut exact_endpoint_data = degree > 1;

    for transverse in 0..transverse_count {
        curve.clear();
        for index in 0..curve_count {
            let control = if periodic_u {
                controls.homogeneous[index * controls.v_count + transverse]
            } else {
                controls.homogeneous[transverse * controls.v_count + index]
            };
            curve.push(control);
        }
        let start_point = evaluate_curve(&curve, knots, degree, start)?;
        let end_point = evaluate_curve(&curve, knots, degree, end)?;
        if !start_point[3].is_finite()
            || start_point[3] <= 0.0
            || !end_point[3].is_finite()
            || end_point[3] <= 0.0
        {
            return Err(invalid("invalid periodic NURBS surface seam weight"));
        }
        start_points.push(start_point);
        end_points.push(end_point);
        if degree > 1 {
            if let (
                Some((exact_start, exact_start_derivative)),
                Some((exact_end, exact_end_derivative)),
            ) = (
                exact_endpoint_curve_jet(&curve, knots, degree, false),
                exact_endpoint_curve_jet(&curve, knots, degree, true),
            ) {
                exact_start_points.push(exact_start);
                exact_end_points.push(exact_end);
                exact_start_derivatives.push(exact_start_derivative);
                exact_end_derivatives.push(exact_end_derivative);
            } else {
                exact_endpoint_data = false;
            }
        }

        derivative.clear();
        for (index, pair) in curve.windows(2).enumerate() {
            let denominator = knots[index + degree + 1] - knots[index + 1];
            if denominator <= 0.0 || !denominator.is_finite() {
                return Err(invalid(
                    "invalid periodic NURBS surface endpoint tangent interval",
                ));
            }
            let scale = degree as f64 / denominator;
            let value = std::array::from_fn(|axis| (pair[1][axis] - pair[0][axis]) * scale);
            if value.iter().any(|component| !component.is_finite()) {
                return Err(invalid("nonfinite periodic NURBS surface endpoint tangent"));
            }
            derivative.push(value);
        }
        start_derivatives.push(evaluate_curve(
            &derivative,
            derivative_knots,
            derivative_degree,
            start,
        )?);
        end_derivatives.push(evaluate_curve(
            &derivative,
            derivative_knots,
            derivative_degree,
            end,
        )?);
    }

    let start_weight = start_points[0][3];
    let end_weight = end_points[0][3];
    let endpoints_match =
        homogeneous_sequence_matches_scale(&start_points, &end_points, start_weight, end_weight);
    let derivatives_match = homogeneous_sequence_matches_scale(
        &start_derivatives,
        &end_derivatives,
        start_weight,
        end_weight,
    ) || homogeneous_derivatives_match_projective_gauge(
        &start_points,
        &start_derivatives,
        &end_derivatives,
        start_weight,
        end_weight,
    );
    let clamped = knots[..=degree].iter().all(|&knot| knot == start)
        && knots[curve_count..].iter().all(|&knot| knot == end);
    let exact_raw_seam_data = if degree == 1 {
        degree_one_periodic_data_is_exact(
            controls,
            knots,
            periodic_u,
            SeamEndpointData {
                start_points: &start_points,
                end_points: &end_points,
                start_derivatives: &start_derivatives,
                end_derivatives: &end_derivatives,
            },
        )
    } else if clamped {
        clamped_periodic_data_is_exact(
            controls,
            knots,
            degree,
            periodic_u,
            SeamEndpointData {
                start_points: &start_points,
                end_points: &end_points,
                start_derivatives: &start_derivatives,
                end_derivatives: &end_derivatives,
            },
        )
    } else {
        exact_endpoint_data
            && exact_start_points.len() == transverse_count
            && exact_end_points.len() == transverse_count
            && exact_start_derivatives.len() == transverse_count
            && exact_end_derivatives.len() == transverse_count
    };
    let (seam_start_points, seam_end_points, seam_start_derivatives, seam_end_derivatives) =
        if degree > 1 && !clamped && exact_raw_seam_data {
            (
                exact_start_points.as_slice(),
                exact_end_points.as_slice(),
                exact_start_derivatives.as_slice(),
                exact_end_derivatives.as_slice(),
            )
        } else {
            (
                start_points.as_slice(),
                end_points.as_slice(),
                start_derivatives.as_slice(),
                end_derivatives.as_slice(),
            )
        };
    if !(endpoints_match && derivatives_match)
        && transverse_count >= 2
        && exact_raw_seam_data
        && if transverse_degree == 1 {
            validate_degree_one_projective_seam(
                seam_start_points,
                seam_end_points,
                seam_start_derivatives,
                seam_end_derivatives,
                transverse_degree,
            )
        } else {
            validate_sampled_projective_seam(
                seam_start_points,
                seam_end_points,
                seam_start_derivatives,
                seam_end_derivatives,
                transverse_knots,
                transverse_degree,
            )
        }
    {
        return Ok(());
    }
    if !(endpoints_match && derivatives_match)
        && seam_matches_at_parameter(
            seam_start_points,
            seam_end_points,
            seam_start_derivatives,
            seam_end_derivatives,
            transverse_knots,
            transverse_degree,
            transverse_parameter,
        )
    {
        return Ok(());
    }
    if !(endpoints_match && derivatives_match)
        && transverse_degree == 1
        && degree_one_transverse_seam_within_tolerance(
            seam_start_points,
            seam_end_points,
            seam_start_derivatives,
            seam_end_derivatives,
            transverse_knots,
        )
    {
        return Ok(());
    }
    if !endpoints_match {
        return Err(invalid(
            "periodic NURBS surface seam endpoints do not match",
        ));
    }
    if !derivatives_match {
        return Err(invalid(
            "periodic NURBS surface seam derivatives do not match",
        ));
    }
    Ok(())
}

/// Evaluate a mapped open, closed nonperiodic, or validated periodic surface form.
///
/// This supported mapped-surface profile uses a v-fast control grid and, for
/// rational surfaces, homogeneous `[x, y, z, weight]` control vertices. It is
/// intentionally narrower than the mapped `NurbsSurface` type. Periodic axes
/// must also be closed and either have the validated repeated-control form,
/// exact homogeneous seam sequences, or an exact projective seam proof when
/// both axes are degree one. Closed
/// nonperiodic axes use the bounded active knot domain without wrapping.
/// The closed flag on a nonperiodic axis is retained metadata and is not
/// verified against the surface geometry by this point evaluator. Rational
/// weights must be finite and positive.
///
/// # Errors
/// Returns an error if the parameters are non-finite or the surface uses an
/// unsupported layout, invalid knot or control data, or a seam that fails the
/// supported validation rules.
pub fn evaluate_mapped_nurbs_surface(
    surface: &NurbsSurface,
    u: f64,
    v: f64,
) -> io::Result<[f64; 3]> {
    if !u.is_finite() || !v.is_finite() {
        return Err(invalid("unsupported NURBS surface profile or parameters"));
    }
    PreparedNurbsSurface::new(surface)?.point(u, v)
}

pub(super) struct PreparedNurbsSurface {
    u_degree: usize,
    v_degree: usize,
    u_count: usize,
    v_count: usize,
    rational: bool,
    u_axis: AxisPeriodicity,
    v_axis: AxisPeriodicity,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    homogeneous: Vec<[f64; 4]>,
    u_min: f64,
    u_max: f64,
    v_min: f64,
    v_max: f64,
}

impl PreparedNurbsSurface {
    #[allow(
        clippy::float_cmp,
        reason = "Periodic profile detection requires exact repeated-knot differences to match the period."
    )]
    #[allow(
        clippy::too_many_lines,
        reason = "Preparation validates and stages the complete NURBS surface representation as one operation."
    )]
    pub(super) fn new(surface: &NurbsSurface) -> io::Result<Self> {
        let (u_degree, v_degree) = (usize::from(surface.u_degree), usize::from(surface.v_degree));
        let (u_count, v_count) = (
            usize::try_from(surface.u_control_vertex_count)
                .map_err(|_| invalid("invalid NURBS surface U control count"))?,
            usize::try_from(surface.v_control_vertex_count)
                .map_err(|_| invalid("invalid NURBS surface V control count"))?,
        );
        let dimension = usize::from(surface.vertex_dimension);
        let controls = u_count
            .checked_mul(v_count)
            .ok_or_else(|| invalid("NURBS surface control grid overflow"))?;
        let u_basis_work = u_degree
            .checked_add(1)
            .and_then(|value| value.checked_mul(value))
            .ok_or_else(|| invalid("NURBS surface work estimate overflow"))?;
        let v_basis_work = v_degree
            .checked_add(1)
            .and_then(|value| value.checked_mul(value))
            .ok_or_else(|| invalid("NURBS surface work estimate overflow"))?;
        let work = v_count
            .checked_mul(u_basis_work)
            .and_then(|value| value.checked_add(v_basis_work))
            .ok_or_else(|| invalid("NURBS surface work estimate overflow"))?;
        let periodic_u = surface.u_periodic;
        let periodic_v = surface.v_periodic;
        if (periodic_u && !surface.u_closed)
            || (periodic_v && !surface.v_closed)
            || u_degree == 0
            || v_degree == 0
            || u_count <= u_degree
            || v_count <= v_degree
            || controls > MAX_CONTROLS
            || work > MAX_EVALUATION_WORK
            || dimension != if surface.rational { 4 } else { 3 }
            || surface.control_vertices.len() != controls
        {
            return Err(invalid("unsupported NURBS surface profile or parameters"));
        }
        let u_knots = expand_knots(
            &surface.u_knots,
            &surface.u_knot_multiplicities,
            u_count,
            u_degree,
            periodic_u,
        )?;
        let v_knots = expand_knots(
            &surface.v_knots,
            &surface.v_knot_multiplicities,
            v_count,
            v_degree,
            periodic_v,
        )?;
        let u_min = u_knots[u_degree];
        let u_max = u_knots[u_count];
        let v_min = v_knots[v_degree];
        let v_max = v_knots[v_count];
        let mut validate_u_seam = false;
        if periodic_u {
            let unique_u = u_count
                .checked_sub(u_degree)
                .ok_or_else(|| invalid("invalid periodic U control count"))?;
            let period = u_knots[u_degree + unique_u] - u_min;
            if !period.is_finite() || period <= 0.0 {
                return Err(invalid("invalid periodic U knot extension"));
            }
            let repeated_profile_possible = unique_u > u_degree;
            let matching_knots = repeated_profile_possible
                && (0..=u_degree).all(|index| u_knots[index + unique_u] - u_knots[index] == period);
            let matching_controls = repeated_profile_possible
                && (0..u_degree).all(|row| {
                    let first = row * v_count;
                    let repeated = (unique_u + row) * v_count;
                    surface.control_vertices[first..first + v_count]
                        == surface.control_vertices[repeated..repeated + v_count]
                });
            validate_u_seam = !(matching_knots && matching_controls && u_degree > 1);
        }
        let mut check_v_seam = false;
        if periodic_v {
            let unique_v = v_count
                .checked_sub(v_degree)
                .ok_or_else(|| invalid("invalid periodic V control count"))?;
            let period = v_knots[v_degree + unique_v] - v_min;
            if !period.is_finite() || period <= 0.0 {
                return Err(invalid("invalid periodic V knot extension"));
            }
            let repeated_profile_possible = unique_v > v_degree;
            let matching_knots = repeated_profile_possible
                && (0..=v_degree).all(|index| v_knots[index + unique_v] - v_knots[index] == period);
            let matching_controls = repeated_profile_possible
                && (0..u_count).all(|row| {
                    let first = row * v_count;
                    let repeated = first + unique_v;
                    surface.control_vertices[first..first + v_degree]
                        == surface.control_vertices[repeated..repeated + v_degree]
                });
            check_v_seam = !(matching_knots && matching_controls && v_degree > 1);
        }

        if validate_u_seam || check_v_seam {
            let mut seam_work = 0usize;
            if validate_u_seam {
                let basis_work = u_basis_work
                    .checked_mul(4)
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
                let exact_jet_work = if u_degree > 1 {
                    u_count
                        .checked_mul(4)
                        .and_then(|value| {
                            u_basis_work
                                .checked_mul(4)
                                .and_then(|basis| value.checked_add(basis))
                        })
                        .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?
                } else {
                    0
                };
                let per_column = u_count
                    .checked_mul(2)
                    .and_then(|value| value.checked_add(basis_work))
                    .and_then(|value| value.checked_add(exact_jet_work))
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
                let transverse_samples = if u_degree == 1 && v_degree > 1 {
                    sampled_seam_work(v_count, v_degree)
                        .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?
                } else {
                    0
                };
                seam_work = v_count
                    .checked_mul(per_column)
                    .and_then(|value| value.checked_add(transverse_samples))
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
            }
            if check_v_seam {
                let basis_work = v_basis_work
                    .checked_mul(4)
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
                let exact_jet_work = if v_degree > 1 {
                    v_count
                        .checked_mul(4)
                        .and_then(|value| {
                            v_basis_work
                                .checked_mul(4)
                                .and_then(|basis| value.checked_add(basis))
                        })
                        .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?
                } else {
                    0
                };
                let per_row = v_count
                    .checked_mul(2)
                    .and_then(|value| value.checked_add(basis_work))
                    .and_then(|value| value.checked_add(exact_jet_work))
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
                let transverse_samples = if v_degree == 1 && u_degree > 1 {
                    sampled_seam_work(u_count, u_degree)
                        .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?
                } else {
                    0
                };
                seam_work = seam_work
                    .checked_add(
                        u_count
                            .checked_mul(per_row)
                            .and_then(|value| value.checked_add(transverse_samples))
                            .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?,
                    )
                    .ok_or_else(|| invalid("NURBS surface seam work estimate overflow"))?;
            }
            if work
                .checked_add(seam_work)
                .is_none_or(|total| total > MAX_EVALUATION_WORK)
            {
                return Err(invalid("NURBS surface seam work limit exceeded"));
            }
        }

        let mut homogeneous = Vec::new();
        homogeneous
            .try_reserve_exact(controls)
            .map_err(|_| invalid("NURBS surface control allocation failed"))?;
        for control in &surface.control_vertices {
            if control.len() != dimension || control.iter().any(|value| !value.is_finite()) {
                return Err(invalid("invalid NURBS surface control vertex"));
            }
            let mut point = [0.0; 4];
            point[..dimension].copy_from_slice(control);
            if surface.rational {
                let weight = point[3];
                if weight <= 0.0 {
                    return Err(invalid("invalid rational NURBS surface weight"));
                }
                for coordinate in &mut point[..3] {
                    *coordinate *= weight;
                }
            } else {
                point[3] = 1.0;
            }
            if point.iter().any(|value| !value.is_finite()) {
                return Err(invalid("nonfinite homogeneous NURBS surface control"));
            }
            homogeneous.push(point);
        }

        Ok(Self {
            u_degree,
            v_degree,
            u_count,
            v_count,
            rational: surface.rational,
            u_axis: AxisPeriodicity {
                periodic: periodic_u,
                validate_seam: validate_u_seam,
            },
            v_axis: AxisPeriodicity {
                periodic: periodic_v,
                validate_seam: check_v_seam,
            },
            u_knots,
            v_knots,
            homogeneous,
            u_min,
            u_max,
            v_min,
            v_max,
        })
    }

    pub(super) fn active_bounds(&self) -> (f64, f64, f64, f64) {
        (self.u_min, self.u_max, self.v_min, self.v_max)
    }

    pub(super) fn point(&self, u: f64, v: f64) -> io::Result<[f64; 3]> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("unsupported NURBS surface profile or parameters"));
        }
        let mut evaluation_u = u;
        let mut evaluation_v = v;
        if self.u_axis.periodic {
            let unique_u = self.u_count - self.u_degree;
            let period = self.u_knots[self.u_degree + unique_u] - self.u_min;
            let offset = u - self.u_min;
            let wraps = offset / period;
            if !wraps.is_finite() || wraps.abs() > MAX_PRECISE_PERIODIC_WRAPS {
                return Err(invalid("periodic U wrap count exceeds precise range"));
            }
            evaluation_u = self.u_min + offset.rem_euclid(period);
            if evaluation_u >= self.u_max {
                evaluation_u = self.u_min;
            }
        }
        if self.v_axis.periodic {
            let unique_v = self.v_count - self.v_degree;
            let period = self.v_knots[self.v_degree + unique_v] - self.v_min;
            let offset = v - self.v_min;
            let wraps = offset / period;
            if !wraps.is_finite() || wraps.abs() > MAX_PRECISE_PERIODIC_WRAPS {
                return Err(invalid("periodic V wrap count exceeds precise range"));
            }
            evaluation_v = self.v_min + offset.rem_euclid(period);
            if evaluation_v >= self.v_max {
                evaluation_v = self.v_min;
            }
        }
        if (!self.u_axis.periodic && (u < self.u_min || u > self.u_max))
            || (!self.v_axis.periodic && (v < self.v_min || v > self.v_max))
        {
            return Err(invalid("NURBS surface parameter outside domain"));
        }
        if self.u_axis.validate_seam {
            validate_periodic_surface_seam(
                SurfaceControlGrid {
                    homogeneous: &self.homogeneous,
                    u_count: self.u_count,
                    v_count: self.v_count,
                },
                &self.u_knots,
                &self.v_knots,
                self.u_degree,
                true,
                self.v_degree,
                evaluation_v,
            )?;
        }
        if self.v_axis.validate_seam {
            validate_periodic_surface_seam(
                SurfaceControlGrid {
                    homogeneous: &self.homogeneous,
                    u_count: self.u_count,
                    v_count: self.v_count,
                },
                &self.v_knots,
                &self.u_knots,
                self.v_degree,
                false,
                self.u_degree,
                evaluation_u,
            )?;
        }

        let mut along_u = Vec::new();
        along_u
            .try_reserve_exact(self.v_count)
            .map_err(|_| invalid("NURBS surface intermediate allocation failed"))?;
        for column in 0..self.v_count {
            let mut points = Vec::new();
            points
                .try_reserve_exact(self.u_count)
                .map_err(|_| invalid("NURBS surface column allocation failed"))?;
            for row in 0..self.u_count {
                points.push(self.homogeneous[row * self.v_count + column]);
            }
            along_u.push(evaluate_curve(
                &points,
                &self.u_knots,
                self.u_degree,
                evaluation_u,
            )?);
        }
        let point = evaluate_curve(&along_u, &self.v_knots, self.v_degree, evaluation_v)?;
        let weight = if self.rational { point[3] } else { 1.0 };
        if weight <= 0.0 || !weight.is_finite() {
            return Err(invalid("invalid NURBS surface evaluation denominator"));
        }
        let result = [point[0] / weight, point[1] / weight, point[2] / weight];
        if result.iter().any(|value| !value.is_finite()) {
            return Err(invalid("nonfinite NURBS surface result"));
        }
        Ok(result)
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::{
        PreparedNurbsSurface, evaluate_mapped_nurbs_surface, exact_evaluate_curve,
        exact_product_difference, exact_sum, sampled_seam_work,
    };

    #[test]
    fn sampled_seam_uses_checked_active_span_and_bounded_work_estimate() {
        let points = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0, 1.0],
        ];
        let knots = [0.0, 0.0, 1.0, 2.0, 2.0];
        assert_eq!(
            exact_evaluate_curve(&points, &knots, 1, 1.5, 2),
            Some([1.5, 0.0, 0.0, 1.0])
        );
        assert_eq!(exact_evaluate_curve(&points, &knots, 1, 1.5, 1), None);
        assert_eq!(sampled_seam_work(3, 2), Some(8_160));
        assert_eq!(sampled_seam_work(usize::MAX, 2), None);
    }
    use crate::brep::NurbsSurface;

    fn periodic_v_surface() -> NurbsSurface {
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 2,
            v_control_vertex_count: 4,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: true,
            u_closed: false,
            v_closed: true,
            rational: false,
            surface_form: 0,
            control_vertices: vec![
                vec![0.0, 0.0, 0.0],
                vec![1.0, 0.0, 0.0],
                vec![-1.0, 0.0, 0.0],
                vec![0.0, 0.0, 0.0],
                vec![0.0, 0.0, 1.0],
                vec![1.0, 0.0, 1.0],
                vec![-1.0, 0.0, 1.0],
                vec![0.0, 0.0, 1.0],
            ],
            u_knots: vec![0.0, 1.0],
            v_knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            u_knot_multiplicities: vec![2, 2],
            v_knot_multiplicities: vec![1; 6],
            sources: vec![],
        }
    }

    fn closed_nonperiodic_v_surface() -> NurbsSurface {
        let mut surface = periodic_v_surface();
        surface.v_periodic = false;
        surface.v_knots = vec![0.0, 1.0, 2.0, 3.0];
        surface.v_knot_multiplicities = vec![2, 1, 1, 2];
        surface
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Prepared and direct evaluators must return exactly equal periodic samples."
    )]
    fn prepared_surface_reuses_controls_and_validates_each_sample() -> std::io::Result<()> {
        let periodic = periodic_v_surface();
        let prepared = PreparedNurbsSurface::new(&periodic)?;
        assert_eq!(prepared.active_bounds(), (0.0, 1.0, 1.0, 4.0));
        for (u, v) in [(0.25, 1.5), (0.75, 2.5), (0.25, 4.5)] {
            assert_eq!(
                prepared.point(u, v)?,
                evaluate_mapped_nurbs_surface(&periodic, u, v)?
            );
        }
        assert!(prepared.point(0.5, f64::MAX).is_err());

        let bounded = PreparedNurbsSurface::new(&closed_nonperiodic_v_surface())?;
        assert!(bounded.point(0.5, 3.1).is_err());
        Ok(())
    }

    fn closed_nonperiodic_uv_surface() -> NurbsSurface {
        let mut control_vertices = Vec::new();
        for u in 0..3 {
            for v in 0..3 {
                let unique_u = if u == 2 { 0 } else { u };
                let unique_v = if v == 2 { 0 } else { v };
                control_vertices.push(vec![f64::from(unique_u), f64::from(unique_v), 0.0]);
            }
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 3,
            v_control_vertex_count: 3,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: false,
            u_closed: true,
            v_closed: true,
            rational: false,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0],
            v_knots: vec![0.0, 1.0, 2.0],
            u_knot_multiplicities: vec![2, 1, 2],
            v_knot_multiplicities: vec![2, 1, 2],
            sources: vec![],
        }
    }

    fn mixed_closed_nonperiodic_u_periodic_v_surface() -> NurbsSurface {
        let v_profile = [[0.0, 0.0], [1.0, 0.0], [-1.0, 0.0], [0.0, 0.0]];
        let mut control_vertices = Vec::new();
        for u in [0.0, 1.0, 0.0] {
            for [x, y] in v_profile {
                control_vertices.push(vec![u, x, y]);
            }
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 3,
            v_control_vertex_count: 4,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: true,
            u_closed: true,
            v_closed: true,
            rational: false,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0],
            v_knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            u_knot_multiplicities: vec![2, 1, 2],
            v_knot_multiplicities: vec![1; 6],
            sources: vec![],
        }
    }

    fn clamped_nonrepeated_u_surface() -> NurbsSurface {
        let profile = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [-4.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let control_vertices = profile
            .into_iter()
            .flat_map(|point| [[point[0], point[1], point[2]], [point[0], point[1], 1.0]])
            .map(|point| point.to_vec())
            .collect();
        NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_control_vertex_count: 5,
            v_control_vertex_count: 2,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: false,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 3.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![3, 2, 3],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        }
    }

    fn degree_one_transverse_varying_projective_seam() -> NurbsSurface {
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 4,
            v_control_vertex_count: 2,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices: vec![
                vec![0.0, 0.0, 0.0, 1.0],
                vec![0.0, 0.0, 0.0, 1.0],
                vec![1.0, 0.0, 0.0, 1.0],
                vec![1.0, 0.0, 0.0, 1.0],
                vec![-1.0, 0.0, 0.0, 1.0],
                vec![-2.0, 0.0, 0.0, 1.0],
                vec![0.0, 0.0, 0.0, 1.0],
                vec![0.0, 0.0, 0.0, 2.0],
            ],
            u_knots: vec![0.0, 1.0, 2.0, 3.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![2, 1, 1, 2],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        }
    }

    fn quadratic_transverse_collapsed_projective_seam() -> NurbsSurface {
        let mut control_vertices = Vec::new();
        for (x, weight) in [(0.0, 1.0), (1.0, 1.0)] {
            for _ in 0..3 {
                control_vertices.push(vec![x, 0.0, 0.0, weight]);
            }
        }
        for x in [-1.0, 0.0] {
            for weight in [2.0, 2.5, 3.0] {
                control_vertices.push(vec![x, 0.0, 0.0, weight]);
            }
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 2,
            u_control_vertex_count: 4,
            v_control_vertex_count: 3,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0, 3.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![2, 1, 1, 2],
            v_knot_multiplicities: vec![3, 3],
            sources: vec![],
        }
    }

    fn quadratic_noncollapsed_varying_projective_seam() -> NurbsSurface {
        let start = [
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ];
        let end = [
            [0.0, 0.0, 0.0, 2.0],
            [1.0, 0.0, 0.0, 3.0],
            [4.0, 0.0, 0.0, 4.0],
        ];
        let start_derivative = [[0.0, 0.0, 1.0, 0.0]; 3];
        let end_derivative = [
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 3.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
        ];
        let middle = std::array::from_fn(|index| {
            std::array::from_fn(|axis| f64::midpoint(start[index][axis], end[index][axis]))
        });
        let rows = [
            start,
            std::array::from_fn(|index| {
                std::array::from_fn(|axis| start[index][axis] + start_derivative[index][axis] / 2.0)
            }),
            middle,
            std::array::from_fn(|index| {
                std::array::from_fn(|axis| end[index][axis] - end_derivative[index][axis] / 2.0)
            }),
            end,
        ];
        let control_vertices = rows
            .into_iter()
            .flat_map(|row| {
                row.into_iter()
                    .map(|[x, y, z, weight]| vec![x / weight, y / weight, z / weight, weight])
            })
            .collect();
        NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_control_vertex_count: 5,
            v_control_vertex_count: 3,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0, 3.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![3, 1, 1, 3],
            v_knot_multiplicities: vec![3, 3],
            sources: vec![],
        }
    }

    fn quadratic_clamped_varying_gauge_seam(periodic_u: bool) -> NurbsSurface {
        let controls = vec![
            vec![0.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0],
            vec![0.5, 0.0, 0.0, 1.0],
            vec![0.5, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0],
            vec![-1.0, 0.0, 0.0, 1.0],
            vec![-2.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 2.0],
        ];
        let (
            u_count,
            v_count,
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_multiplicities,
            v_multiplicities,
            control_vertices,
        ) = if periodic_u {
            (
                5,
                2,
                2,
                1,
                vec![0.0, 1.0, 3.0],
                vec![0.0, 1.0],
                vec![3, 2, 3],
                vec![2, 2],
                controls,
            )
        } else {
            let mut transposed = Vec::with_capacity(10);
            for v in 0..2 {
                for u in 0..5 {
                    transposed.push(controls[u * 2 + v].clone());
                }
            }
            (
                2,
                5,
                1,
                2,
                vec![0.0, 1.0],
                vec![0.0, 1.0, 3.0],
                vec![2, 2],
                vec![3, 2, 3],
                transposed,
            )
        };
        NurbsSurface {
            u_degree,
            v_degree,
            u_control_vertex_count: u_count,
            v_control_vertex_count: v_count,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: periodic_u,
            v_periodic: !periodic_u,
            u_closed: periodic_u,
            v_closed: !periodic_u,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots,
            v_knots,
            u_knot_multiplicities: u_multiplicities,
            v_knot_multiplicities: v_multiplicities,
            sources: vec![],
        }
    }

    #[test]
    fn projective_seam_products_are_signed_and_error_free_or_rejected() {
        assert_eq!(exact_product_difference(-1.5, 2.0, -3.0, 1.0), Some(0.0));
        assert_eq!(exact_sum(-1.0, 1.0), Some(0.0));
        assert_eq!(exact_product_difference(0.1, 0.1, 0.01, 1.0), None);
        assert_eq!(exact_product_difference(f64::MAX, 2.0, 1.0, 1.0), None);
        assert_eq!(exact_sum(1.0e16, 1.0), None);
    }

    #[test]
    fn degree_one_periodic_data_rejects_mismatched_endpoint_derivatives() {
        let surface = degree_one_transverse_varying_projective_seam();
        let homogeneous: Vec<[f64; 4]> = surface
            .control_vertices
            .iter()
            .map(|control| {
                [
                    control[0] * control[3],
                    control[1] * control[3],
                    control[2] * control[3],
                    control[3],
                ]
            })
            .collect();
        let start = [[0.0, 0.0, 0.0, 1.0]; 2];
        let end = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 2.0]];
        let start_derivatives = [[1.0, 0.0, 0.0, 0.0]; 2];
        let end_derivatives = [[1.0, 0.0, 0.0, 0.0], [2.0, 0.0, 0.0, 1.0]];
        let knots = [0.0, 0.0, 1.0, 2.0, 3.0, 3.0];
        assert!(super::degree_one_periodic_data_is_exact(
            super::SurfaceControlGrid {
                homogeneous: &homogeneous,
                u_count: 4,
                v_count: 2,
            },
            &knots,
            true,
            super::SeamEndpointData {
                start_points: &start,
                end_points: &end,
                start_derivatives: &start_derivatives,
                end_derivatives: &end_derivatives,
            },
        ));

        let mut wrong_start = start_derivatives;
        wrong_start[1][0] = 1.5;
        assert!(!super::degree_one_periodic_data_is_exact(
            super::SurfaceControlGrid {
                homogeneous: &homogeneous,
                u_count: 4,
                v_count: 2,
            },
            &knots,
            true,
            super::SeamEndpointData {
                start_points: &start,
                end_points: &end,
                start_derivatives: &wrong_start,
                end_derivatives: &end_derivatives,
            },
        ));
        let mut wrong_end = end_derivatives;
        wrong_end[1][0] = 2.5;
        assert!(!super::degree_one_periodic_data_is_exact(
            super::SurfaceControlGrid {
                homogeneous: &homogeneous,
                u_count: 4,
                v_count: 2,
            },
            &knots,
            true,
            super::SeamEndpointData {
                start_points: &start,
                end_points: &end,
                start_derivatives: &start_derivatives,
                end_derivatives: &wrong_end,
            },
        ));
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Equivalent periodic seam parameters must produce exactly equal surface points."
    )]
    fn degree_one_transverse_varying_projective_seam_is_exact_and_perturbation_fails()
    -> std::io::Result<()> {
        let surface = degree_one_transverse_varying_projective_seam();
        let seam = evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5)?;
        assert_eq!(seam, [0.0; 3]);
        assert_eq!(seam, evaluate_mapped_nurbs_surface(&surface, 3.0, 0.5)?);
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5)?,
            evaluate_mapped_nurbs_surface(&surface, 3.5, 0.5)?
        );

        let mut perturbed = surface;
        perturbed.control_vertices[7][0] = 0.5;
        assert!(evaluate_mapped_nurbs_surface(&perturbed, 0.5, 0.5).is_err());
        Ok(())
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Equivalent periodic seam parameters must produce exactly equal surface points."
    )]
    fn quadratic_transverse_collapsed_projective_seam_is_exact_and_perturbation_fails()
    -> std::io::Result<()> {
        let surface = quadratic_transverse_collapsed_projective_seam();
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5)?,
            evaluate_mapped_nurbs_surface(&surface, 3.0, 0.5)?
        );

        let mut perturbed = surface;
        perturbed.control_vertices[11][0] = 0.125;
        assert!(evaluate_mapped_nurbs_surface(&perturbed, 0.5, 0.5).is_err());
        Ok(())
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Equivalent periodic seam parameters must produce exactly equal surface points."
    )]
    fn quadratic_noncollapsed_varying_projective_seam_wraps_and_rejects_perturbation()
    -> std::io::Result<()> {
        let surface = quadratic_noncollapsed_varying_projective_seam();
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5)?,
            evaluate_mapped_nurbs_surface(&surface, 3.0, 0.5)?
        );
        assert_ne!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.25)?,
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.75)?
        );

        let mut perturbed = surface;
        perturbed.control_vertices[14][0] += 0.125;
        assert!(evaluate_mapped_nurbs_surface(&perturbed, 0.5, 0.5).is_err());
        Ok(())
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Equivalent closed-axis parameters must produce exactly equal seam points."
    )]
    fn quadratic_clamped_varying_gauge_seam_accepts_u_and_v_and_rejects_perturbation()
    -> std::io::Result<()> {
        for periodic_u in [true, false] {
            let surface = quadratic_clamped_varying_gauge_seam(periodic_u);
            let wrapped = if periodic_u {
                evaluate_mapped_nurbs_surface(&surface, 3.5, 0.5)?
            } else {
                evaluate_mapped_nurbs_surface(&surface, 0.5, 3.5)?
            };
            assert_eq!(evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5)?, wrapped);

            let mut perturbed = surface;
            perturbed.control_vertices[9][0] = 0.25;
            assert!(evaluate_mapped_nurbs_surface(&perturbed, 0.5, 0.5).is_err());
        }
        Ok(())
    }

    fn rational_projectively_scaled_clamped_u_surface() -> NurbsSurface {
        let mut control_vertices = Vec::new();
        for u_index in 0..5 {
            for transverse_x in [0.0, 1.0] {
                let (x, y, weight) = match u_index {
                    0 => (transverse_x, 0.0, 2.0),
                    1 => (transverse_x + 0.5, 0.0, 2.0),
                    2 => (transverse_x, 1.0, 2.0),
                    3 => (transverse_x - 1.0, 0.0, 4.0),
                    _ => (transverse_x, 0.0, 4.0),
                };
                control_vertices.push(vec![x, y, 0.0, weight]);
            }
        }
        NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_control_vertex_count: 5,
            v_control_vertex_count: 2,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 3.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![3, 2, 3],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        }
    }

    fn rational_periodic_u_surface_with_projective_derivative_gauge() -> NurbsSurface {
        let mut control_vertices = Vec::new();
        for (x_offset, y, weight) in [
            (0.0, 0.0, 2.0),
            (0.5, 0.0, 2.0),
            (0.0, 1.0, 2.0),
            (-1.0, 0.0, 2.0),
            (0.0, 0.0, 4.0),
        ] {
            for transverse_x in [0.0, 1.0] {
                control_vertices.push(vec![transverse_x + x_offset, y, 0.0, weight]);
            }
        }
        NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_control_vertex_count: 5,
            v_control_vertex_count: 2,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![3, 2, 3],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        }
    }

    fn minimum_count_clamped_periodic_u_surface() -> NurbsSurface {
        let profile = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let control_vertices = profile
            .into_iter()
            .flat_map(|point| [[point[0], point[1], point[2]], [point[0], point[1], 1.0]])
            .map(|point| point.to_vec())
            .collect();
        NurbsSurface {
            u_degree: 3,
            v_degree: 1,
            u_control_vertex_count: 4,
            v_control_vertex_count: 2,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: false,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![4, 4],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        }
    }

    fn minimum_count_clamped_periodic_v_surface() -> NurbsSurface {
        let profile = [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let mut control_vertices = Vec::new();
        for u in 0..2 {
            control_vertices
                .extend(profile.map(|point| vec![f64::from(u) + point[0], point[1], point[2]]));
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 3,
            u_control_vertex_count: 2,
            v_control_vertex_count: 4,
            vertex_dimension: 3,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: true,
            u_closed: false,
            v_closed: true,
            rational: false,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![2, 2],
            v_knot_multiplicities: vec![4, 4],
            sources: vec![],
        }
    }

    #[test]
    fn tolerance_only_periodic_surface_seam_is_point_local() {
        let mut within_defaults = minimum_count_clamped_periodic_v_surface();
        for row in 0..2 {
            within_defaults.control_vertices[row * 4 + 2][0] += 5.0e-9;
            within_defaults.control_vertices[row * 4 + 3][0] += 5.005e-9;
        }
        within_defaults.control_vertices[5][2] = 1.0;
        within_defaults.control_vertices[6][2] = -1.0 + 1.0e-12;
        let within_result = evaluate_mapped_nurbs_surface(&within_defaults, 0.5, 0.5);
        assert!(
            within_result.is_ok(),
            "near-tolerance periodic seam failed: {within_result:?}"
        );
        for transverse in [0.0, 0.25, 0.75, 1.0] {
            assert!(evaluate_mapped_nurbs_surface(&within_defaults, transverse, 0.5).is_ok());
        }

        let mut endpoint_outside = minimum_count_clamped_periodic_v_surface();
        for row in 0..2 {
            endpoint_outside.control_vertices[row * 4 + 2][0] += 2.0e-8;
            endpoint_outside.control_vertices[row * 4 + 3][0] += 2.0e-8;
        }
        for transverse in [0.0, 0.5, 1.0] {
            assert!(evaluate_mapped_nurbs_surface(&endpoint_outside, transverse, 0.5).is_err());
        }

        let mut tangent_outside = minimum_count_clamped_periodic_v_surface();
        for row in 0..2 {
            tangent_outside.control_vertices[row * 4 + 2][0] += 1.0e-10;
        }
        for transverse in [0.0, 0.5, 1.0] {
            assert!(evaluate_mapped_nurbs_surface(&tangent_outside, transverse, 0.5).is_err());
        }
    }

    #[test]
    fn periodic_u_surface_seam_accepts_xt_angular_resolution_and_rejects_above_it() {
        let mut within_resolution = minimum_count_clamped_periodic_u_surface();
        for transverse in 0..2 {
            within_resolution.control_vertices[4 + transverse][1] += 9.999_995e-12;
        }
        assert!(evaluate_mapped_nurbs_surface(&within_resolution, 0.0, 0.5).is_ok());

        let mut above_resolution = minimum_count_clamped_periodic_u_surface();
        for transverse in 0..2 {
            above_resolution.control_vertices[4 + transverse][1] += 1.000_001e-11;
        }
        assert!(evaluate_mapped_nurbs_surface(&above_resolution, 0.0, 0.5).is_err());
    }

    fn rational_clamped_nonrepeated_v_surface() -> NurbsSurface {
        let mut control_vertices = Vec::new();
        for u in 0..2 {
            let base = f64::from(u);
            control_vertices.extend([
                vec![base, 0.0, 0.0, 2.0],
                vec![2.0 * base + 2.0, 0.0, 0.0, 1.0],
                vec![0.0, 1.0, 0.0, 2.0],
                vec![base / 2.0 - 1.0, 0.0, 0.0, 4.0],
                vec![base, 0.0, 0.0, 2.0],
            ]);
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 2,
            u_control_vertex_count: 2,
            v_control_vertex_count: 5,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: false,
            v_periodic: true,
            u_closed: false,
            v_closed: true,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0],
            v_knots: vec![0.0, 1.0, 3.0],
            u_knot_multiplicities: vec![2, 2],
            v_knot_multiplicities: vec![3, 2, 3],
            sources: vec![],
        }
    }

    fn nonclamped_quadratic_varying_gauge_seam() -> NurbsSurface {
        let start = [
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ];
        let end = [
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.5],
            [2.0, 0.0, 0.0, 2.0],
        ];
        let end_derivative = [
            [0.0, 0.0, 3.0, 0.0],
            [0.0, 0.0, 4.5, 0.0],
            [0.0, 0.0, 6.0, 0.0],
        ];
        let mut controls = Vec::with_capacity(18);
        let mut start_left = Vec::with_capacity(3);
        let mut start_right = Vec::with_capacity(3);
        for start_point in &start {
            start_left.push([start_point[0], 0.0, start_point[2] - 1.5, start_point[3]]);
            start_right.push([start_point[0], 0.0, start_point[2] + 1.5, start_point[3]]);
        }
        controls.extend(start_left);
        controls.extend(start_right);
        for _ in 0..3 {
            controls.push([1.0, 0.0, 3.0, 1.0]);
        }
        for _ in 0..3 {
            controls.push([1.0, 0.0, -2.0, 1.0]);
        }
        let mut end_left = Vec::with_capacity(3);
        let mut end_right = Vec::with_capacity(3);
        for index in 0..3 {
            end_left.push([
                end[index][0],
                0.0,
                end[index][2] - end_derivative[index][2] / 2.0,
                end[index][3],
            ]);
            end_right.push([
                end[index][0],
                0.0,
                end[index][2] + end_derivative[index][2] / 2.0,
                end[index][3],
            ]);
        }
        controls.extend(end_left);
        controls.extend(end_right);
        let control_vertices = controls
            .into_iter()
            .map(|control| {
                vec![
                    control[0] / control[3],
                    control[1] / control[3],
                    control[2] / control[3],
                    control[3],
                ]
            })
            .collect();
        NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_control_vertex_count: 6,
            v_control_vertex_count: 3,
            vertex_dimension: 4,
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: false,
            u_closed: true,
            v_closed: false,
            rational: true,
            surface_form: 0,
            control_vertices,
            u_knots: vec![-1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            v_knots: vec![0.0, 1.0],
            u_knot_multiplicities: vec![1; 9],
            v_knot_multiplicities: vec![3, 3],
            sources: vec![],
        }
    }

    #[test]
    fn periodic_v_surface_wraps_positive_and_negative_periods() {
        let surface = periodic_v_surface();
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 2.5).unwrap();
        assert_eq!(at, [0.0, 0.0, 0.5]);
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 0.5, 5.5).unwrap()
        );
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 0.5, -0.5).unwrap()
        );
    }

    #[test]
    fn periodic_v_surface_rejects_invalid_suffix_and_unbounded_wraps() {
        let mut surface = periodic_v_surface();
        surface.control_vertices[3][0] = 1.0;
        assert!(evaluate_mapped_nurbs_surface(&surface, 0.5, 1.5).is_err());

        let surface = periodic_v_surface();
        assert!(evaluate_mapped_nurbs_surface(&surface, 0.5, f64::MAX).is_err());
    }

    #[test]
    fn periodic_v_surface_rejects_repeated_corner_with_tangent_discontinuity() {
        let mut surface = periodic_v_surface();
        surface.control_vertices[2] = vec![0.0, 1.0, 0.0];
        assert!(evaluate_mapped_nurbs_surface(&surface, 0.5, 1.5).is_err());
    }

    #[test]
    fn clamped_nonrepeated_periodic_u_surface_requires_and_wraps_exact_seam() {
        let surface = clamped_nonrepeated_u_surface();
        assert_ne!(
            &surface.control_vertices[..surface.v_control_vertex_count as usize],
            &surface.control_vertices[(surface.u_control_vertex_count as usize - 2)
                * surface.v_control_vertex_count as usize..]
        );
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 3.5, 0.5).unwrap()
        );
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 3.0, 0.5).unwrap()
        );

        let mut broken = surface;
        broken.control_vertices[8][0] = 0.5;
        assert!(evaluate_mapped_nurbs_surface(&broken, 0.5, 0.5).is_err());
    }

    #[test]
    fn rational_periodic_surface_accepts_common_positive_homogeneous_seam_scale() {
        let surface = rational_projectively_scaled_clamped_u_surface();
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 3.5, 0.5).unwrap()
        );
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 3.0, 0.5).unwrap()
        );

        let mut endpoint_scale_mismatch = surface.clone();
        endpoint_scale_mismatch.control_vertices[9][3] = 6.0;
        assert!(evaluate_mapped_nurbs_surface(&endpoint_scale_mismatch, 0.5, 0.5).is_err());

        let mut derivative_scale_mismatch = surface.clone();
        derivative_scale_mismatch.control_vertices[6][0] = -0.5;
        assert!(evaluate_mapped_nurbs_surface(&derivative_scale_mismatch, 0.5, 0.5).is_ok());

        let mut angular_mismatch = surface.clone();
        angular_mismatch.control_vertices[6][1] += 0.5;
        assert!(evaluate_mapped_nurbs_surface(&angular_mismatch, 0.5, 0.5).is_err());

        let mut endpoint_scale_with_equal_derivative = surface;
        endpoint_scale_with_equal_derivative.control_vertices[6][0] += 0.5;
        endpoint_scale_with_equal_derivative.control_vertices[7][0] += 0.5;
        assert!(
            evaluate_mapped_nurbs_surface(&endpoint_scale_with_equal_derivative, 0.5, 0.5).is_ok()
        );
    }

    #[test]
    fn rational_periodic_surface_accepts_exact_projective_derivative_gauge() {
        let surface = rational_periodic_u_surface_with_projective_derivative_gauge();
        for transverse_x in [0.0, 1.0] {
            let start_x = 2.0 * transverse_x;
            let end_x = 4.0 * transverse_x;
            let start_derivative_x = 2.0;
            let end_derivative_x = 4.0 * transverse_x + 4.0;
            assert_eq!(start_x / 2.0, end_x / 4.0);
            assert_eq!(
                (start_derivative_x * 2.0) / 4.0,
                (end_derivative_x * 4.0 - end_x * 4.0) / 16.0
            );
        }
        let start = evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5).unwrap();
        let end = evaluate_mapped_nurbs_surface(&surface, 2.0, 0.5).unwrap();
        assert_eq!(start, [0.5, 0.0, 0.0]);
        assert_eq!(start, end);
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 2.5, 0.5).unwrap()
        );

        let mut wrong_gauge = surface;
        wrong_gauge.control_vertices[7][0] += 0.5;
        assert!(evaluate_mapped_nurbs_surface(&wrong_gauge, 0.5, 0.5).is_ok());

        let mut wrong_direction = rational_periodic_u_surface_with_projective_derivative_gauge();
        wrong_direction.control_vertices[7][1] += 0.5;
        assert!(evaluate_mapped_nurbs_surface(&wrong_direction, 0.5, 0.5).is_err());
    }

    #[test]
    fn minimum_control_count_clamped_periodic_u_surface_validates_exact_seam() {
        let surface = minimum_count_clamped_periodic_u_surface();
        assert_eq!(surface.u_degree, 3);
        assert_eq!(surface.u_control_vertex_count, 4);
        assert_ne!(surface.control_vertices[0], surface.control_vertices[2]);
        let at = evaluate_mapped_nurbs_surface(&surface, 0.25, 0.5).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 1.25, 0.5).unwrap()
        );
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.0, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 1.0, 0.5).unwrap()
        );

        let surface = minimum_count_clamped_periodic_v_surface();
        assert_eq!(surface.v_degree, 3);
        assert_eq!(surface.v_control_vertex_count, 4);
        assert_ne!(surface.control_vertices[0], surface.control_vertices[2]);
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 0.25).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 0.5, 1.25).unwrap()
        );
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 0.5, 0.0).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 0.5, 1.0).unwrap()
        );
    }

    #[test]
    fn rational_clamped_nonrepeated_periodic_v_surface_wraps_with_matching_seam() {
        let surface = rational_clamped_nonrepeated_v_surface();
        assert_ne!(
            &surface.control_vertices[..surface.v_control_vertex_count as usize],
            &surface.control_vertices[(surface.v_control_vertex_count as usize - 2)..]
        );
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 0.5).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 0.5, 3.5).unwrap()
        );

        let mut broken = surface;
        broken.control_vertices[9][3] = 3.0;
        assert!(evaluate_mapped_nurbs_surface(&broken, 0.5, 0.5).is_err());
    }

    #[test]
    fn nonclamped_quadratic_rational_periodic_u_extracts_exact_projective_seam() {
        let surface = nonclamped_quadratic_varying_gauge_seam();
        assert_eq!(surface.u_degree, 2);
        assert_eq!(surface.u_knots[surface.u_degree as usize], 1.0);
        assert_eq!(
            surface.u_knots[surface.u_control_vertex_count as usize],
            5.0
        );
        assert_ne!(
            surface.u_knots[0],
            surface.u_knots[surface.u_degree as usize]
        );
        let expected_start = [
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ];
        let expected_end = [
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.5],
            [2.0, 0.0, 0.0, 2.0],
        ];
        let expected_start_derivative = [[0.0, 0.0, 3.0, 0.0]; 3];
        let expected_end_derivative = [
            [0.0, 0.0, 3.0, 0.0],
            [0.0, 0.0, 4.5, 0.0],
            [0.0, 0.0, 6.0, 0.0],
        ];
        for transverse in 0..3 {
            let curve: Vec<[f64; 4]> = (0..6)
                .map(|u| {
                    let control = &surface.control_vertices[u * 3 + transverse];
                    [
                        control[0] * control[3],
                        control[1] * control[3],
                        control[2] * control[3],
                        control[3],
                    ]
                })
                .collect();
            assert_eq!(
                super::exact_endpoint_curve_jet(&curve, &surface.u_knots, 2, false),
                Some((
                    expected_start[transverse],
                    expected_start_derivative[transverse]
                ))
            );
            assert_eq!(
                super::exact_endpoint_curve_jet(&curve, &surface.u_knots, 2, true),
                Some((
                    expected_end[transverse],
                    expected_end_derivative[transverse]
                ))
            );
        }
        assert_eq!(
            evaluate_mapped_nurbs_surface(&surface, 1.0, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&surface, 5.0, 0.5).unwrap()
        );

        let mut perturbed = surface;
        perturbed.control_vertices[17][0] += 0.125;
        assert!(evaluate_mapped_nurbs_surface(&perturbed, 1.0, 0.5).is_err());
    }

    fn periodic_v_rational_surface() -> NurbsSurface {
        let mut surface = periodic_v_surface();
        surface.rational = true;
        surface.vertex_dimension = 4;
        for control in &mut surface.control_vertices {
            let weight = 1.0 + control[2] / 10.0;
            control.push(weight);
        }
        surface
    }

    fn periodic_uv_surface(rational: bool) -> NurbsSurface {
        let profile = [0.0, 1.0, -1.0, 0.0];
        let mut control_vertices = Vec::new();
        for u in 0..4 {
            for v in 0..4 {
                let unique_u = profile[u];
                let unique_v = profile[v];
                let mut control = vec![unique_u, unique_v, unique_u + unique_v];
                if rational {
                    control.push(2.0);
                }
                control_vertices.push(control);
            }
        }
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_control_vertex_count: 4,
            v_control_vertex_count: 4,
            vertex_dimension: if rational { 4 } else { 3 },
            u_knot_type: 0,
            v_knot_type: 0,
            u_periodic: true,
            v_periodic: true,
            u_closed: true,
            v_closed: true,
            rational,
            surface_form: 0,
            control_vertices,
            u_knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            v_knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            u_knot_multiplicities: vec![1; 6],
            v_knot_multiplicities: vec![1; 6],
            sources: vec![],
        }
    }

    #[test]
    fn periodic_uv_surfaces_wrap_both_axes_and_preserve_seams() {
        for rational in [false, true] {
            let surface = periodic_uv_surface(rational);
            let at = evaluate_mapped_nurbs_surface(&surface, 2.5, 2.5).unwrap();
            if rational {
                let expected = [0.0; 3];
                for axis in 0..3 {
                    assert!((at[axis] - expected[axis]).abs() < 1e-12);
                }
            }
            assert_eq!(
                at,
                evaluate_mapped_nurbs_surface(&surface, 5.5, -0.5).unwrap()
            );
            assert_eq!(
                evaluate_mapped_nurbs_surface(&surface, 1.0, 2.5).unwrap(),
                evaluate_mapped_nurbs_surface(&surface, 4.0, 2.5).unwrap()
            );
            assert_eq!(
                evaluate_mapped_nurbs_surface(&surface, 2.5, 1.0).unwrap(),
                evaluate_mapped_nurbs_surface(&surface, 2.5, 4.0).unwrap()
            );
        }
    }

    #[test]
    fn one_axis_rational_periodic_surfaces_evaluate_and_wrap() {
        let v_periodic = periodic_v_rational_surface();
        assert_eq!(
            evaluate_mapped_nurbs_surface(&v_periodic, 0.5, 2.5).unwrap(),
            evaluate_mapped_nurbs_surface(&v_periodic, 0.5, 5.5).unwrap()
        );

        let mut u_periodic = periodic_uv_surface(true);
        u_periodic.v_periodic = false;
        u_periodic.v_closed = false;
        u_periodic.v_knots = vec![0.0, 1.0, 3.0, 4.0];
        u_periodic.v_knot_multiplicities = vec![2, 1, 1, 2];
        assert_eq!(
            evaluate_mapped_nurbs_surface(&u_periodic, 2.5, 2.0).unwrap(),
            evaluate_mapped_nurbs_surface(&u_periodic, 5.5, 2.0).unwrap()
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "This piecewise-linear fixture produces exact coordinates at the tested domain boundaries, and each unwrap checks an in-domain evaluation."
    )]
    fn closed_nonperiodic_axes_stay_inside_their_active_domains() {
        let one_closed_axis = closed_nonperiodic_v_surface();
        assert_eq!(
            evaluate_mapped_nurbs_surface(&one_closed_axis, 0.5, 1.5).unwrap(),
            [0.0, 0.0, 0.5]
        );
        assert!(evaluate_mapped_nurbs_surface(&one_closed_axis, 0.5, 3.5).is_err());

        let both_closed_axes = closed_nonperiodic_uv_surface();
        let at = evaluate_mapped_nurbs_surface(&both_closed_axes, 0.5, 0.5).unwrap();
        assert_eq!(at, [0.5, 0.5, 0.0]);
        assert_eq!(
            evaluate_mapped_nurbs_surface(&both_closed_axes, 0.0, 0.5).unwrap(),
            evaluate_mapped_nurbs_surface(&both_closed_axes, 2.0, 0.5).unwrap()
        );
        assert_eq!(
            evaluate_mapped_nurbs_surface(&both_closed_axes, 0.5, 0.0).unwrap(),
            evaluate_mapped_nurbs_surface(&both_closed_axes, 0.5, 2.0).unwrap()
        );
        assert!(evaluate_mapped_nurbs_surface(&both_closed_axes, -0.1, 0.5).is_err());
        assert!(evaluate_mapped_nurbs_surface(&both_closed_axes, 0.5, 2.1).is_err());
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "The periodic and nonperiodic axis fixture is evaluated only at valid points, and wrapping must reproduce the exact same mapped coordinates."
    )]
    fn mixed_periodic_and_closed_nonperiodic_axes_wrap_only_periodic_axis() {
        let surface = mixed_closed_nonperiodic_u_periodic_v_surface();
        let at = evaluate_mapped_nurbs_surface(&surface, 0.5, 2.5).unwrap();
        assert_eq!(
            at,
            evaluate_mapped_nurbs_surface(&surface, 0.5, 5.5).unwrap()
        );
        assert!(evaluate_mapped_nurbs_surface(&surface, 2.1, 2.5).is_err());

        let mut periodic_without_closed = periodic_v_surface();
        periodic_without_closed.v_closed = false;
        assert!(evaluate_mapped_nurbs_surface(&periodic_without_closed, 0.5, 1.5).is_err());
    }
}
