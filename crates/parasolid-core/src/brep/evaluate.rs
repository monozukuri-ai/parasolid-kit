//! Generic evaluation helpers for mapped Parasolid B-rep curves and surfaces.

/// Result of evaluating a 3D surface at parametric coordinates `(u, v)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceEvaluation {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// Result of evaluating a 3D curve at parameter `t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveEvaluation {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
}

/// Evaluates a mapped Parasolid surface at parametric coordinates `(u, v)`.
///
/// Supported analytical surfaces:
/// - `Plane { point, normal, x_axis }`
/// - `Cylinder { point, axis, radius, x_axis }`
/// - `Sphere { center, radius, axis, x_axis }`
/// - `Cone { point, axis, radius, sin_half_angle, cos_half_angle, x_axis }`
/// - `XtCone { point, axis, radius, sin_half_angle, cos_half_angle, x_axis }`
/// - `Torus { center, axis, major_radius, minor_radius, x_axis }`
#[must_use]
pub fn evaluate_surface(
    surface: &crate::brep::SurfaceKind,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    match surface {
        crate::brep::SurfaceKind::Plane {
            point,
            normal,
            x_axis,
        } => {
            let (basis_x, basis_y, normal) =
                orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
            let pt = vec3_add(
                point.to_array(),
                vec3_add(vec3_scale(basis_x, u), vec3_scale(basis_y, v)),
            );
            Some(SurfaceEvaluation { point: pt, normal })
        }
        crate::brep::SurfaceKind::Cylinder {
            point,
            axis,
            radius,
            x_axis,
        } => {
            let (basis_x, basis_y, axis) =
                orthonormal_frame_from_axis_and_ref(axis.to_array(), x_axis.to_array());
            let radial = vec3_add(vec3_scale(basis_x, u.cos()), vec3_scale(basis_y, u.sin()));
            let pt = vec3_add(
                point.to_array(),
                vec3_add(vec3_scale(axis, v), vec3_scale(radial, *radius)),
            );
            Some(SurfaceEvaluation {
                point: pt,
                normal: radial,
            })
        }
        crate::brep::SurfaceKind::Sphere {
            center,
            radius,
            axis,
            x_axis,
        } => {
            let (basis_x, basis_y, basis_z) =
                orthonormal_frame_from_axis_and_ref(axis.to_array(), x_axis.to_array());
            let rad_dir = vec3_add(
                vec3_add(
                    vec3_scale(basis_x, v.cos() * u.cos()),
                    vec3_scale(basis_y, v.cos() * u.sin()),
                ),
                vec3_scale(basis_z, v.sin()),
            );
            let pt = vec3_add(center.to_array(), vec3_scale(rad_dir, *radius));
            Some(SurfaceEvaluation {
                point: pt,
                normal: rad_dir,
            })
        }
        crate::brep::SurfaceKind::Cone { .. } => evaluate_cone_surface(surface, u, v),
        crate::brep::SurfaceKind::XtCone { .. } => evaluate_xt_cone_surface(surface, u, v),
        crate::brep::SurfaceKind::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            x_axis,
        } => {
            let (basis_x, basis_y, axis) =
                orthonormal_frame_from_axis_and_ref(axis.to_array(), x_axis.to_array());
            let major_dir = vec3_add(vec3_scale(basis_x, u.cos()), vec3_scale(basis_y, u.sin()));
            let outward = vec3_add(vec3_scale(major_dir, v.cos()), vec3_scale(axis, v.sin()));
            let pt = vec3_add(
                vec3_add(center.to_array(), vec3_scale(major_dir, *major_radius)),
                vec3_scale(outward, *minor_radius),
            );
            Some(SurfaceEvaluation {
                point: pt,
                normal: outward,
            })
        }
        crate::brep::SurfaceKind::Nurbs(nurbs) => {
            let prepared = super::nurbs_surface::PreparedNurbsSurface::new(nurbs).ok()?;
            evaluate_prepared_nurbs_surface(nurbs, &prepared, u, v)
        }
        _ => None,
    }
}

fn evaluate_cone_surface(
    surface: &crate::brep::SurfaceKind,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    let crate::brep::SurfaceKind::Cone {
        point,
        axis,
        radius,
        sin_half_angle,
        cos_half_angle,
        x_axis,
    } = surface
    else {
        return None;
    };
    let (basis_x, basis_y, axis) =
        orthonormal_frame_from_axis_and_ref(axis.to_array(), x_axis.to_array());
    let radial = vec3_add(vec3_scale(basis_x, u.cos()), vec3_scale(basis_y, u.sin()));
    let tan_half_angle = if cos_half_angle.abs() > 1e-12 {
        sin_half_angle / cos_half_angle
    } else {
        0.0
    };
    let section_radius = radius + v * tan_half_angle;
    let point = vec3_add(
        vec3_add(point.to_array(), vec3_scale(axis, v)),
        vec3_scale(radial, section_radius),
    );
    let normal = vec3_normalize(vec3_sub(
        vec3_scale(radial, *cos_half_angle),
        vec3_scale(axis, *sin_half_angle),
    ));
    Some(SurfaceEvaluation { point, normal })
}

fn evaluate_xt_cone_surface(
    surface: &crate::brep::SurfaceKind,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    let crate::brep::SurfaceKind::XtCone {
        point,
        axis,
        radius,
        sin_half_angle,
        cos_half_angle,
        x_axis,
    } = surface
    else {
        return None;
    };
    let axis = axis.to_array();
    let point = point.to_array();
    let x_axis = x_axis.to_array();
    let angle_norm = sin_half_angle.hypot(*cos_half_angle);
    if !u.is_finite()
        || !v.is_finite()
        || !radius.is_finite()
        || !sin_half_angle.is_finite()
        || !cos_half_angle.is_finite()
        || !angle_norm.is_finite()
        || angle_norm <= 1e-12
        || cos_half_angle.abs() <= 1e-12
        || !point.into_iter().all(f64::is_finite)
        || !axis.into_iter().all(f64::is_finite)
        || !x_axis.into_iter().all(f64::is_finite)
        || vec3_norm(axis) <= 1e-12
    {
        return None;
    }
    let (basis_x, basis_y, axis) = orthonormal_frame_from_axis_and_ref(axis, x_axis);
    let radial = vec3_add(vec3_scale(basis_x, u.cos()), vec3_scale(basis_y, u.sin()));
    let tan_half_angle = sin_half_angle / cos_half_angle;
    let section_radius = radius + v * tan_half_angle;
    let normal_scale = -section_radius;
    if !tan_half_angle.is_finite() || !section_radius.is_finite() || section_radius.abs() <= 1e-12 {
        return None;
    }
    let point = vec3_add(
        vec3_add(point, vec3_scale(axis, -v)),
        vec3_scale(radial, section_radius),
    );
    let normal = vec3_normalize(vec3_scale(
        vec3_add(radial, vec3_scale(axis, tan_half_angle)),
        normal_scale,
    ));
    (point.into_iter().all(f64::is_finite) && normal.into_iter().all(f64::is_finite))
        .then_some(SurfaceEvaluation { point, normal })
}

pub(super) fn evaluate_prepared_nurbs_surface(
    surface: &crate::brep::NurbsSurface,
    prepared: &super::nurbs_surface::PreparedNurbsSurface,
    u: f64,
    v: f64,
) -> Option<SurfaceEvaluation> {
    let point = prepared.point(u, v).ok()?;
    let (u_min, u_max, v_min, v_max) = prepared.active_bounds();
    let du = nurbs_tangent(
        prepared,
        u,
        v,
        (u_min, u_max),
        surface.u_periodic,
        true,
        point,
    )?;
    let dv = nurbs_tangent(
        prepared,
        u,
        v,
        (v_min, v_max),
        surface.v_periodic,
        false,
        point,
    )?;
    let normal = normalize_nonzero(vec3_cross(du, dv)).unwrap_or([0.0, 0.0, 1.0]);
    Some(SurfaceEvaluation { point, normal })
}

fn nurbs_tangent(
    prepared: &super::nurbs_surface::PreparedNurbsSurface,
    u: f64,
    v: f64,
    (min, max): (f64, f64),
    periodic: bool,
    along_u: bool,
    point: [f64; 3],
) -> Option<[f64; 3]> {
    let parameter = if along_u { u } else { v };
    let step = (max - min) * 1e-4;
    let evaluate = |parameter| {
        if along_u {
            prepared.point(parameter, v)
        } else {
            prepared.point(u, parameter)
        }
    };
    if !step.is_finite() || step <= 0.0 {
        return None;
    }
    let (lower, upper) = if periodic || (parameter - step >= min && parameter + step <= max) {
        (
            evaluate(parameter - step).ok()?,
            evaluate(parameter + step).ok()?,
        )
    } else if parameter + step <= max {
        (point, evaluate(parameter + step).ok()?)
    } else if parameter - step >= min {
        (evaluate(parameter - step).ok()?, point)
    } else {
        return None;
    };
    normalize_nonzero(vec3_sub(upper, lower))
}

fn normalize_nonzero(vector: [f64; 3]) -> Option<[f64; 3]> {
    let scale = vector.into_iter().map(f64::abs).fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    let scaled = vector.map(|component| component / scale);
    let length = vec3_norm(scaled);
    (length.is_finite() && length > 0.0).then(|| scaled.map(|component| component / length))
}

pub(super) fn plane_uv_from_point(
    surface: &crate::brep::SurfaceKind,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    let crate::brep::SurfaceKind::Plane {
        point: origin,
        normal,
        x_axis,
    } = surface
    else {
        return None;
    };
    let origin = origin.to_array();
    if !origin
        .into_iter()
        .chain(normal.to_array())
        .chain(x_axis.to_array())
        .chain(point)
        .all(f64::is_finite)
    {
        return None;
    }
    let (x, y, _) = orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
    let delta = vec3_sub(point, origin);
    let uv = [vec3_dot(delta, x), vec3_dot(delta, y)];
    uv.into_iter().all(f64::is_finite).then_some(uv)
}

/// Evaluates a surface support at the raw parametric coordinates `(u, v)`.
///
/// Analytic surfaces use [`evaluate_surface`]; mapped NURBS surfaces use the
/// bounded mapped-surface evaluator. This evaluates only the support surface and does
/// not test whether `(u, v)` lies inside a trimmed face.
#[must_use]
pub fn evaluate_surface_point(
    surface: &crate::brep::SurfaceKind,
    u: f64,
    v: f64,
) -> Option<[f64; 3]> {
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    match surface {
        crate::brep::SurfaceKind::Nurbs(nurbs) => {
            crate::brep::evaluate_mapped_nurbs_surface(nurbs, u, v).ok()
        }
        _ => evaluate_surface(surface, u, v).map(|evaluation| evaluation.point),
    }
}

/// Evaluates an analytical Parasolid curve at parameter `t`.
///
/// Supported analytical curves:
/// - `Line { point, direction }`
/// - `Circle { center, normal, x_axis, radius }`
/// - `Ellipse { center, normal, x_axis, major_radius, minor_radius }`
/// - `Parabola { origin, normal, x_axis, focal_length }`
/// - `Hyperbola { origin, normal, x_axis, transverse_radius, conjugate_radius }`
#[must_use]
pub fn evaluate_curve(curve: &crate::brep::CurveKind, t: f64) -> Option<CurveEvaluation> {
    match curve {
        crate::brep::CurveKind::Line { point, direction } => {
            let dir = direction.to_array();
            let pt = vec3_add(point.to_array(), vec3_scale(dir, t));
            Some(CurveEvaluation {
                point: pt,
                tangent: dir,
            })
        }
        crate::brep::CurveKind::Circle {
            center,
            normal,
            x_axis,
            radius,
        } => {
            let (basis_x, basis_y, _normal) =
                orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
            let pt = vec3_add(
                center.to_array(),
                vec3_scale(
                    vec3_add(vec3_scale(basis_x, t.cos()), vec3_scale(basis_y, t.sin())),
                    *radius,
                ),
            );
            let tangent = vec3_scale(
                vec3_add(vec3_scale(basis_x, -t.sin()), vec3_scale(basis_y, t.cos())),
                *radius,
            );
            Some(CurveEvaluation { point: pt, tangent })
        }
        crate::brep::CurveKind::Ellipse {
            center,
            normal,
            x_axis,
            major_radius,
            minor_radius,
        } => {
            let (basis_x, basis_y, _normal) =
                orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
            let pt = vec3_add(
                center.to_array(),
                vec3_add(
                    vec3_scale(basis_x, *major_radius * t.cos()),
                    vec3_scale(basis_y, *minor_radius * t.sin()),
                ),
            );
            let tangent = vec3_add(
                vec3_scale(basis_x, -*major_radius * t.sin()),
                vec3_scale(basis_y, *minor_radius * t.cos()),
            );
            Some(CurveEvaluation { point: pt, tangent })
        }
        crate::brep::CurveKind::Parabola {
            origin,
            normal,
            x_axis,
            focal_length,
        } => evaluate_parabola(origin, normal, x_axis, *focal_length, t),
        crate::brep::CurveKind::Hyperbola {
            origin,
            normal,
            x_axis,
            transverse_radius,
            conjugate_radius,
        } => evaluate_hyperbola(
            origin,
            normal,
            x_axis,
            *transverse_radius,
            *conjugate_radius,
            t,
        ),
        crate::brep::CurveKind::Nurbs(nurbs) => {
            let pt = crate::brep::evaluate_mapped_nurbs_curve_candidate(nurbs, t).ok()?;
            let eps = 1e-4;
            let pt1 =
                crate::brep::evaluate_mapped_nurbs_curve_candidate(nurbs, t + eps).unwrap_or(pt);
            let pt0 =
                crate::brep::evaluate_mapped_nurbs_curve_candidate(nurbs, t - eps).unwrap_or(pt);
            let dt = vec3_scale(vec3_sub(pt1, pt0), 1.0 / (2.0 * eps));
            let tangent = if vec3_norm(dt) > 1e-12 {
                vec3_normalize(dt)
            } else {
                [1.0, 0.0, 0.0]
            };
            Some(CurveEvaluation { point: pt, tangent })
        }
        _ => None,
    }
}

fn evaluate_hyperbola(
    origin: &crate::brep::Vector3,
    normal: &crate::brep::Vector3,
    x_axis: &crate::brep::Vector3,
    transverse_radius: f64,
    conjugate_radius: f64,
    parameter: f64,
) -> Option<CurveEvaluation> {
    if !parameter.is_finite()
        || !transverse_radius.is_finite()
        || !conjugate_radius.is_finite()
        || transverse_radius <= 1e-12
        || conjugate_radius <= 1e-12
    {
        return None;
    }
    let (basis_x, basis_y, _normal) =
        orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
    let cosh_parameter = parameter.cosh();
    let sinh_parameter = parameter.sinh();
    let point = vec3_add(
        origin.to_array(),
        vec3_add(
            vec3_scale(basis_x, transverse_radius * cosh_parameter),
            vec3_scale(basis_y, conjugate_radius * sinh_parameter),
        ),
    );
    let tangent = vec3_add(
        vec3_scale(basis_x, transverse_radius * sinh_parameter),
        vec3_scale(basis_y, conjugate_radius * cosh_parameter),
    );
    (point.into_iter().all(f64::is_finite) && tangent.into_iter().all(f64::is_finite))
        .then_some(CurveEvaluation { point, tangent })
}

fn evaluate_parabola(
    origin: &crate::brep::Vector3,
    normal: &crate::brep::Vector3,
    x_axis: &crate::brep::Vector3,
    focal_length: f64,
    parameter: f64,
) -> Option<CurveEvaluation> {
    if !parameter.is_finite() || !focal_length.is_finite() || focal_length <= 1e-12 {
        return None;
    }
    let (basis_x, basis_y, _normal) =
        orthonormal_frame_from_axis_and_ref(normal.to_array(), x_axis.to_array());
    let point = vec3_add(
        origin.to_array(),
        vec3_add(
            vec3_scale(basis_x, (parameter * parameter) / (4.0 * focal_length)),
            vec3_scale(basis_y, parameter),
        ),
    );
    let tangent = vec3_add(
        vec3_scale(basis_x, parameter / (2.0 * focal_length)),
        basis_y,
    );
    (point.into_iter().all(f64::is_finite) && tangent.into_iter().all(f64::is_finite))
        .then_some(CurveEvaluation { point, tangent })
}

/// Evaluates a point on an analytical or mapped 3D curve at parameter `t`.
///
/// Mapped NURBS use the bounded mapped-curve evaluator. This returns no derivative;
/// analytical curves delegate to [`evaluate_curve`].
#[must_use]
pub fn evaluate_curve_point(curve: &crate::brep::CurveKind, t: f64) -> Option<[f64; 3]> {
    if !t.is_finite() {
        return None;
    }
    match curve {
        crate::brep::CurveKind::Nurbs(nurbs) => {
            crate::brep::evaluate_mapped_nurbs_curve_candidate(nurbs, t).ok()
        }
        _ => evaluate_curve(curve, t).map(|evaluation| evaluation.point),
    }
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
fn vec3_scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
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

#[inline]
fn vec3_normalize(a: [f64; 3]) -> [f64; 3] {
    let len = vec3_norm(a);
    if len > 1e-12 {
        vec3_scale(a, 1.0 / len)
    } else {
        a
    }
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::{
        evaluate_curve, evaluate_surface, orthonormal_frame_from_axis_and_ref, vec3_add,
        vec3_cross, vec3_dot, vec3_normalize, vec3_scale, vec3_sub,
    };
    use crate::brep::{
        BrepMetrics, BrepModel, BrepSourceFormat, CurveKind, NurbsSurface, Sense, SourceNodeRef,
        SurfaceGeometry, SurfaceKind, TopologyValidation, Vector3,
    };

    #[test]
    fn evaluates_analytic_plane_and_circle() {
        let plane = SurfaceKind::Plane {
            point: Vector3::from_array([1.0, 2.0, 3.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let evaluated = evaluate_surface(&plane, 4.0, 5.0).unwrap();
        assert_eq!(evaluated.point, [5.0, 7.0, 3.0]);
        assert_eq!(evaluated.normal, [0.0, 0.0, 1.0]);

        let circle = CurveKind::Circle {
            center: Vector3::from_array([1.0, 2.0, 3.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 2.0,
        };
        let evaluated = evaluate_curve(&circle, 0.0).unwrap();
        assert_eq!(evaluated.point, [3.0, 2.0, 3.0]);
        assert_eq!(evaluated.tangent, [0.0, 2.0, 0.0]);
    }

    #[test]
    fn evaluates_tiny_domain_nurbs_plane_normal_at_bounds() {
        let domain = 1e-8;
        let extent = 1e-15;
        let surface = SurfaceKind::Nurbs(NurbsSurface {
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
                vec![0.0, 0.0, extent],
                vec![extent, 0.0, 0.0],
                vec![extent, 0.0, extent],
            ],
            u_knots: vec![0.0, domain],
            v_knots: vec![0.0, domain],
            u_knot_multiplicities: vec![2, 2],
            v_knot_multiplicities: vec![2, 2],
            sources: vec![],
        });

        for (u, v) in [(0.0, 0.0), (domain * 0.5, domain * 0.5), (domain, domain)] {
            let evaluated = evaluate_surface(&surface, u, v).expect("evaluate NURBS plane");
            assert!(evaluated.normal[1] < -0.999_999_999_999);
            assert!(evaluated.normal[0].abs() < 1e-12);
            assert!(evaluated.normal[2].abs() < 1e-12);
        }
    }

    #[test]
    fn evaluates_xt_cone_chart_and_natural_normal() {
        let axial_cone = SurfaceKind::XtCone {
            point: Vector3::from_array([2.0, -3.0, 4.0]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 5.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        assert_eq!(
            evaluate_surface(&axial_cone, 0.0, 2.0).unwrap().point,
            [8.5, -3.0, 2.0]
        );

        let surface = SurfaceKind::XtCone {
            point: Vector3::from_array([2.0, -3.0, 4.0]),
            axis: Vector3::from_array([1.0, 2.0, 2.0]),
            radius: 5.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let (x, y, axis) = orthonormal_frame_from_axis_and_ref([1.0, 2.0, 2.0], [1.0, 0.0, 0.0]);
        let point = [2.0, -3.0, 4.0];
        let tan_half_angle: f64 = 0.75;

        for (u, v) in [(0.3, 2.0), (1.7, -1.0), (2.4, -8.0)] {
            let evaluated = evaluate_surface(&surface, u, v).unwrap();
            let radial = vec3_add(vec3_scale(x, u.cos()), vec3_scale(y, u.sin()));
            let section_radius = 5.0 + v * tan_half_angle;
            let expected_point = vec3_add(
                vec3_add(point, vec3_scale(axis, -v)),
                vec3_scale(radial, section_radius),
            );
            let du = vec3_scale(
                vec3_add(vec3_scale(x, -u.sin()), vec3_scale(y, u.cos())),
                section_radius,
            );
            let dv = vec3_add(vec3_scale(axis, -1.0), vec3_scale(radial, tan_half_angle));
            let expected_normal = vec3_normalize(vec3_cross(du, dv));
            for (actual, expected) in evaluated.point.into_iter().zip(expected_point) {
                assert!((actual - expected).abs() < 1e-12);
            }
            for (actual, expected) in evaluated.normal.into_iter().zip(expected_normal) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn xt_cone_recursive_offset_matches_signed_p82_displacement() {
        let source = SourceNodeRef {
            node_index: 0,
            node_type: 0,
            type_name: String::new(),
            node_id: None,
            byte_range: 0..0,
        };
        let cone = SurfaceKind::XtCone {
            point: Vector3::from_array([1.0, 2.0, -3.0]),
            axis: Vector3::from_array([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]),
            radius: 5.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([2.0 / 5.0_f64.sqrt(), -1.0 / 5.0_f64.sqrt(), 0.0]),
        };
        let offset = 1.7;
        let (u, v) = (0.73, -2.4);

        for basis_sense in [Sense::Positive, Sense::Negative] {
            let brep = BrepModel {
                source_format: BrepSourceFormat::Text,
                schema_key: "13006".into(),
                complete: true,
                assemblies: vec![],
                instances: vec![],
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
                surfaces: vec![SurfaceGeometry {
                    id: 1,
                    sense: basis_sense,
                    owner: None,
                    kind: cone.clone(),
                    source: source.clone(),
                }],
                attributes: vec![],
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
                diagnostics: vec![],
            };
            let base =
                crate::brep::evaluate_brep_surface(&brep, &cone, u, v).expect("evaluate XT basis");
            assert!(base.point.into_iter().all(f64::is_finite));
            assert!(base.normal.into_iter().all(f64::is_finite));

            let offset_surface = SurfaceKind::Offset {
                basis_surface: 1,
                offset,
            };
            let offset_eval = crate::brep::evaluate_brep_surface(&brep, &offset_surface, u, v)
                .expect("recursively evaluate XT offset");
            let basis_orientation = if basis_sense == Sense::Negative {
                -1.0
            } else {
                1.0
            };
            let expected_point = vec3_add(
                base.point,
                vec3_scale(base.normal, offset * basis_orientation),
            );
            for (actual, expected) in offset_eval.point.into_iter().zip(expected_point) {
                assert!((actual - expected).abs() < 1e-12);
            }
            for (actual, expected) in offset_eval.normal.into_iter().zip(base.normal) {
                assert!((actual - expected).abs() < 1e-12);
            }
            let displacement = vec3_sub(offset_eval.point, base.point);
            assert!(
                (vec3_dot(displacement, base.normal) - offset * basis_orientation).abs() < 1e-12
            );
        }
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "The cone fixture's chart values yield exact expected point and normal components, and the unwrap asserts a supported chart evaluation."
    )]
    fn rejects_xt_cone_apex_and_preserves_generic_cone_chart() {
        let xt_cone = SurfaceKind::XtCone {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 4.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        assert!(evaluate_surface(&xt_cone, 0.0, -4.0 / 0.75).is_none());

        let cone = SurfaceKind::Cone {
            point: Vector3::from_array([0.0, 0.0, 0.0]),
            axis: Vector3::from_array([0.0, 0.0, 1.0]),
            radius: 4.0,
            sin_half_angle: 0.6,
            cos_half_angle: 0.8,
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
        };
        let evaluated = evaluate_surface(&cone, 0.0, 2.0).unwrap();
        assert_eq!(evaluated.point, [5.5, 0.0, 2.0]);
        assert_eq!(evaluated.normal, [0.8, 0.0, -0.6]);
    }

    #[test]
    fn evaluates_parabola_and_hyperbola_curves() {
        let parabola = CurveKind::Parabola {
            origin: Vector3::from_array([1.0, 2.0, 3.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            focal_length: 2.0,
        };
        let eval_p = evaluate_curve(&parabola, 4.0).expect("evaluate parabola");
        // P(t) = origin + (t^2 / 4f) * X + t * Y = [1, 2, 3] + (16/8)*[1, 0, 0] + 4*[0, 1, 0] = [3, 6, 3]
        // Tangent = (t / 2f) * X + Y = (4/4)*[1, 0, 0] + [0, 1, 0] = [1, 1, 0]
        assert_eq!(eval_p.point, [3.0, 6.0, 3.0]);
        assert_eq!(eval_p.tangent, [1.0, 1.0, 0.0]);

        let hyperbola = CurveKind::Hyperbola {
            origin: Vector3::from_array([0.0, 0.0, 0.0]),
            normal: Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: Vector3::from_array([1.0, 0.0, 0.0]),
            transverse_radius: 3.0,
            conjugate_radius: 4.0,
        };
        let eval_h = evaluate_curve(&hyperbola, 0.0).expect("evaluate hyperbola at t=0");
        // cosh(0) = 1, sinh(0) = 0 => point = [3, 0, 0], tangent = [0, 4, 0]
        assert_eq!(eval_h.point, [3.0, 0.0, 0.0]);
        assert_eq!(eval_h.tangent, [0.0, 4.0, 0.0]);
    }
}
