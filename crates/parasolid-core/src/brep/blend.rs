use super::{
    BrepModel, CurveEvaluation, CurveKind, NurbsCurve, Sense, SourceNodeRef, SurfaceEvaluation,
    SurfaceKind, evaluate_curve, evaluate_curve_point,
};

const TOLERANCE: f64 = 1e-8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollingBallSection {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius_start: f64,
    radius_end: f64,
    weight_start: f64,
    weight_end: f64,
    pub angle: f64,
}

impl RollingBallSection {
    #[must_use]
    pub fn point(&self, v: f64) -> Option<[f64; 3]> {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return None;
        }
        let theta = v * self.angle;
        let r_base = (1.0 - v) * self.radius_start + v * self.radius_end;
        let w = (1.0 - v) * self.weight_start + v * self.weight_end;
        let r = r_base * w;
        Some(vec3_add(
            self.center,
            vec3_scale(
                vec3_add(
                    vec3_scale(self.x, theta.cos()),
                    vec3_scale(self.y, theta.sin()),
                ),
                r,
            ),
        ))
    }

    #[must_use]
    pub fn radius(&self) -> f64 {
        0.5 * (self.radius_start + self.radius_end)
    }
}

#[must_use]
pub fn rolling_ball_section(
    center: [f64; 3],
    tangent: [f64; 3],
    contacts: [[f64; 3]; 2],
    radius: f64,
    reverse_support_order: bool,
) -> Option<RollingBallSection> {
    rolling_ball_section_variable(
        center,
        tangent,
        contacts,
        [radius, radius],
        [1.0, 1.0],
        reverse_support_order,
    )
}

#[must_use]
pub fn rolling_ball_section_variable(
    center: [f64; 3],
    tangent: [f64; 3],
    contacts: [[f64; 3]; 2],
    radii: [f64; 2],
    thumb_weights: [f64; 2],
    reverse_support_order: bool,
) -> Option<RollingBallSection> {
    if !radii[0].is_finite()
        || !radii[1].is_finite()
        || radii[0] <= f64::EPSILON
        || radii[1] <= f64::EPSILON
        || !thumb_weights[0].is_finite()
        || !thumb_weights[1].is_finite()
        || thumb_weights[0] <= 0.0
        || thumb_weights[1] <= 0.0
    {
        return None;
    }
    let (first, second, r_start, r_end, w_start, w_end) = if reverse_support_order {
        (
            contacts[1],
            contacts[0],
            radii[1],
            radii[0],
            thumb_weights[1],
            thumb_weights[0],
        )
    } else {
        (
            contacts[0],
            contacts[1],
            radii[0],
            radii[1],
            thumb_weights[0],
            thumb_weights[1],
        )
    };
    let x_vector = vec3_sub(first, center);
    let other_vector = vec3_sub(second, center);
    if !close(vec3_norm(x_vector), r_start) || !close(vec3_norm(other_vector), r_end) {
        return None;
    }
    let tangent = unit(tangent)?;
    let x_dot = vec3_dot(tangent, x_vector) / r_start;
    let other_dot = vec3_dot(tangent, other_vector) / r_end;
    if x_dot.abs() > 1e-3 || other_dot.abs() > 1e-3 {
        return None;
    }
    let x_proj = vec3_sub(x_vector, vec3_scale(tangent, vec3_dot(tangent, x_vector)));
    let other_proj = vec3_sub(
        other_vector,
        vec3_scale(tangent, vec3_dot(tangent, other_vector)),
    );
    let x = unit(x_proj)?;
    let other = unit(other_proj)?;
    let y = unit(vec3_cross(tangent, x))?;
    let angle = vec3_dot(other, y).atan2(vec3_dot(other, x));
    if !angle.is_finite() || angle.abs() <= 1e-10 || angle.abs() >= std::f64::consts::PI - 1e-8 {
        return None;
    }
    Some(RollingBallSection {
        center,
        x,
        y,
        radius_start: r_start,
        radius_end: r_end,
        weight_start: w_start,
        weight_end: w_end,
        angle,
    })
}

#[must_use]
pub fn evaluate_spine_with_brep(
    brep: Option<&BrepModel>,
    kind: &CurveKind,
    sense: Sense,
    u: f64,
) -> Option<CurveEvaluation> {
    let mut evaluated = match kind {
        CurveKind::Line { .. }
        | CurveKind::Circle { .. }
        | CurveKind::Ellipse { .. }
        | CurveKind::Parabola { .. }
        | CurveKind::Hyperbola { .. } => {
            let mut eval = evaluate_curve(kind, u)?;
            eval.tangent = unit(eval.tangent)?;
            eval
        }
        CurveKind::Nurbs(nurbs) => {
            let (lo, hi) = nurbs_domain(nurbs)?;
            if u < lo || u > hi {
                return None;
            }
            let point = evaluate_curve_point(kind, u)?;
            let step = (1e-5 * u.abs().max(1.0)).min(1e-3);
            let u0 = (u - step).max(lo);
            let u1 = (u + step).min(hi);
            if u0 >= u1 {
                return None;
            }
            let p0 = evaluate_curve_point(kind, u0)?;
            let p1 = evaluate_curve_point(kind, u1)?;
            CurveEvaluation {
                point,
                tangent: unit(vec3_sub(p1, p0))?,
            }
        }
        CurveKind::Trimmed {
            basis_curve,
            start_parameter,
            end_parameter,
            ..
        } => {
            let brep_ref = brep?;
            let basis = brep_ref.curves.iter().find(|c| c.id == *basis_curve)?;
            let mut mapped_u = u;
            if start_parameter.is_finite() && end_parameter.is_finite() {
                let min_p = start_parameter.min(*end_parameter);
                let max_p = start_parameter.max(*end_parameter);
                if (0.0..=1.0).contains(&mapped_u) && (mapped_u < min_p || mapped_u > max_p) {
                    mapped_u = start_parameter + mapped_u * (end_parameter - start_parameter);
                }
            }
            let comb_sense = match (sense, basis.sense) {
                (Sense::Negative, Sense::Positive) | (Sense::Positive, Sense::Negative) => {
                    Sense::Negative
                }
                (Sense::Negative, Sense::Negative) | (Sense::Positive, Sense::Positive) => {
                    Sense::Positive
                }
                _ => Sense::Unknown,
            };
            return evaluate_spine_with_brep(Some(brep_ref), &basis.kind, comb_sense, mapped_u);
        }
        _ => {
            let point = evaluate_curve_point(kind, u)?;
            let step = (1e-5 * u.abs().max(1.0)).min(1e-3);
            let p0 = evaluate_curve_point(kind, u - step);
            let p1 = evaluate_curve_point(kind, u + step);
            let tangent = match (p0, p1) {
                (Some(p0), Some(p1)) => unit(vec3_sub(p1, p0))?,
                (Some(p0), None) => unit(vec3_sub(point, p0))?,
                (None, Some(p1)) => unit(vec3_sub(p1, point))?,
                (None, None) => return None,
            };
            CurveEvaluation { point, tangent }
        }
    };
    if sense == Sense::Negative {
        evaluated.tangent = vec3_scale(evaluated.tangent, -1.0);
    }
    Some(evaluated)
}

#[must_use]
pub fn evaluate_spine(kind: &CurveKind, sense: Sense, u: f64) -> Option<CurveEvaluation> {
    evaluate_spine_with_brep(None, kind, sense, u)
}

#[must_use]
pub fn evaluate_spine_in_brep(
    brep: &BrepModel,
    kind: &CurveKind,
    sense: Sense,
    u: f64,
) -> Option<CurveEvaluation> {
    evaluate_spine_with_brep(Some(brep), kind, sense, u)
}

#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "The public rolling-ball evaluator keeps its established parameter-based API for compatibility."
)]
pub fn evaluate_rolling_ball(
    brep: &BrepModel,
    supporting_surfaces: [u32; 2],
    spine_curve: u32,
    ranges: [f64; 2],
    thumb_weights: [f64; 2],
    boundary_surfaces: [Option<u32>; 2],
    start: Option<&SourceNodeRef>,
    end: Option<&SourceNodeRef>,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    let spine = brep.curves.iter().find(|curve| curve.id == spine_curve)?;
    evaluate_rolling_ball_with_spine(
        brep,
        supporting_surfaces,
        spine.sense,
        ranges,
        thumb_weights,
        boundary_surfaces,
        start,
        end,
        u,
        v,
        |parameter| evaluate_spine_in_brep(brep, &spine.kind, spine.sense, parameter),
    )
}

#[must_use]
#[allow(
    clippy::float_cmp,
    reason = "Exact thumb-weight equality restricts this evaluator to the supported unweighted type."
)]
#[allow(
    clippy::too_many_arguments,
    reason = "The public rolling-ball evaluator keeps its established parameter-based API for compatibility."
)]
pub fn evaluate_rolling_ball_with_spine(
    brep: &BrepModel,
    supporting_surfaces: [u32; 2],
    spine_sense: Sense,
    ranges: [f64; 2],
    thumb_weights: [f64; 2],
    boundary_surfaces: [Option<u32>; 2],
    start: Option<&SourceNodeRef>,
    end: Option<&SourceNodeRef>,
    u: f64,
    v: f64,
    evaluate_spine_at: impl Fn(f64) -> Option<CurveEvaluation>,
) -> Option<SurfaceEvaluation> {
    if !u.is_finite()
        || !v.is_finite()
        || !(0.0..=1.0).contains(&v)
        || ranges.iter().any(|x| !x.is_finite())
        || thumb_weights.iter().any(|x| !x.is_finite() || *x <= 0.0)
        || boundary_surfaces != [None, None]
        || start.is_some()
        || end.is_some()
    {
        return None;
    }
    // Range signs offset each support; magnitudes set the common section radius.
    let r0 = ranges[0].abs();
    let r1 = ranges[1].abs();
    if r0 <= f64::EPSILON
        || r1 <= f64::EPSILON
        || (r0 - r1).abs() > 1e-12 * r0.max(r1)
        || thumb_weights != [1.0, 1.0]
    {
        return None;
    }
    let supports = [
        brep.surfaces
            .iter()
            .find(|s| s.id == supporting_surfaces[0])?,
        brep.surfaces
            .iter()
            .find(|s| s.id == supporting_surfaces[1])?,
    ];
    let point_at = |u: f64, v: f64| {
        let curve = evaluate_spine_at(u)?;
        let center = curve.point;
        let contacts = [
            support_contact(supports[0].sense, &supports[0].kind, center, ranges[0], r0)?,
            support_contact(supports[1].sense, &supports[1].kind, center, ranges[1], r1)?,
        ];
        rolling_ball_section(
            center,
            curve.tangent,
            [contacts[0].0, contacts[1].0],
            r0,
            spine_sense == Sense::Negative,
        )?
        .point(v)
    };

    let point = point_at(u, v)?;
    let du = (1e-5 * u.abs().max(1.0)).min(1e-3);
    let before = point_at(u - du, v);
    let after = point_at(u + du, v);
    let (point_0, point_1, span) = match (before, after) {
        (Some(before), Some(after)) => (before, after, 2.0 * du),
        (Some(before), None) => (before, point, du),
        (None, Some(after)) => (point, after, du),
        (None, None) => return None,
    };
    let v0 = (v - 1e-5).max(0.0);
    let v1 = (v + 1e-5).min(1.0);
    let tangent_u = vec3_scale(vec3_sub(point_1, point_0), 1.0 / span);
    let tangent_v = vec3_scale(
        vec3_sub(point_at(u, v1)?, point_at(u, v0)?),
        1.0 / (v1 - v0),
    );
    let normal = unit(vec3_cross(tangent_u, tangent_v))?;
    Some(SurfaceEvaluation { point, normal })
}

fn nurbs_domain(curve: &NurbsCurve) -> Option<(f64, f64)> {
    let degree = usize::from(curve.degree);
    let count = usize::try_from(curve.control_vertex_count).ok()?;
    if curve.periodic
        || degree == 0
        || count <= degree
        || count > 1_048_576
        || curve.control_vertices.len() != count
        || curve.knots.len() != curve.knot_multiplicities.len()
    {
        return None;
    }
    let expected = count.checked_add(degree)?.checked_add(1)?;
    if curve.knots.len() > expected {
        return None;
    }
    let mut knots = Vec::with_capacity(expected);
    for (&knot, &multiplicity) in curve.knots.iter().zip(&curve.knot_multiplicities) {
        if !knot.is_finite() || multiplicity == 0 {
            return None;
        }
        let next = knots.len().checked_add(usize::from(multiplicity))?;
        if next > expected {
            return None;
        }
        knots.resize(next, knot);
    }
    if knots.len() != expected {
        return None;
    }
    let lo = *knots.get(degree)?;
    let hi = *knots.get(count)?;
    (lo.is_finite() && hi.is_finite() && lo < hi).then_some((lo, hi))
}

fn support_contact(
    sense: Sense,
    surface: &SurfaceKind,
    center: [f64; 3],
    range: f64,
    radius: f64,
) -> Option<([f64; 3], [f64; 3])> {
    let signed_offset = if sense == Sense::Negative {
        -range
    } else {
        range
    };
    let (contact, normal) = match surface {
        SurfaceKind::Plane { point, normal, .. } => {
            let normal = unit(normal.to_array())?;
            let distance = vec3_dot(vec3_sub(center, point.to_array()), normal);
            if !close(distance, signed_offset) {
                return None;
            }
            (vec3_sub(center, vec3_scale(normal, distance)), normal)
        }
        SurfaceKind::Cylinder {
            point,
            axis,
            radius: cylinder_radius,
            ..
        } => {
            let axis = unit(axis.to_array())?;
            if !cylinder_radius.is_finite() || *cylinder_radius <= 0.0 {
                return None;
            }
            let delta = vec3_sub(center, point.to_array());
            let station = vec3_dot(delta, axis);
            let radial = vec3_sub(delta, vec3_scale(axis, station));
            let radial_distance = vec3_norm(radial);
            let offset_radius = cylinder_radius + signed_offset;
            if !radial_distance.is_finite()
                || radial_distance <= f64::EPSILON
                || offset_radius <= f64::EPSILON
                || !close(radial_distance, offset_radius)
            {
                return None;
            }
            let normal = vec3_scale(radial, 1.0 / radial_distance);
            (
                vec3_add(
                    vec3_add(point.to_array(), vec3_scale(axis, station)),
                    vec3_scale(normal, *cylinder_radius),
                ),
                normal,
            )
        }
        SurfaceKind::Sphere {
            center: sphere_center,
            radius: sphere_radius,
            ..
        } => {
            if !sphere_radius.is_finite() || *sphere_radius <= 0.0 {
                return None;
            }
            let radial = vec3_sub(center, sphere_center.to_array());
            let radial_distance = vec3_norm(radial);
            let offset_radius = sphere_radius + signed_offset;
            if !offset_radius.is_finite()
                || offset_radius <= f64::EPSILON
                || !close(radial_distance, offset_radius)
            {
                return None;
            }
            let normal = unit(radial)?;
            (
                vec3_add(sphere_center.to_array(), vec3_scale(normal, *sphere_radius)),
                normal,
            )
        }
        SurfaceKind::Cone {
            point,
            axis,
            radius: cone_radius,
            sin_half_angle,
            cos_half_angle,
            ..
        } => {
            if !cone_radius.is_finite()
                || !sin_half_angle.is_finite()
                || !cos_half_angle.is_finite()
                || *cos_half_angle <= 0.0
                || !close(
                    sin_half_angle * sin_half_angle + cos_half_angle * cos_half_angle,
                    1.0,
                )
            {
                return None;
            }
            let axis = unit(axis.to_array())?;
            let delta = vec3_sub(center, point.to_array());
            let station = vec3_dot(delta, axis);
            let radial = vec3_sub(delta, vec3_scale(axis, station));
            let radial_distance = vec3_norm(radial);
            if !radial_distance.is_finite() || radial_distance <= f64::EPSILON {
                return None;
            }
            let normal = vec3_add(
                vec3_scale(radial, *cos_half_angle / radial_distance),
                vec3_scale(axis, -*sin_half_angle),
            );
            let normal = unit(normal)?;
            let contact_station = station + signed_offset * sin_half_angle;
            let contact_radius = cone_radius + contact_station * (sin_half_angle / cos_half_angle);
            if !contact_radius.is_finite() || contact_radius <= f64::EPSILON {
                return None;
            }
            (
                vec3_add(
                    vec3_add(point.to_array(), vec3_scale(axis, contact_station)),
                    vec3_scale(radial, contact_radius / radial_distance),
                ),
                normal,
            )
        }
        SurfaceKind::Torus {
            center: torus_center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            if !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
                || major_radius <= minor_radius
            {
                return None;
            }
            let axis = unit(axis.to_array())?;
            let delta = vec3_sub(center, torus_center.to_array());
            let station = vec3_dot(delta, axis);
            let radial = vec3_sub(delta, vec3_scale(axis, station));
            let radial_distance = vec3_norm(radial);
            let offset_radius = minor_radius + signed_offset;
            if !radial_distance.is_finite()
                || radial_distance <= f64::EPSILON
                || !offset_radius.is_finite()
                || offset_radius <= f64::EPSILON
                || *major_radius <= offset_radius
            {
                return None;
            }
            let major_direction = vec3_scale(radial, 1.0 / radial_distance);
            let tube_center = vec3_add(
                torus_center.to_array(),
                vec3_scale(major_direction, *major_radius),
            );
            let normal = unit(vec3_sub(center, tube_center))?;
            let tube_distance = vec3_norm(vec3_sub(center, tube_center));
            if !close(tube_distance, offset_radius) {
                return None;
            }
            (
                vec3_add(tube_center, vec3_scale(normal, *minor_radius)),
                normal,
            )
        }
        _ => return None,
    };
    if !close(vec3_norm(vec3_sub(center, contact)), radius)
        || !close_vec(center, vec3_add(contact, vec3_scale(normal, signed_offset)))
    {
        return None;
    }
    Some((contact, normal))
}

fn unit(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = vec3_norm(vector);
    if !length.is_finite() || length <= f64::EPSILON {
        None
    } else {
        Some(vec3_scale(vector, 1.0 / length))
    }
}

fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= TOLERANCE * a.abs().max(b.abs()).max(1.0)
}

fn close_vec(a: [f64; 3], b: [f64; 3]) -> bool {
    vec3_norm(vec3_sub(a, b)) <= TOLERANCE * a.iter().chain(&b).fold(1.0_f64, |s, x| s.max(x.abs()))
}

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
fn vec3_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn vec3_norm(a: [f64; 3]) -> f64 {
    vec3_dot(a, a).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{
        BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, Sense, SourceNodeRef,
        SurfaceGeometry, SurfaceKind, TopologyValidation, Vector3,
    };

    fn test_source() -> SourceNodeRef {
        SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        }
    }

    fn test_brep(curves: Vec<CurveGeometry>, surfaces: Vec<SurfaceGeometry>) -> BrepModel {
        BrepModel {
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
            curves,
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
    fn test_evaluate_spine_parabola() {
        let parabola = CurveKind::Parabola {
            origin: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            focal_length: 1.0,
        };
        // At u = 2.0: x = 2^2 / (4*1) = 1.0, y = 2.0, z = 0.0
        let eval = evaluate_spine(&parabola, Sense::Positive, 2.0).expect("parabola eval");
        assert!((eval.point[0] - 1.0).abs() < 1e-9);
        assert!((eval.point[1] - 2.0).abs() < 1e-9);
        assert!((eval.point[2] - 0.0).abs() < 1e-9);
        // Tangent: unit(Y + (2 / 2f) X) = unit([1, 1, 0])
        let inv_sqrt2 = 1.0 / 2.0_f64.sqrt();
        assert!((eval.tangent[0] - inv_sqrt2).abs() < 1e-6);
        assert!((eval.tangent[1] - inv_sqrt2).abs() < 1e-6);
        assert!((eval.tangent[2] - 0.0).abs() < 1e-6);

        // Test negative sense reverses tangent
        let eval_neg = evaluate_spine(&parabola, Sense::Negative, 2.0).expect("parabola eval neg");
        assert!((eval_neg.tangent[0] - (-inv_sqrt2)).abs() < 1e-6);
        assert!((eval_neg.tangent[1] - (-inv_sqrt2)).abs() < 1e-6);
    }

    #[test]
    fn test_evaluate_spine_hyperbola() {
        let hyperbola = CurveKind::Hyperbola {
            origin: Vector3::from_array([1.0, 2.0, 3.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            transverse_radius: 2.0,
            conjugate_radius: 3.0,
        };
        // At u = 0.0: cosh(0) = 1, sinh(0) = 0 => point = [1 + 2*1, 2 + 0, 3] = [3, 2, 3]
        let eval = evaluate_spine(&hyperbola, Sense::Positive, 0.0).expect("hyperbola eval");
        assert!((eval.point[0] - 3.0).abs() < 1e-9);
        assert!((eval.point[1] - 2.0).abs() < 1e-9);
        assert!((eval.point[2] - 3.0).abs() < 1e-9);
        // Tangent: unit(a*sinh(0)*X + b*cosh(0)*Y) = unit([0, 3, 0]) = [0, 1, 0]
        assert!((eval.tangent[0] - 0.0).abs() < 1e-9);
        assert!((eval.tangent[1] - 1.0).abs() < 1e-9);
        assert!((eval.tangent[2] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_evaluate_spine_trimmed_curve() {
        let parabola = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Parabola {
                origin: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 0.0, 1.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
                focal_length: 1.0,
            },
            source: test_source(),
        };
        let trimmed = CurveGeometry {
            id: 2,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_point: Vector3::from_array([0.0, 0.0, 0.0]),
                end_point: Vector3::from_array([4.0, 4.0, 0.0]),
                start_parameter: 0.0,
                end_parameter: 4.0,
            },
            source: test_source(),
        };
        let brep = test_brep(vec![parabola.clone(), trimmed.clone()], vec![]);

        // At u = 2.0, direct parameter within [0.0, 4.0] evaluates parabola at 2.0
        let eval = evaluate_spine_in_brep(&brep, &trimmed.kind, Sense::Positive, 2.0)
            .expect("trimmed eval direct");
        assert!((eval.point[0] - 1.0).abs() < 1e-9);
        assert!((eval.point[1] - 2.0).abs() < 1e-9);
        assert!((eval.point[2] - 0.0).abs() < 1e-9);
        let inv_sqrt2 = 1.0 / 2.0_f64.sqrt();
        assert!((eval.tangent[0] - inv_sqrt2).abs() < 1e-6);
        assert!((eval.tangent[1] - inv_sqrt2).abs() < 1e-6);

        // For trimmed curve with offset domain [2.0, 6.0], normalized u in [0, 1] maps to domain
        let trimmed_offset = CurveGeometry {
            id: 3,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Trimmed {
                basis_curve: 1,
                start_point: Vector3::from_array([1.0, 2.0, 0.0]),
                end_point: Vector3::from_array([9.0, 6.0, 0.0]),
                start_parameter: 2.0,
                end_parameter: 6.0,
            },
            source: test_source(),
        };
        let brep_offset = test_brep(vec![parabola, trimmed_offset.clone()], vec![]);
        // u = 0.0 is < 2.0, maps to 2.0
        let eval_norm =
            evaluate_spine_in_brep(&brep_offset, &trimmed_offset.kind, Sense::Positive, 0.0)
                .expect("trimmed eval normalized");
        assert!((eval_norm.point[0] - 1.0).abs() < 1e-9);
        assert!((eval_norm.point[1] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_evaluate_rolling_ball_equal_radius_on_parabola_spine() {
        // Spine: Parabola in XZ plane with vertex at origin, axis along Y, x_axis along X.
        // At u = 0: point = [0, 0, 0], tangent = [0, 0, 1] (along Z).
        let spine = CurveGeometry {
            id: 1,
            sense: Sense::Positive,
            owner: None,
            kind: CurveKind::Parabola {
                origin: Vector3::from_array([0.0, 0.0, 0.0]),
                normal: Vector3::from_array([0.0, 1.0, 0.0]), // normal is Y
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]), // x_axis is X
                focal_length: 1.0,
            },
            source: test_source(),
        };

        // Support 0: Plane x = -1.0, normal [1, 0, 0]
        let s0 = SurfaceGeometry {
            id: 10,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Plane {
                point: Vector3::from_array([-1.0, 0.0, 0.0]),
                normal: Vector3::from_array([1.0, 0.0, 0.0]),
                x_axis: Vector3::from_array([0.0, 1.0, 0.0]),
            },
            source: test_source(),
        };
        // Support 1: Plane y = 1.0, normal [0, 1, 0]
        let s1 = SurfaceGeometry {
            id: 20,
            sense: Sense::Positive,
            owner: None,
            kind: SurfaceKind::Plane {
                point: Vector3::from_array([0.0, 1.0, 0.0]),
                normal: Vector3::from_array([0.0, 1.0, 0.0]),
                x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            },
            source: test_source(),
        };

        let brep = test_brep(vec![spine.clone()], vec![s0, s1]);

        let evaluate = |ranges, thumb_weights, v| {
            evaluate_rolling_ball_with_spine(
                &brep,
                [10, 20],
                Sense::Positive,
                ranges,
                thumb_weights,
                [None, None],
                None,
                None,
                0.0,
                v,
                |p| evaluate_spine(&spine.kind, spine.sense, p),
            )
        };
        let eval_start = evaluate([1.0, -1.0], [1.0, 1.0], 0.0).expect("rolling ball eval at v=0");
        let eval_mid = evaluate([1.0, -1.0], [1.0, 1.0], 0.5).expect("rolling ball eval at v=0.5");
        let eval_end = evaluate([1.0, -1.0], [1.0, 1.0], 1.0).expect("rolling ball eval at v=1");

        assert!((eval_start.point[0] + 1.0).abs() < 1e-4);
        assert!(eval_start.point[1].abs() < 1e-4);
        assert!(eval_mid.point[0] < 0.0 && eval_mid.point[1] > 0.0);
        assert!(eval_end.point[0].abs() < 1e-4);
        assert!((eval_end.point[1] - 1.0).abs() < 1e-4);
        for evaluation in [eval_start, eval_mid, eval_end] {
            assert!((vec3_norm(evaluation.point) - 1.0).abs() < 1e-4);
        }
        assert!(eval_mid.normal[0].is_finite() && eval_mid.normal[1].is_finite());
        assert!(evaluate([1.0, -2.0], [1.0, 1.0], 0.5).is_none());
        assert!(evaluate([1.0, -1.0], [1.0, 2.0], 0.5).is_none());
    }
}
