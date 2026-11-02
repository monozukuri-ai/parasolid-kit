//! Parasolid-native B-Rep source model and raw-document mapping.

mod mapper;
mod metrics;
mod model;
mod profile_roles;

pub use mapper::{
    map_xb_brep, map_xb_brep_with_diagnostic_limit, map_xt_brep, map_xt_brep_with_diagnostic_limit,
};
pub use model::{
    Assembly, AssemblyKind, Body, BodyKind, BoundingBox, BrepDiagnostic, BrepId, BrepMetrics,
    BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, Edge, Face, HalfEdge, Instance,
    InstancePart, Loop, NurbsCurve, NurbsSurface, PointGeometry, Region, RegionKind, Sense, Shell,
    SourceNodeRef, SurfaceGeometry, SurfaceKind, TopologyValidation, Transform, Vector3, Vertex,
};
