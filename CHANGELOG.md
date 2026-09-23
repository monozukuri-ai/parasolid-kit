# Changelog

## Unreleased

- Extend the exact iCAD V34 profile to revision 2 with the reviewed 13006
  TORUS (54) definition and matching B-Rep roles. The profile ID is
  `icad-sch34101-13006-r2`; other schema keys and unknown base types retain
  their existing boundaries. This core patch supports catalog-free parsing
  of qualified toroidal saved solids in downstream iCAD readers.

- Add the exact iCAD `SCH_3401212_34101_13006` built-in profile for extracted
  Parasolid streams, including pinned source B-Rep roles. Unknown base types and
  nearby keys still require their own reviewed support. Native ICD extraction,
  part ownership, placement and unit interpretation remain caller responsibilities.

## 0.2.0 — unreleased

- Add the exact Onshape `SCH_3701212_37102_13006` profile, including analytic
  geometry, direct NURBS curves/surfaces, intersection and trimmed curves,
  surface-parametric curves, and fully declared multi-part transmit blocks.
  Other modeller keys still require their own reviewed profile or an explicit
  schema provider.
- Preserve intersection chart and limit positions in the source B-Rep.
- Add the bundled local viewer and bounded UV/intersection conversion.
  Parser, OCCT conversion, STEP export and preview retain separate support limits.
- Extend release validation with native NURBS coefficients, sampled parametric
  geometry, producer tolerance reporting and hashed native evidence.

### Migration from 0.1.0

- The V30 profile is `onshape-sch30000-r3`; the new current Onshape profile is
  `onshape-sch37102-13006-r3`. Consumers that pin profile IDs or hashes must
  update their expectations for these exact keys.
- Intersection definitions include `chart_points`, `start_points` and
  `end_points`. Strict JSON consumers must accept these additional fields.
- Core area and volume require complete planar polygonal boundaries. Curved
  boundaries return `None`, including cases previously assigned incorrect
  endpoint-polygon metrics. Bounding boxes still use source topological vertices.
- Reading a NURBS definition does not establish OCCT/STEP/preview conversion for
  every rational, closed or periodic variant. Assemblies, native CAD containers
  and SolidWorks delta application remain outside the complete parser contract.

See [format support](docs/format-support.md) for exact keys and stage boundaries.
