//! Explicitly opted-in ruled-sweep point evaluation.

use std::io;

use super::{
    CurveGeometry, CurveKind, NurbsCurve, SweptSurfaceData,
    evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension,
};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Evaluates a ruled sweep from a section curve and an already-resolved direction.
///
/// The caller explicitly supplies the direction because source formats may store
/// additional sweep fields whose relationship to this vector is not established.
/// The section curve uses the opt-in linear endpoint-tangent extension evaluator.
/// This helper is not used by the generic surface evaluator or face tessellator.
///
/// # Errors
/// Returns invalid data for nonfinite parameters or directions, a zero direction,
/// failed section evaluation, or a nonfinite output point.
pub fn evaluate_ruled_sweep_point_candidate(
    section: &NurbsCurve,
    direction: [f64; 3],
    u: f64,
    v: f64,
) -> io::Result<[f64; 3]> {
    if !u.is_finite() || !v.is_finite() {
        return Err(invalid("nonfinite ruled-sweep parameter"));
    }
    if direction.iter().any(|component| !component.is_finite()) {
        return Err(invalid("nonfinite ruled-sweep direction"));
    }
    if direction.iter().all(|component| *component == 0.0) {
        return Err(invalid("zero ruled-sweep direction"));
    }

    let section_point =
        evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(section, u)?;
    let mut point = [0.0; 3];
    for axis in 0..3 {
        point[axis] = section_point[axis] + v * direction[axis];
        if !point[axis].is_finite() {
            return Err(invalid("nonfinite ruled-sweep point"));
        }
    }
    Ok(point)
}

/// Evaluates one mapped `X_T` swept point from its mapped section curve.
///
/// This follows the documented `C(u) + vD` relation. The raw `X_T` scale is
/// retained in `swept` and is intentionally not used as a distance factor.
/// `D` must be unit length within 64 binary64 epsilons; it is not normalized.
/// Section continuation is limited to the existing validated open, clamped,
/// nonperiodic, nonrational NURBS profile. The caller must provide data and
/// mapped section provenance from the same parsed document; same-document
/// ownership is a caller responsibility because transmit indexes are local.
/// This candidate does not establish trim membership or face acceptance.
///
/// # Errors
/// Returns invalid data for missing or mismatched section data, invalid
/// parameters or direction, unsupported section curves, or failed evaluation.
pub fn evaluate_mapped_xt_swept_point_candidate(
    swept: &SweptSurfaceData,
    section: Option<&CurveGeometry>,
    u: f64,
    v: f64,
) -> io::Result<[f64; 3]> {
    let section_index = swept
        .section
        .ok_or_else(|| invalid("X_T swept section pointer is null"))?;
    let section = section.ok_or_else(|| invalid("mapped X_T swept section is missing"))?;
    if section.source.node_index != section_index {
        return Err(invalid(
            "mapped curve does not match X_T swept section pointer",
        ));
    }
    let direction = [
        swept.sweep[0].ok_or_else(|| invalid("X_T swept direction is incomplete"))?,
        swept.sweep[1].ok_or_else(|| invalid("X_T swept direction is incomplete"))?,
        swept.sweep[2].ok_or_else(|| invalid("X_T swept direction is incomplete"))?,
    ];
    if !u.is_finite() || !v.is_finite() {
        return Err(invalid("nonfinite X_T swept parameter"));
    }
    if direction.iter().any(|component| !component.is_finite()) {
        return Err(invalid("nonfinite X_T swept direction"));
    }
    let direction_length = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    if !direction_length.is_finite() || (direction_length - 1.0).abs() > 64.0 * f64::EPSILON {
        return Err(invalid("X_T swept direction is not unit length"));
    }
    let section_point = match &section.kind {
        CurveKind::Nurbs(curve) => {
            evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension(curve, u)?
        }
        CurveKind::Line { .. } | CurveKind::Circle { .. } | CurveKind::Ellipse { .. } => {
            let pt = crate::brep::evaluate_curve_point(&section.kind, u)
                .ok_or_else(|| invalid("failed to evaluate analytic section curve point"))?;
            if pt.iter().any(|coord| !coord.is_finite()) {
                return Err(invalid("nonfinite analytic section curve point"));
            }
            pt
        }
        _ => return Err(invalid("X_T swept section curve kind is unsupported")),
    };
    let mut point = [0.0; 3];
    for axis in 0..3 {
        point[axis] = section_point[axis] + v * direction[axis];
        if !point[axis].is_finite() {
            return Err(invalid("nonfinite X_T swept point"));
        }
    }
    Ok(point)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::float_cmp,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::{evaluate_mapped_xt_swept_point_candidate, evaluate_ruled_sweep_point_candidate};
    use crate::brep::{CurveGeometry, CurveKind, NurbsCurve, SourceNodeRef, SweptSurfaceData};

    fn mapped_section() -> CurveGeometry {
        CurveGeometry {
            id: 1,
            sense: crate::brep::Sense::Positive,
            owner: None,
            kind: CurveKind::Nurbs(section_line()),
            source: SourceNodeRef {
                node_index: 7,
                node_type: 42,
                type_name: "B_CURVE".to_owned(),
                node_id: Some(42),
                byte_range: 0..1,
            },
        }
    }

    fn swept() -> SweptSurfaceData {
        SweptSurfaceData {
            source: SourceNodeRef {
                node_index: 9,
                node_type: 67,
                type_name: "SWEPT_SURF".to_owned(),
                node_id: Some(43),
                byte_range: 1..2,
            },
            section: Some(7),
            sweep: [Some(0.0), Some(0.8), Some(0.6)],
            scale: None,
        }
    }

    fn section_line() -> NurbsCurve {
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

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the integer-valued endpoint fixture produces exact expected coordinates for its affine extension"
    )]
    fn ruled_sweep_uses_linear_section_extensions_at_both_endpoints() -> std::io::Result<()> {
        let direction = [0.0, 2.0, 3.0];
        assert_eq!(
            evaluate_ruled_sweep_point_candidate(&section_line(), direction, 0.0, 2.0)?,
            [-4.0, 4.0, 3.0]
        );
        assert_eq!(
            evaluate_ruled_sweep_point_candidate(&section_line(), direction, 8.0, 2.0)?,
            [12.0, 12.0, 11.0]
        );
        Ok(())
    }

    #[test]
    fn ruled_sweep_rejects_invalid_inputs_and_nonfinite_results() {
        let section = section_line();
        assert!(evaluate_ruled_sweep_point_candidate(&section, [0.0; 3], 4.0, 1.0).is_err());
        assert!(
            evaluate_ruled_sweep_point_candidate(&section, [0.0, f64::INFINITY, 1.0], 4.0, 1.0)
                .is_err()
        );
        assert!(
            evaluate_ruled_sweep_point_candidate(&section, [1.0, 0.0, 0.0], 4.0, f64::NAN).is_err()
        );
        assert!(
            evaluate_ruled_sweep_point_candidate(&section, [f64::MAX, 0.0, 0.0], 4.0, 2.0).is_err()
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the fixed mapped fixture pins the evaluator's exact output coordinates"
    )]
    fn mapped_xt_swept_evaluator_uses_linked_section_and_ignores_nullable_scale()
    -> std::io::Result<()> {
        assert_eq!(
            evaluate_mapped_xt_swept_point_candidate(&swept(), Some(&mapped_section()), 4.0, 2.0)?,
            [4.0, 5.6, 2.2]
        );
        Ok(())
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the endpoint fixture produces exact integer coordinates for the supported linear continuation"
    )]
    fn mapped_xt_swept_evaluator_continues_only_supported_sections_outside_domain()
    -> std::io::Result<()> {
        let section = mapped_section();
        assert_eq!(
            evaluate_mapped_xt_swept_point_candidate(&swept(), Some(&section), 0.0, 0.0)?,
            [-4.0, 0.0, -3.0]
        );
        let mut unsupported = section.clone();
        if let CurveKind::Nurbs(curve) = &mut unsupported.kind {
            curve.rational = true;
        }
        assert!(
            evaluate_mapped_xt_swept_point_candidate(&swept(), Some(&unsupported), 0.0, 0.0)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn mapped_xt_swept_evaluator_rejects_null_malformed_and_unrelated_data() {
        let section = mapped_section();
        assert!(evaluate_mapped_xt_swept_point_candidate(&swept(), None, 4.0, 1.0).is_err());
        let mut data = swept();
        data.section = None;
        assert!(evaluate_mapped_xt_swept_point_candidate(&data, Some(&section), 4.0, 1.0).is_err());
        let mut data = swept();
        data.sweep[1] = None;
        assert!(evaluate_mapped_xt_swept_point_candidate(&data, Some(&section), 4.0, 1.0).is_err());
        let mut data = swept();
        data.sweep[1] = Some(2.0);
        assert!(evaluate_mapped_xt_swept_point_candidate(&data, Some(&section), 4.0, 1.0).is_err());
        let mut data = swept();
        data.section = Some(8);
        assert!(evaluate_mapped_xt_swept_point_candidate(&data, Some(&section), 4.0, 1.0).is_err());
        assert!(
            evaluate_mapped_xt_swept_point_candidate(&swept(), Some(&section), f64::NAN, 1.0)
                .is_err()
        );
        let mut unsupported = section.clone();
        unsupported.kind = CurveKind::Parabola {
            origin: crate::brep::Vector3::from_array([0.0; 3]),
            normal: crate::brep::Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: crate::brep::Vector3::from_array([1.0, 0.0, 0.0]),
            focal_length: 1.0,
        };
        assert!(
            evaluate_mapped_xt_swept_point_candidate(&swept(), Some(&unsupported), 4.0, 1.0)
                .is_err()
        );
    }

    #[test]
    fn mapped_xt_swept_evaluator_supports_analytic_line_circle_and_ellipse() {
        let swept_data = swept();
        let mut line_section = mapped_section();
        line_section.kind = CurveKind::Line {
            point: crate::brep::Vector3::from_array([1.0, 2.0, 3.0]),
            direction: crate::brep::Vector3::from_array([1.0, 0.0, 0.0]),
        };
        // C(u) = [1.0 + 2.0, 2.0, 3.0] = [3.0, 2.0, 3.0]
        // v = 5.0, D = [0.0, 0.8, 0.6] -> vD = [0.0, 4.0, 3.0]
        // R(u, v) = [3.0, 6.0, 6.0]
        let pt_line =
            evaluate_mapped_xt_swept_point_candidate(&swept_data, Some(&line_section), 2.0, 5.0)
                .expect("evaluate line swept point");
        assert_eq!(pt_line, [3.0, 6.0, 6.0]);

        let mut circle_section = mapped_section();
        circle_section.kind = CurveKind::Circle {
            center: crate::brep::Vector3::from_array([0.0, 0.0, 0.0]),
            normal: crate::brep::Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: crate::brep::Vector3::from_array([1.0, 0.0, 0.0]),
            radius: 5.0,
        };
        // u = 0.0 -> C(0) = [5.0, 0.0, 0.0]
        // v = 10.0, D = [0.0, 0.8, 0.6] -> vD = [0.0, 8.0, 6.0]
        // R(0, 10.0) = [5.0, 8.0, 6.0]
        let pt_circle =
            evaluate_mapped_xt_swept_point_candidate(&swept_data, Some(&circle_section), 0.0, 10.0)
                .expect("evaluate circle swept point");
        assert_eq!(pt_circle, [5.0, 8.0, 6.0]);

        // u = PI / 2 -> C(PI/2) = [0.0, 5.0, 0.0]
        // R(PI/2, 10.0) = [0.0, 13.0, 6.0]
        let pt_circle_half_pi = evaluate_mapped_xt_swept_point_candidate(
            &swept_data,
            Some(&circle_section),
            std::f64::consts::FRAC_PI_2,
            10.0,
        )
        .expect("evaluate circle swept point at pi/2");
        assert!((pt_circle_half_pi[0]).abs() < 1e-12);
        assert!((pt_circle_half_pi[1] - 13.0).abs() < 1e-12);
        assert!((pt_circle_half_pi[2] - 6.0).abs() < 1e-12);

        let mut ellipse_section = mapped_section();
        ellipse_section.kind = CurveKind::Ellipse {
            center: crate::brep::Vector3::from_array([0.0, 0.0, 0.0]),
            normal: crate::brep::Vector3::from_array([0.0, 0.0, 1.0]),
            x_axis: crate::brep::Vector3::from_array([1.0, 0.0, 0.0]),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
        // u = 0.0 -> C(0) = [4.0, 0.0, 0.0]
        // v = 5.0, D = [0.0, 0.8, 0.6] -> vD = [0.0, 4.0, 3.0]
        // R(0, 5.0) = [4.0, 4.0, 3.0]
        let pt_ellipse =
            evaluate_mapped_xt_swept_point_candidate(&swept_data, Some(&ellipse_section), 0.0, 5.0)
                .expect("evaluate ellipse swept point");
        assert_eq!(pt_ellipse, [4.0, 4.0, 3.0]);
    }
}
