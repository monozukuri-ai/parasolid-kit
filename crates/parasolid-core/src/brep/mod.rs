//! Parasolid-native B-Rep source model and raw-document mapping.

mod assembly;
mod attributes;
mod blend;
mod body_hierarchy;
mod body_hierarchy_mapping;
mod evaluate;
mod intersection_candidate;
mod intersection_eval;
mod intersection_generic;
mod intersection_generic_dispatch;
mod mapper;
mod mesh_data;
mod metrics;
mod model;
mod nurbs_curve;
mod nurbs_surface;
mod polygon;
mod polyline_data;
mod profile_roles;
mod sp_curve;
mod swept;
mod tessellation;

pub use assembly::{
    AssemblyNode, AssemblyStructure, InstanceNode, TransformNode, TransformPrecision,
    map_xb_assembly, map_xt_assembly,
};
pub use attributes::{
    AttributeFieldData, AttributeFieldValue, AttributeValueNode, BrepAttribute, BrepAttributes,
    map_xb_brep_attributes, map_xt_brep_attributes,
};
pub use blend::{
    RollingBallSection, evaluate_rolling_ball, evaluate_rolling_ball_with_spine, evaluate_spine,
    evaluate_spine_in_brep, evaluate_spine_with_brep, rolling_ball_section,
    rolling_ball_section_variable,
};
pub use body_hierarchy::{
    BodyChildChainTermination, BodyHierarchyData, BodyHierarchyFieldNames,
    decode_named_body_children, decode_named_body_hierarchy,
};
pub use body_hierarchy_mapping::{
    BrepModelWithBodyHierarchy, BrepModelWithResolvedBodyHierarchy, MappedBodyChildren,
    MappedBodyHierarchy, map_xb_brep_with_body_hierarchy, map_xb_brep_with_resolved_body_hierarchy,
    map_xt_brep_with_body_hierarchy, map_xt_brep_with_resolved_body_hierarchy,
};
pub use evaluate::{
    CurveEvaluation, SurfaceEvaluation, evaluate_curve, evaluate_curve_point, evaluate_surface,
    evaluate_surface_point,
};
pub use intersection_candidate::{
    ChartIntersectionCandidate, ChartIntersectionError,
    intersect_torus_ruled_sweep_with_chart_plane_candidate,
};
pub use intersection_eval::{
    IntersectionCurveEvaluation, IntersectionEvaluationError,
    evaluate_cone_cylinder_intersection_candidate,
    evaluate_cone_cylinder_intersection_candidate_with_reversed_mapped_axis,
    evaluate_cylinder_cylinder_intersection_candidate,
    evaluate_cylinder_torus_intersection_candidate, evaluate_intersection_candidate,
    evaluate_plane_cylinder_intersection_candidate, evaluate_plane_plane_intersection_candidate,
    evaluate_plane_xt_cone_intersection_candidate,
    evaluate_plane_xt_cone_intersection_candidate_with_reversed_mapped_axis,
    evaluate_torus_ruled_sweep_intersection_candidate,
};
pub use mapper::{
    map_xb_brep, map_xb_brep_with_diagnostic_limit, map_xb_brep_with_metadata_recovery,
    map_xb_brep_with_metadata_recovery_and_diagnostic_limit, map_xt_brep,
    map_xt_brep_with_diagnostic_limit, map_xt_brep_with_metadata_recovery,
    map_xt_brep_with_metadata_recovery_and_diagnostic_limit, map_xt_intersection_data,
    map_xt_swept_data,
};
pub use mesh_data::{
    MeshDataError, MeshIndexOrigin, RawIntegerPool, RawMeshData, RawVectorPool,
    decode_embedded_mesh, decode_mesh_linked_transform, decode_mesh_triangles,
};
pub use model::{
    Assembly, AssemblyKind, Attribute, AttributeField, AttributeOwner, BlendType, Body, BodyKind,
    BoundingBox, BrepDiagnostic, BrepId, BrepMetrics, BrepModel, BrepSourceFormat, CurveGeometry,
    CurveKind, Edge, Face, HalfEdge, Instance, InstancePart, IntersectionChartParameters,
    IntersectionData, IntersectionLimitKind, IntersectionUvCoordinates, IntersectionUvType, Loop,
    NurbsCurve, NurbsSurface, PointGeometry, Region, RegionKind, Sense, Shell, SourceNodeRef,
    SurfaceGeometry, SurfaceKind, SweptSurfaceData, TopologyValidation, Transform, Vector3, Vertex,
};
pub use nurbs_curve::{
    evaluate_mapped_nurbs_curve_candidate, evaluate_mapped_nurbs_curve_candidate_2d,
    evaluate_mapped_nurbs_curve_candidate_with_linear_endpoint_extension,
    evaluate_nurbs_curve_values_candidate,
};
pub use nurbs_surface::evaluate_mapped_nurbs_surface;
pub use polygon::triangulate_polygon_2d;
pub use polyline_data::{PolylineData, decode_named_polyline};
pub use sp_curve::evaluate_sp_curve_candidate;
pub use swept::{evaluate_mapped_xt_swept_point_candidate, evaluate_ruled_sweep_point_candidate};
pub use tessellation::{
    TessellatedMesh, compute_newell_normal, evaluate_mapped_xt_swept_offset_surface_candidate,
    evaluate_surface as evaluate_brep_surface,
    evaluate_surface_point as evaluate_brep_surface_point, split_polygon_at_periodic_seam,
    tessellate_brep_body_to_mesh, tessellate_brep_face, tessellate_brep_face_with_explicit_uv,
    tessellate_brep_face_with_intersection_candidates, tessellate_brep_to_mesh,
    tessellate_generic_polygon, tessellate_mapped_xt_swept_face_with_explicit_uv,
    tessellate_mapped_xt_swept_face_with_sampled_chord_error,
    tessellate_resolved_brep_body_to_mesh, tessellate_swept_brep_face_with_explicit_uv,
};
