# Format support and limitations

`parasolid-kit` is a read-focused Parasolid transmit-file parser with the bounded
support described below.
This page describes the supported public behavior; it is not a claim of
compatibility with every Parasolid version, producer, or geometry type.

## File operations

| Operation | X_B | X_T |
|---|---:|---:|
| Inspect the common and internal header | Supported | Supported |
| Parse a complete node stream | Built-in subset or exact external schema | Built-in subset or exact external schema |
| Preserve source bytes and byte ranges | Supported | Supported |
| Reconstruct an unmodified document | Byte-exact | Not supported |
| Compare decoded documents | Supported | Supported |
| Map to the typed B-Rep source model | Supported subset | Supported subset |
| Compact `read_brep`/`check` summary | Supported | Supported |
| Optional OCCT conversion | Documented exact and bounded parametric subset | Documented exact and bounded parametric subset |
| Optional AP242 export with cold reimport | Documented I7/OCCT subset | Documented I7/OCCT subset |
| Optional CadQuery shape adapter | Documented I7/OCCT subset | Documented I7/OCCT subset |
| Optional GLB/local viewer with source picking | Documented I7/OCCT subset | Documented I7/OCCT subset |

Inputs can be filesystem paths or `bytes`, `bytearray`, and `memoryview`
values. File-like objects are not accepted. Inspection validates the header
only; it does not prove that the remaining node stream or geometry is valid.

## Schema selection and built-in scope

### Supported profiles

This is the shared support matrix for the Python, Rust, and CLI entry points.
Default parsing selects one compiled profile by the complete internal stream
key. Every exact profile has `verified_subset` coverage; the `USFLD_SIZE` user
words that follow every node are read into `RawNode.user_fields`. A key that
no exact profile accepts is next matched by its
[schema family](#schema-families), the key without its modeller build; a
document read that way reports `family_inferred` coverage. An exact profile is
normally completed by its family: a type it does not declare itself is read
with the declaration of another reviewed profile of the same family or base,
while the provenance still names the exact profile. The five TopSolid v6 keys
below instead use bundled base and effective catalogs selected by exact key;
they do not establish support for other TopSolid keys or native packages.
The type and field counts describe compiled base definitions; embedded input
definitions can change the effective layout. A key without a base suffix
transmits no schema, so its counts describe the complete layouts. Profile hashes identify the
reviewed definitions, not the bytes or geometric correctness of an input.

| Exact internal key | Profile / revision | Base types / field groups | Canonical SHA-256 |
|---|---|---|---|
| `SCH_2701192_26105_13006` | `topsolid-v6-sch-2701192-26105-13006-r1` / 1 | 140 / 1005 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| `SCH_3300259_32001_13006` | `topsolid-v6-sch-3300259-32001-13006-r1` / 1 | 140 / 1005 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| `SCH_3300261_32001_13006` | `topsolid-v6-sch-3300261-32001-13006-r1` / 1 | 140 / 1005 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| `SCH_3700192_36001_13006` | `topsolid-v6-sch-3700192-36001-13006-r1` / 1 | 140 / 1005 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| `SCH_3600188_36001_13006` | `topsolid-v6-sch-3600188-36001-13006-r1` / 1 | 140 / 1005 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| `SCH_3000000_30000` | `onshape-sch30000-r3` / 3 | 44 / 356 | `67e0f3f90c9025c16269c0b03d2365834d949797e1f4c0eb4255b153960b7bf4` |
| `SCH_1300000_13006` | `onshape-sch13006-r6` / 6 | 40 / 327 | `2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb` |
| `SCH_3000310_30000_13006` | `icad-sch30000-13006-r6` / 6 | 43 / 366 | `29d58f5c4aba74913689b9034b0532a17601bb660fafc01d21692ef5330f4f8e` |
| `SCH_3401212_34101_13006` | `icad-sch34101-13006-r3` / 3 | 40 / 320 | `595a8848c2b7730367bb41bbe75332c79c9038eb56accc7ee785ca9742a37de7` |
| `SCH_3701212_37102_13006` | `onshape-sch37102-13006-r3` / 3 | 41 / 339 | `3f9489b24874ca7857e48d8daf106bcf821f612b49e0d60650b20e132e04abaa` |
| `SCH_3701229_37102_13006` | `solidworks-sch37102-13006-r3` / 3 | 56 / 460 | `7300096c034365663a13f40c96a919d19b73d25b4c957f9dbb0e472c0a381730` |
| `SCH_2700199_26105_13006` | `solidworks-2700199-26105-13006-r2` / 2 | 56 / 460 | `edab51092b3b8d1e0f137e6e3d7f666348a433b2bf17a932e8b708886e6b4331` |
| `SCH_2800191_28002_13006` | `solidworks-2800191-28002-13006-r2` / 2 | 56 / 460 | `388cf98e9768b041ff7dd572df1e63f83c41afc36eaa635ccf61fdc272214a3e` |
| `SCH_2800209_28002_13006` | `solidworks-2800209-28002-13006-r2` / 2 | 56 / 460 | `162b6cdd51c8b6ac6bcbddf363c2b9cba98262d4e08365d31576b30a3653fe3e` |
| `SCH_2900166_28101_13006` | `solidworks-2900166-28101-13006-r2` / 2 | 56 / 460 | `f34b810890bf48d501fd941a5e2793741df03915b93292ce3abb36d6b16a6bff` |
| `SCH_2900190_28101_13006` | `solidworks-2900190-28101-13006-r2` / 2 | 56 / 460 | `6625faaa9d5afae86fa6f9058a6ae36a2bc97852f6d66dd8d68b3c899a868f7f` |
| `SCH_2900195_28101_13006` | `solidworks-2900195-28101-13006-r2` / 2 | 56 / 460 | `3ed9e73e24121d843d2393b42462c5ae633c8054ec8f66b54ebddc2ea4c8a28b` |
| `SCH_3000226_30000_13006` | `solidworks-3000226-30000-13006-r2` / 2 | 56 / 460 | `734a40413d7a1a97a2344e5d6379255b299365ea50361ff0286c7f90fcd956cc` |
| `SCH_3000298_30000_13006` | `solidworks-3000298-30000-13006-r2` / 2 | 56 / 460 | `c9d5c679dba4855d478de10e6c0cd88167113bbdc0a9553b259bab261e490cfd` |
| `SCH_3100203_31001_13006` | `solidworks-3100203-31001-13006-r2` / 2 | 56 / 460 | `b209ba14673b1fb93d43bb4d74aab652e63fba9d9bbeb88e4c1ee5608c1b9bb1` |
| `SCH_3101290_31100_13006` | `solidworks-3101290-31100-13006-r2` / 2 | 56 / 460 | `bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b` |
| `SCH_3401254_34101_13006` | `solidworks-3401254-34101-13006-r2` / 2 | 56 / 460 | `c702f7a475e4e727791463e24fa7673d289f77aab0768e01bd61b25ce960b059` |
| `SCH_3501210_35102_13006` | `solidworks-3501210-35102-13006-r2` / 2 | 56 / 460 | `9673a3534ce3df39916de4d9016403861526d0cc66a3887ae4dcf41b2a12d689` |
| `SCH_3501256_35102_13006` | `solidworks-3501256-35102-13006-r2` / 2 | 56 / 460 | `2b66c69af0f79c8a2f7558696b125a994a86026e452d8fc2a7d6577b4fb5d9d9` |
| `SCH_3601228_36001_13006` | `solidworks-3601228-36001-13006-r2` / 2 | 56 / 460 | `71d7a26b5ee2ce9d857449556f91757cc943e8ea8aedceb82c150f706585dc7c` |
| `SCH_1500137_15003_13006` | `icad-1500137-15003-13006-r1` / 1 | 31 / 252 | `dcb4bb70654e9d1bf091b1310b2068872b56304f0b820435a3910bcec366a9e8` |
| `SCH_1500245_15003_13006` | `icad-1500245-15003-13006-r1` / 1 | 34 / 292 | `4179d5785e61e89926dfb54e0bc5eed7f0fb7cdec7300de0936ef678ba4d0f4c` |
| `SCH_1700223_16100_13006` | `icad-1700223-16100-13006-r1` / 1 | 31 / 252 | `a27d8216055fe16425438c854482ec8dfac5e6f2e0aea1e1b7ce3605bc6ac633` |
| `SCH_1700256_16100_13006` | `icad-1700256-16100-13006-r1` / 1 | 39 / 305 | `30fc3f141639819bb6c76a4585056c7b274a6103d6c3924f71c448603796e802` |
| `SCH_1901261_19008_13006` | `icad-1901261-19008-13006-r1` / 1 | 31 / 252 | `f8913a60054dd7f29049a46c4b57ec6e7be048a2c112230453d31fe8587b6e22` |
| `SCH_1901315_19008_13006` | `icad-1901315-19008-13006-r2` / 2 | 34 / 288 | `e9d776fdc6cd29bdc9b3bbbc05e0b75c10e287d515a5ac7c6cad01547639ded8` |
| `SCH_2100293_20000_13006` | `icad-2100293-20000-13006-r2` / 2 | 44 / 375 | `52f7f986db1d2851aaf5c4a9938a5ade7a3c3daaa008cc8811247556a059f31f` |
| `SCH_2100311_20000_13006` | `icad-2100311-20000-13006-r1` / 1 | 43 / 363 | `09ac64c00702f713b5ef2eaccffe003ce6e073a1b2694dd4184178cbd9399dff` |
| `SCH_2401260_20000_13006` | `icad-2401260-20000-13006-r1` / 1 | 31 / 252 | `d89bae93d6a526d92c022933d332b88b9c7083561b0e7cecec79c7a94e24a149` |
| `SCH_2601246_26105_13006` | `icad-2601246-26105-13006-r1` / 1 | 43 / 366 | `bc85f903ff495ab14dcce7e0528e28ad0f23197d60493e68f2b525ea321c024a` |
| `SCH_2800188_28002_13006` | `icad-2800188-28002-13006-r1` / 1 | 31 / 252 | `83f469624fd9e0a636850444e728bd65b7599669e74fa6816972c9a0f19a0976` |
| `SCH_2901199_28101_13006` | `icad-2901199-28101-13006-r1` / 1 | 43 / 368 | `1d834f8db3223aa77e005f9ac394d29c0c4d4d018d74dc4098293ae783031627` |
| `SCH_3200152_32001_13006` | `icad-3200152-32001-13006-r1` / 1 | 39 / 305 | `b6d919827de77377bc8a96581f25c1654bf257e5d0f3c126ba1090850b623413` |
| `SCH_3200252_32001_13006` | `icad-3200252-32001-13006-r1` / 1 | 44 / 379 | `141ddd0f43c9a5c8bac32395171352899adca081f7b17fb77f9fd625e2495172` |
| `SCH_3301231_33103_13006` | `icad-3301231-33103-13006-r2` / 2 | 45 / 391 | `cc98a11221746512e9429ae85dd6c354b2c557fb7fe3959c7d13151220936baf` |
| `SCH_2400164_20000_13006` | `parasolid-2400164-20000-13006-r2` / 2 | 58 / 474 | `ef75cfbf0f21455156dde0b6d346e3be92dd114f9eb2500f4a2e7f7dd9de39ae` |
| `SCH_2600169_25001_13006` | `nx-2600169-25001-13006-r2` / 2 | 56 / 460 | `45a2e1d9ca4e48831c5e4c757978f0cde09e3d67c46d88392055f52ea031eea0` |
| `SCH_2600217_25001_13006` | `parasolid-2600217-25001-13006-r2` / 2 | 56 / 460 | `99becb31c455144e4bb98e25fa97232518754b4ea9b6115c3533f554ad8ce18b` |
| `SCH_3001232_30100_13006` | `nx-3001232-30100-13006-r2` / 2 | 56 / 460 | `a4d96295b72774062ffc6a7fb124664e1254f1df584748bbc5960897ec9b576b` |
| `SCH_3400176_34001_13006` | `nx-3400176-34001-13006-r2` / 2 | 56 / 460 | `254c0340fcef549d67badc03c25653f518e06d7657b892308a48f1f4b3aabf05` |
| `SCH_1300218_13006` | `icad-1300218-13006-r1` / 1 | 23 / 189 | `928962c3f7f5a87013f6af6e45a443d040ff9a967b0887f959366b9bd2774b82` |
| `SCH_1302234_13006` | `icad-1302234-13006-r1` / 1 | 19 / 165 | `29708f25ceea002ff57f2ebcada3a8bd25ffb6ef1f908ff678b45c3c7abc9f54` |
| `SCH_1500000_15003` | `icad-1500000-15003-r1` / 1 | 28 / 242 | `d3f7b3909050da2834e34c7f1ccf6b17f2f9cf16048053634dd7fc61506ad313` |
| `SCH_1700000_16100` | `icad-1700000-16100-r2` / 2 | 39 / 311 | `695d4a8c31c1a187dd07d475b4a28af97f800d68c1d1b99650322cef368e9f9c` |
| `SCH_1901000_19008` | `icad-1901000-19008-r1` / 1 | 20 / 181 | `c273c545b4b69f176457113d9bb1e158f170a39a4cd8babfbd687e9e0c325ba0` |
| `SCH_2401000_20000` | `icad-2401000-20000-r3` / 3 | 46 / 411 | `fbdd2b2c31177b9fba0104c4a7d9ead562c38202aff91f2759f30101557fa53f` |
| `SCH_2601000_26105` | `icad-2601000-26105-r1` / 1 | 38 / 336 | `f2717a3ccb97d6e836bb36e981671974b0d2fd0a586c9b42e3fcbe77fe36976d` |
| `SCH_2800000_28002` | `icad-2800000-28002-r1` / 1 | 41 / 370 | `a1693181a2af88fd8456155296f7ffefca2a71fdf14366a9866ab9c88e6531b7` |
| `SCH_2901000_28101` | `icad-2901000-28101-r2` / 2 | 26 / 216 | `aa6770061f55dde9d96afcff1b8ab260293e1a9671cda701b262b775f2192548` |
| `SCH_3200000_32001` | `icad-3200000-32001-r2` / 2 | 31 / 263 | `ab1d5312e7892bba12d53262617350ecc6da575f658a582180c3edf6bc84f603` |
| `SCH_3301000_33103` | `icad-3301000-33103-r1` / 1 | 25 / 219 | `29c38621bc5d937dddd0550bc52a4f3fcbc71616a5dd8774881b66aa5bcf030f` |
| `SCH_900141_9003` | `nx-900141-9003-r1` / 1 | 44 / 360 | `a76f2f8fb2ef43265887356651c011a09c2a39ef284b506a0427ed610c4e1dd4` |
| `SCH_901297_9008` | `microstation-901297-9008-r1` / 1 | 44 / 377 | `9069e19228b0ad87331d428b316379fd674afe56af939f3af956cefbfbbd697f` |
| `SCH_1000252_10004` | `nx-1000252-10004-r1` / 1 | 47 / 408 | `a744e21d11d31c699a658ccd117431250364255a88707f32a01694d84be45063` |
| `SCH_1100129_11004` | `ironcad-1100129-11004-r1` / 1 | 34 / 327 | `e3710ba1d738ab627448987fe60b81808d0f985c9368433fd0266064953f1adb` |
| `SCH_1200000_12006` | `solidworks-1200000-12006-r1` / 1 | 15 / 130 | `fc4d672349724760f98e267dfa2d901b9dcf4d1335c4c03a6509e40f359c0ecc` |
| `SCH_1600000_16004` | `nx-1600000-16004-r1` / 1 | 30 / 246 | `ac2c24ed9b802285a17fd6cbb20805944442363d831ba6aaf40578de1bed2590` |

The following cells summarize recorded local validation, not a new CAD capture
or a guarantee for every input with the same key. Real fixtures are outside
public CI. The detailed evidence and its retained failures are in
[profile provenance](builtin-profiles.md) and
[SolidWorks partitions](solidworks-partitions.md).

| Profile scope | Raw decoding | Source B-Rep | Independent geometry evidence | OCCT / STEP / preview | Native saved state |
|---|---|---|---|---|---|
| Onshape V30 | Paired X_T / neutral X_B for basic solids, spheres, elliptical edges, a frustum and a ring torus; revision-3 complex X_T has exact-catalog parity | Topology, analytic, NURBS and curve-wrapper definitions in the reviewed subset | Producer / STEP comparisons, including two new analytic models under revision 3; complex parametric geometry has no new independent producer campaign; earlier metric failures remain | Planar box/prism validation plus bounded UV/intersection conversion and preview of one local complex model; source-tolerance warnings remain | CAD container/configuration reconstruction is outside the parser |
| Onshape current (`SCH_3701212_37102_13006`) | Paired X_T / X_B for analytic solids, direct NURBS sheets and [compound shapes](onshape-composite.md) | Analytic/NURBS geometry, intersection/trimmed/SP_CURVE wrappers and multiple source bodies | Native coefficients and sampled geometry; auxiliary-surface evidence is bounded; strict STEP and integrated-metric failures remain recorded | No new general curved-solid or rational/periodic NURBS adapter conversion | CAD container/configuration reconstruction is outside the parser |
| Onshape V13 | Paired X_T / neutral X_B, including the documented NURBS / SP_CURVE campaigns | Topology, analytic and bounded NURBS / wrapper definitions | Producer / STEP and sampled surface evidence; recorded curve-tolerance failures remain | Bounded open nonrational UV and intersection conversion; rational/periodic NURBS remain unsupported | CAD container/configuration reconstruction is outside the parser |
| iCAD embedded V30 | 17 neutral X_B streams from one container; embedded X_T tests are synthetic | All 17 reach complete source B-Rep/topology | No independent CAD/STEP oracle; line-trim endpoint checks are internal consistency evidence | No real iCAD conversion validation; source B-Rep success does not establish conversion | `.icd` extraction and saved-state selection are caller responsibilities; not established by these streams |
| iCAD embedded V34 (unreleased) | Ten neutral X_B resources, including saved variants, match explicit-catalog values and byte boundaries | Ten complete source B-Reps match explicit-catalog mapping except type labels | Four authored solids match independent SDK dimensions and mass properties; sphere SDK edge enumeration failed | No new OCCT/STEP/preview qualification | `.icd` extraction, part ownership, placement and unit interpretation remain caller responsibilities |
| [iCAD legacy five exact keys](builtin-profiles.md#legacy-icad-embedded-keys) (unreleased) | 539 sampled neutral X_B resource occurrences match catalog values, ranges and terminators; public X_T/X_B fixtures are authored | 535 complete, four explicitly partial SPUN_SURF cases; all topology valid; catalog parity including source references | Authored wire coordinates, blend references/parameters and UV spline coefficients; no independent legacy CAD measurement campaign | No new conversion qualification on those resources; SPUN_SURF converts as a surface of revolution (unreleased) | Native ICD framing, ownership, transforms, extrusion and feature history remain downstream responsibilities |
| [iCAD later exact keys](builtin-profiles.md#later-icad-keys) (unreleased): eight embedded and five standard | 17,082 of 17,099 distinct sampled X_B resources match catalog values, ranges and terminators; 17 with OFFSET_SURF (60) fail closed; public X_T/X_B fixtures are authored | 17,047 complete, 15 explicitly partial SPUN_SURF cases and 20 null-parameter errors, each identical to the catalog path | Authored wire coordinates, per-ordinal record values and B-spline surface coefficients; no independent CAD measurement campaign in this repository | No new conversion qualification | Native ICD framing, ownership, transforms and feature history remain downstream responsibilities |
| [Further iCAD keys and revisions](builtin-profiles.md#further-icad-keys-and-revisions) (unreleased): two embedded and six standard keys, four revised profiles, OFFSET_SURF | 18,610 of 18,634 distinct sampled X_B resources of all iCAD keys match catalog values, ranges and terminators; schema 8008 and 12103 keys and two V30 standard resources still fail closed | 18,567 complete, 32 explicitly partial SPUN_SURF cases and 11 errors, each identical to the catalog path | Authored offset-surface and unset-trim-parameter records with independent values; no independent CAD measurement campaign in this repository | No new conversion qualification | Native ICD framing, ownership, transforms and feature history remain downstream responsibilities |
| [Schema families, transmit-file keys, user fields and assembly structure](builtin-profiles.md#schema-families-and-base-contributors) (unreleased): 36 derived families, five transmit profiles, exact profiles completed by their family, standard 16100/20000 revision 2, ASSEMBLY/INSTANCE/TRANSFORM, the attribute value nodes of the reference | 22 public X_T transmit files (SolidWorks 2009-2024, NX, Inventor 2012, the Parasolid 24 and 26 acceptance tests, the Siemens Parasolid 26/27 examples, ANSYS, MicroStation): all 22 parse, against 2 with the 0.3.8 release and 6 with exact-key families alone (unreleased: the ANSYS Electronics Desktop file's leading count is skipped and its node type 3 is declared from that one observation; the MicroStation file reads with the schema-9008 profile below) | 22 complete source B-Reps (unreleased: the SolidWorks 2011 plaque and the NX part whose swept surfaces were explicitly unsupported decode now); three assemblies (SolidWorks 2020 with 8 bodies and 10 placed instances, two NX files) map their instances and transforms | Public files only; no catalog comparison and no independent CAD measurement in this repository | Twenty-two strict OCCT conversions (unreleased: the parametric path is selected for vertex-bounded arcs, surface-parameter and intersection curves and trimmed spheres; cones measure v along the axis; unbounded blend spines are bounded; sliver-face crossings and ring edges running against their source circle are handled; cliff-edge blends are the pipe around their spine; an edge through a sphere pole is split there and an edge on a periodic seam carries a pcurve per orientation; swept surfaces are extruded sections and offsets follow the sense of their basis; an analytic branch that misses the source points on the scaled surfaces is retried unscaled and blend contact curves are corrected by their chart residuals), against four with the analytic selection alone | Assembly structure is returned beside the bodies and not applied to their geometry; the public files are not redistributed |
| [Older standard keys](builtin-profiles.md#older-standard-keys-parasolid-9-to-16) (unreleased): six standard keys of Parasolid 9 to 16 (NX, MicroStation, IronCAD, SolidWorks 98 and the reference key) and their families | 24 public X_T files (NIST engineering-design-models and other GitHub repositories) read to their terminators; the BODY, FIN, ATTRIB_DEF, CURVE_DATA and LIST layouts are read from those streams, the reference's transmit example is authored verbatim and in the 9008 layouts in X_T/X_B fixtures | 24 complete source B-Reps (4 to 300 faces, two Parasolid 9 assemblies of 9 bodies) | Public files only; no catalog comparison and no independent CAD measurement in this repository | Strict OCCT conversions of the four MicroStation files, the 300-face part in about 20 s, and of 10 of the other 20 (eight more in partial mode, two rejected by BRepCheck); measured in the corpus matrix | Only the node types those files carry are declared; the one SolidWorks 2022 file under the reference key stops at CYLINDER (51), and would stop at HELIX_CU_FORM (163) with its types declared; the files are not redistributed |
| SolidWorks partitions (15 exact keys, SolidWorks 2015-2026) | 75 partition, 58 ghost-partition and 811 local-body X_B streams from 63 Parts; the historical replay had four partition failures (then-undeclared HELIX_CU_FORM, base type 184, NURBS padding; current declarations and padding handling have not been re-measured on these inputs); deltas stop at unknown base type 3 | 902 complete B-Reps; 26 explicitly partial with a retained SWEPT_SURF; 12 bodiless partitions | sldkit point/FIN/NURBS parity for the 2026 key; display-outline comparison after OCCT conversion is recorded in sldkit, not here | No partition conversion validation in this repository | Delta application and final saved configuration reconstruction remain unsupported; see [SolidWorks partitions](solidworks-partitions.md) |

Runtime, build, and installation of the built-in parser require no external
schema catalog or CAD installation. Normal build/package dependencies are
separate from parser runtime inputs. The implementation does not download
catalogs. Development provenance includes optional catalog comparisons and a
membership-only audits for types 176 and 204 in base 13006; see
[that evidence boundary](builtin-profiles.md#embedded-intersection-data-revision-4).
Historical catalog comparisons did not generate the profiles. The V30 revision-3
developer audit and its version-specific definitions are documented separately.

### Schema families

A transmit key is `SCH_<modeller>_<effective>[_<base>]`. The effective schema
and, for embedded keys, the base fix the node layouts; the modeller component
is the producer's Parasolid build number, which changes with every CAD release
and service pack (SolidWorks 2024 exports carry `SCH_3501251_35102_13006`, its
saved Parts `SCH_3501210_35102_13006` and `SCH_3501256_35102_13006`). The
registry therefore derives one family profile per `<effective>[-<base>]` from
the exact profiles that share it, its members, and, for an embedded family,
from every other reviewed profile of the same base, its base contributors: the
union of their reviewed definitions and base-membership claims. Base 13006
declarations describe the same base whichever effective schema a key names, so
a type reviewed under one 13006 key (TORUS, OFFSET_SURF, WORLD, the B-spline
surfaces) is available under every 13006 family. A node type whose layout or
membership the contributors dispute is excluded from the family and stays
unknown. Exact profiles are selected first and are completed by their family;
a key with no exact profile is read with its family profile and reports
`coverage: family_inferred`, `profile_id: family-<family>` and the requested
key. Types outside the merged declarations still fail closed with
`schema.builtin_profile_uncovered_type` or `schema.unknown_base_type`, and a
key whose family has no member fails with `schema.missing_base_schema`.

A family match means the layouts of its schema revision were reviewed, not
that the producer or build was. It carries no producer evidence of its own;
the evidence is the members' and base contributors'. The family digest hashes
the sorted member and base-contributor identities (`profile_id`, revision,
`profile_sha256`), so it changes whenever any of them changes; see the
[hash contract](builtin-profiles.md#hash-and-revision-contract).
`parasolid_kit.builtin_profiles()` lists the exact profiles and the derived
families; this table is checked against it.

| Family | Merged types / field groups | Reviewed profiles | Base contributors | Family SHA-256 |
|---|---|---|---|---|
| `10004` | 47 / 408 | `nx-1000252-10004-r1` | none | `7cdfd3ac33bff689a6e352b350251d2e3aa35a15fdf05443141b6672b5bbcb5c` |
| `11004` | 34 / 327 | `ironcad-1100129-11004-r1` | none | `bd57b76b304deaad7d465a8bc6793f4a9a67e976b2fdb3f3eb16afe4aa784894` |
| `12006` | 15 / 130 | `solidworks-1200000-12006-r1` | none | `28709be637a0ec323f439eb35e3c935cc8b324ec1e73886903a3a6e4f5de3746` |
| `13006` | 58 / 474 | `icad-1300218-13006-r1`, `icad-1302234-13006-r1`, `onshape-sch13006-r6` | the other 38 profiles of base 13006 | `9e9e3cec48091d4771dd669cd811fb12373b5f6e764b01277eca8907bcff66b5` |
| `15003` | 28 / 242 | `icad-1500000-15003-r1` | none | `832c3e08690feac9c9289aed7d7dc1fb6e94e21f676aaadcb83c7dcee54071ba` |
| `16004` | 30 / 246 | `nx-1600000-16004-r1` | none | `a735b30e1daef6a0c1c1ec313e26e93915c3592fee3dc967341dcf47723cd6de` |
| `16100` | 39 / 311 | `icad-1700000-16100-r2` | none | `95552b33d166be3fac54cda9879e23f7b054509765df97c13232c1ab79d6702b` |
| `19008` | 20 / 181 | `icad-1901000-19008-r1` | none | `ce584d52f3f158de1e68b4b1266119985b993c0779bceea4e6d2c6a55ffbe471` |
| `20000` | 46 / 411 | `icad-2401000-20000-r3` | none | `e28128bab31fc0ad544682c0da71c2861566542cae67b5309ec7d839fc0ed9d0` |
| `26105` | 38 / 336 | `icad-2601000-26105-r1` | none | `e9853d8913065b3d67a7f92166b844553a7c362ac2f29354c19fe2eb6169a9de` |
| `28002` | 41 / 370 | `icad-2800000-28002-r1` | none | `6d6dad26be6666a889d408ef28148864d6c37d22614a683d6a1ab8bd9d252734` |
| `28101` | 26 / 216 | `icad-2901000-28101-r2` | none | `c4620698b5c8743c68d8392346b4fb3b77e01ed7cfd9c1b7751ab477671f2d27` |
| `30000` | 44 / 356 | `onshape-sch30000-r3` | none | `a647445d2220d16319cdbe07255da15da80e093d0ab885526f8209cb66228698` |
| `32001` | 31 / 263 | `icad-3200000-32001-r2` | none | `eb36667e08f5c5344b69bef13b960a3f654b553c4818de8d865464863348f9d9` |
| `33103` | 25 / 219 | `icad-3301000-33103-r1` | none | `0f378a23e4fa883b08c32b64b08e1b69dd2723f9e5f11c62761b24e22e24082b` |
| `9003` | 44 / 360 | `nx-900141-9003-r1` | none | `6f398e0e39375a7dca727a53135731601777eb6b1ba117b8f1582907e1b52fff` |
| `9008` | 44 / 377 | `microstation-901297-9008-r1` | none | `0423090b256712358e99f211eac964a01bde5bdc6977504e1de868cd301c18c6` |
| `15003-13006` | 58 / 474 | `icad-1500137-15003-13006-r1`, `icad-1500245-15003-13006-r1` | the other 39 profiles of base 13006 | `35343490a6876a49868a4351a6f809c99eee4b76840a6cca78b6a085033d8b40` |
| `16100-13006` | 58 / 474 | `icad-1700223-16100-13006-r1`, `icad-1700256-16100-13006-r1` | the other 39 profiles of base 13006 | `859ba17153c523dee68d759b51536edb0e61b9ec59bebc93cf7901c65ac06506` |
| `19008-13006` | 58 / 474 | `icad-1901261-19008-13006-r1`, `icad-1901315-19008-13006-r2` | the other 39 profiles of base 13006 | `031f75d59786510b29d8067f6d8e510c9f3fc1a6d0910a2b045ef634022bac47` |
| `20000-13006` | 58 / 474 | `icad-2100293-20000-13006-r2`, `icad-2100311-20000-13006-r1`, `icad-2401260-20000-13006-r1`, `parasolid-2400164-20000-13006-r2` | the other 37 profiles of base 13006 | `cc48d2f53ccd66dd7074961e5347ee4ab9a7f84157715e97ba10964421725da2` |
| `25001-13006` | 58 / 474 | `nx-2600169-25001-13006-r2`, `parasolid-2600217-25001-13006-r2` | the other 39 profiles of base 13006 | `035da6ba6a75a432353c786afb9d8574620bd319248f05d389aa0e333ff7531c` |
| `26105-13006` | 58 / 474 | `icad-2601246-26105-13006-r1`, `solidworks-2700199-26105-13006-r2` | the other 39 profiles of base 13006 | `a1414c458175d697895dcf0faff66ac545b079e0274a35c90366676a54c679af` |
| `28002-13006` | 58 / 474 | `icad-2800188-28002-13006-r1`, `solidworks-2800191-28002-13006-r2`, `solidworks-2800209-28002-13006-r2` | the other 38 profiles of base 13006 | `035899578369acab4f49407028a75c76d3dfddd8908b935a62eed51dcb1bc812` |
| `28101-13006` | 58 / 474 | `icad-2901199-28101-13006-r1`, `solidworks-2900166-28101-13006-r2`, `solidworks-2900190-28101-13006-r2`, `solidworks-2900195-28101-13006-r2` | the other 37 profiles of base 13006 | `6668a671b75a21d8b54617697b56c2d38f5b8910035215a594b8277c9125ac7c` |
| `30000-13006` | 58 / 474 | `icad-sch30000-13006-r6`, `solidworks-3000226-30000-13006-r2`, `solidworks-3000298-30000-13006-r2` | the other 38 profiles of base 13006 | `e4af9eb85bac9f28afe7f46ae65e2407df5dd2bfb58c2563f05ef2a3fb9dbb46` |
| `30100-13006` | 58 / 474 | `nx-3001232-30100-13006-r2` | the other 40 profiles of base 13006 | `c181b3e908ad56ab1a710827473c12c5c30253907b8add300da872b3f6accb9d` |
| `31001-13006` | 58 / 474 | `solidworks-3100203-31001-13006-r2` | the other 40 profiles of base 13006 | `d9bbd3b1256106b78f658094a847f4257c3b2fa84ee31d615670447be30b8cfe` |
| `31100-13006` | 58 / 474 | `solidworks-3101290-31100-13006-r2` | the other 40 profiles of base 13006 | `e91bb6905efcc1d6faac96695dff3f4708f17bcb7923a8409f92092f1b215197` |
| `32001-13006` | 58 / 474 | `icad-3200152-32001-13006-r1`, `icad-3200252-32001-13006-r1` | the other 39 profiles of base 13006 | `22b379d82101076d7cad17c23de3bdbb5dd3d08d9532e33c5133bd8013d8337a` |
| `33103-13006` | 58 / 474 | `icad-3301231-33103-13006-r2` | the other 40 profiles of base 13006 | `7a42e473be92d2e76a1d837db1a3e6d9825bccd52317cdd07f863ca13905b3e5` |
| `34001-13006` | 58 / 474 | `nx-3400176-34001-13006-r2` | the other 40 profiles of base 13006 | `acc400560e39b4a66181eef99d3545a7becfbdb787767304897f40777914449f` |
| `34101-13006` | 58 / 474 | `icad-sch34101-13006-r3`, `solidworks-3401254-34101-13006-r2` | the other 39 profiles of base 13006 | `5114b6dfe340d00266584e93333537393b7968f76c333468e4b7b42f7be2ce01` |
| `35102-13006` | 58 / 474 | `solidworks-3501210-35102-13006-r2`, `solidworks-3501256-35102-13006-r2` | the other 39 profiles of base 13006 | `0202ba52c2be767950aaf07cc0019e4ac15e60919c895c3c1f6c2c3a2edce12e` |
| `36001-13006` | 58 / 474 | `solidworks-3601228-36001-13006-r2` | the other 40 profiles of base 13006 | `8dd3e4575d2b1f032292a6f9303c69d246f35775e4c43f2f836a2c68c0dcd8ce` |
| `37102-13006` | 58 / 474 | `onshape-sch37102-13006-r3`, `solidworks-sch37102-13006-r3` | the other 39 profiles of base 13006 | `952cacfd1e526d70c7b0917fdc29811368640b9b5728d895fa9e3ca817838cc5` |

No compiled family currently excludes a disputed type. Standard families have
no base contributors and declare complete layouts only for the node types
their members checked, so a file from another producer under such a schema
fails closed at its first undeclared type. Embedded families take every
reviewed 13006 declaration and membership audit; version differences arrive as
each stream's embedded edits as before.

### Detailed verified scope

The development checkout adds the exact iCAD V34 key
`SCH_3401212_34101_13006`, now `icad-sch34101-13006-r3`. It is not included in the
published 0.2.0 release. This profile reuses the reviewed 13006 subset and only
the existing type-204 absence audit; revision 3 admits the blend, spun-surface
and SP-curve dependency types observed under the key. No nearby-key fallback is used. See the
[V34 evidence and limits](builtin-profiles.md#icad-v34-embedded-key).

The source tree adds `onshape-sch37102-13006-r3` for current Onshape exports
whose internal key is exactly `SCH_3701212_37102_13006`. This unreleased profile
covers 41 base types / 339 field groups: analytic solids, elliptical edges,
direct NURBS curves/surfaces, intersections, trimmed and surface-parametric
curves. Fully declared types 176 and 204 are accepted based on their confirmed
absence from base 13006. Multi-body B-Rep does not establish assembly semantics.
See the [compound-geometry evidence and retained accuracy failures](onshape-composite.md)
and the earlier [direct NURBS evidence](onshape-current-parametric.md).


The source tree compiles fifteen exact `SolidWorks` partition keys, from
`SCH_2700199_26105_13006` (SolidWorks 2015) to `SCH_3701229_37102_13006`
(SolidWorks 2026, now `solidworks-sch37102-13006-r3`). All share one reviewed
declaration set of 47 base types / 412 field groups: the 13006 subset, WORLD,
the reviewed TORUS, blend, offset and spun surfaces, and the public-reference
SWEPT_SURF (67), which the B-Rep mapper decodes as a swept surface (the
released packages retain it as an explicitly partial surface). Delta streams
remain unsupported. See
[SolidWorks partitions](solidworks-partitions.md) for the local evidence.

For V30, default parsing uses `onshape-sch30000-r3` revision 3 only for the exact internal
key `SCH_3000000_30000`. Its coverage is `verified_subset`: Onshape V30 text and
neutral binary exports, zero user fields, and single-solid boxes, prisms,
cylinders, through-holes, spheres, verified solids with elliptical edges, and
the [new frustum and ring-torus cases](builtin-profiles.md#v30-revision-3-new-analytic-producer-pairs).
The same producer with a different modeller/key
component is not selected. See [profile provenance](builtin-profiles.md) for
sources, canonical hash, covered types, and local evidence.

The V30 built-in raw decoder covers 44 types / 356 field groups. Revision 3
adds cone/torus, trimmed and intersection curves, SP_CURVE and NURBS dependencies.
A local complex model passes full raw/B-Rep comparison with an exact caller
catalog. See the [parametric viewer report](v30-parametric-viewer.md) for
conversion and display evidence, approximation limits and source-unit handling.
The B-Rep bounding box encloses source vertices; it is not a curved-shape bounds
oracle. Core area/volume are available only for planar polygonal topology.

Default parsing also accepts `SCH_1300000_13006` with `onshape-sch13006-r6`
and `SCH_3000310_30000_13006` with `icad-sch30000-13006-r6`. They share the
reviewed 13006 base subset of 30 types / 240 base field groups, including cone
(52), intersection (38), chart (40), limit (41), trimmed curve (133) and shared
geometry owner (141), and retain intersection chart/limit references.
The V13 profile additionally covers SP_CURVE (137), B_CURVE (134), NURBS_CURVE
(136), CURVE_DATA (135), BSPLINE_VERTICES (45), KNOT_MULT (127), and KNOT_SET
(128), plus B_SURFACE (124), NURBS_SURF (126), and SURFACE_DATA (125):
40 types / 327 field groups. It maps surface/parameter-curve references
and preserves homogeneous control coefficients, distinct knots and multiplicities.
CURVE_DATA and SURFACE_DATA remain raw metadata. Producer evidence includes
open degree-1 nonrational UV curves on planes and cylinders, degree-2 open and
periodic cylinder UV curves, and degree-3 rational periodic UV boundaries on
nonrational bilinear B-surfaces (planar and nonplanar). A separate
[surface campaign](nurbs-surface-evidence.md) verifies degree-3-by-2 open
nonrational/rational patches and nonrational periodic sheets in each axis,
including nonuniform knots and overlapping control rows or columns. A further
[rational periodic campaign](rational-periodic-surface-evidence.md) verifies
weighted periodic sheets in each axis, including seam positions and derivatives.
A [doubly periodic campaign](doubly-periodic-surface-evidence.md) verifies
degree-3-by-2 nonrational/rational sheets periodic in both axes, including their
shared control-grid overlap and seam intersections. Control coefficients,
homogeneous weights, imaginary knots and periodic flags remain as stored.
Malformed knot multiplicities and empty active parameter domains are rejected.
Cylinder angles remain unwrapped: an open UV line spanning one turn can describe
a closed 3D curve. The cylindrical campaign passes parsing and parameter checks,
but one curve per model exceeds the imported STEP edge tolerance by about 1.03%.
The later high-degree campaign stays within the STEP files' declared distance
accuracy, but exceeds the imported per-edge tolerances; both results remain
explicit. This does not establish general spline evaluation or STEP reconstruction.
The [STEP accuracy audit](step-accuracy.md) attributes these saved discrepancies
to existing tolerant FIN/EDGE geometry and STEP boundary approximation, and
separately measures imported pcurve consistency and area integration.
V30 revision 3 separately audits these geometry dependencies; historical V13
producer evidence does not become new V30 producer evidence.
The `TrimmedCurve` model retains the basis curve, endpoints and parameters.
Some inputs leave both parameters of a trim on a LINE unset. They are then the
parameters of the stored end points under the published parameterisation
`R(t) = P + tD`; each point must lie on the line within 1e-8 and the usual
order rule still applies. The raw fields remain null. Unset parameters on any
other basis, or only one unset parameter, are still rejected.
V13 type 133 is verified using new producer text/binary pairs and independent
STEP comparisons; its scope is still the exact V13 key.
The embedded profile applies each transmitted definition/delta and retains its
source, edits, and byte range. Unknown base membership stops before decoding
the definition; an unimplemented type is never treated as absent. Revision 4
of the embedded profile explicitly classifies type 204 as absent from 13006,
so its full input-defined fields can be decoded. Its intersection-data pointer
is exposed as a source reference; numeric UV semantics remain uninterpreted.
B-Rep roles follow fields copied from the reviewed base, so insertions may
shift wire positions without assigning semantics to inserted names. The
optional `intersection_data` reference has a separate reviewed Append contract
requiring a transmitted scalar pointer of class 204.

V13 validation covers paired Onshape text/neutral binary exports of boxes,
spheres, cones/frustums, solids with elliptical edges, and intersecting-cylinder
booleans. Intersection curves remain source models: their point arrays identify
the branch. That producer campaign verified source decoding, without adding a
public core evaluator or curved core metrics. The optional adapter now provides
[bounded intersection construction](v30-parametric-viewer.md) under the geometry
coverage constraints below; this does not expand the campaign's producer evidence.
Embedded real-data validation uses
neutral binary streams extracted locally from one iCAD file; embedded text
has synthetic decoder and B-Rep tests, without a real paired producer export.
This does not add `.icd` container support or an independent geometry oracle
for its extracted streams. See [profile provenance](builtin-profiles.md).
The existing 17 extracted iCAD V30 streams now all reach complete raw parsing
and source B-Rep/topology. The 15 observed trims reference lines; their endpoints
agree with evaluation of the stored basis and parameters. This consistency check
does not establish general trimmed-curve evaluation or OCCT export support.

The V13 NURBS and sheet evidence is limited to the degrees and motifs in the
linked campaigns. It does not extend the V30 or iCAD profiles, establish arbitrary
NURBS compatibility, or add a public evaluator or rational/periodic OCCT path.
Other producers/keys or embedded bases remain outside these verified subsets;
[assembly structure](builtin-profiles.md#assembly-structure-assembly-instance-and-transform)
is returned beside the bodies and not applied to them. Onshape/iCAD evidence
does not establish general multiple-body support. The SolidWorks partitions
include multi-body examples, without establishing arbitrary multi-body or
configuration compatibility. Structural checks
control parsing; matching a key is not a guarantee that every shape emitted
under that key has been verified.

An explicit `SchemaProvider` or `--schema-dir` selects the caller's catalog
without fallback. For `SCH_<modeller>_<effective>` the required catalog is
`<effective>`; for `SCH_<modeller>_<effective>_<base>` it is `<base>`, even when
embedded definitions are present. `DirectorySchemaProvider` considers only
`sch_<provider-schema>.sch_txt`, rejects symbolic links, and does not recurse or
substitute a nearby version. The human-readable common-header `SCH` in X_T is
not used for selection. [Schema catalogs](schema-catalogs.md) describes
the external-provider path.

Unsupported default keys use `schema.missing_base_schema`; uncovered built-in
standard-schema types use `schema.builtin_profile_uncovered_type`. Embedded
membership failures use `schema.unknown_base_type` or
`schema.unsupported_base_type`. None causes a guessed layout or a different
profile to be selected. Siemens catalogs and native CAD fixtures are excluded
from wheel/sdist; runtime code does not download them.

## Result stages and caller responsibilities

Success applies to the requested stage and the supplied stream. In particular,
`ParsedBrep.complete`, `BrepSummary.complete`, and `BrepModel.complete` describe
source B-Rep mapping. They do not certify interpreted attributes, a public
geometry evaluator, successful conversion, or the final saved state of a CAD
container. `complete=True` can coexist with unavailable curved metrics or
unsupported OCCT conversion.

| Entry point / stage | Meaning of success | Failure or remaining boundary |
|---|---|---|
| `inspect_xb` / `inspect_xt`; CLI `inspect` | Header validated | Node stream and geometry remain unvalidated |
| `parse_xb` / `parse_xt`; CLI `parse` | Complete raw stream decoded to its terminator using an exact provider | Unsupported layouts and malformed input fail; raw success does not require a complete B-Rep |
| `map_brep` / Rust `map_xb_brep` / `map_xt_brep`; `read_brep`; CLI `check` / `parse --brep` | Source topology checked and supported geometry mapped | Well-formed unmapped geometry is explicit with `complete=False`; invalid references/topology raise parse errors |
| `compare_documents`; CLI `compare` | Decoded structure and values agree after remapping within the selected tolerances | Equivalent documents are not an independent geometric oracle |
| `write_xb` | Original bytes retained for an unmodified binary document | Does not encode edited values; replay equality alone does not validate decoding |
| OCCT / STEP / preview / CadQuery | The requested adapter's separate completeness, validity and constraints pass | Conversion and preview coverage do not broaden the input profile |
| Rust `partial` | Known fragment records and available read spans recovered | No document-wide framing or full delta state; the caller retains unrecognized bytes and incomplete status |

CLI status `0` means success for that command's stage. `parse --brep` also
returns `0` when mapping returns an explicitly incomplete model; inspect its
JSON `brep.complete`, or use `check` to enforce completeness by exit status.
Status `1` reports an incomplete `check` result or non-equivalent `compare`
result, and `2` reports input/schema/parse/conversion errors. An explicitly
allowed partial preview can likewise be generated with status `0`.
See the [API diagnostics](api.md#inputs-and-schema-selection) for exact codes.

CAD adapters extract and bound the stream, select configurations, pair
partitions and deltas, and retain container/decompression provenance. The strict
parser's ranges start at the supplied complete stream; `partial` ranges start
at its supplied body slice. IDs are local to their document/stream. Physical
units must come from caller evidence. The caller converts model-space lengths
once and preserves knots, angles, directions, weights and surface UV values.
Strict NURBS models retain source homogeneous coefficients; partial readers
return Euclidean poles. See the
[partial-reader contract](../crates/parasolid-core/PARTIAL_READERS.md).

An iCAD container adapter is planned but has not been created. Existing iCAD
evidence concerns extracted Parasolid bytes only. SolidWorks container and
configuration handling belongs to sldkit; its shared partial readers do not
establish complete saved-state reconstruction.

## B-Rep topology

The typed source model represents bodies, regions, shells, faces, loops,
half-edges, edges, vertices, points, curves, and surfaces. It validates required
references, ownership chains, loop and edge rings, inverse relationships, and
solid edge manifold conditions before returning the model.

Topology and geometry IDs are local to one parsed document. Every mapped value
retains a reference to its source node and byte range; these IDs must not be
treated as persistent Parasolid identifiers.

## Typed geometry

With a suitable explicit schema provider, the mapper provides typed values for
the following effective geometry types. This broader mapper table does not
expand the built-in profile's raw/type coverage:

| Category | Effective types |
|---|---|
| Analytic curves | `LINE`, `CIRCLE`, `ELLIPSE`, `PARABOLA`, `HYPERBOLA` |
| Curve wrappers | `TRIMMED_CURVE`, `SP_CURVE`, `INTERSECTION` |
| Analytic surfaces | `PLANE`, `CYLINDER`, `CONE`, `SPHERE`, `TORUS` |
| Surface wrappers | `OFFSET_SURF`, `BLENDED_EDGE`, `BLEND_BOUND` |
| NURBS | `B_CURVE`, `NURBS_CURVE`, `B_SURFACE`, `NURBS_SURF`, and their control-point and knot records |

Valid geometry outside this table remains visible as `UnsupportedGeometry`.
It produces a recoverable diagnostic and sets `BrepModel.complete` to `False`.
Malformed references, arrays, or topology fail with `ParseError` instead of
being converted to an empty shape.

## Parse, OCCT, and STEP geometry coverage

This adapter table assumes an already mapped `BrepModel`; it does not imply
built-in raw support for every listed type. This table is rendered from
`parasolid_kit.interop.occt.GEOMETRY_COVERAGE`; a test requires the embedded
text to match that machine-readable contract exactly. `conditional` means the listed data and construction constraints must pass.
Structural checks precede the OCCT import; kernel validity and sampled geometry
checks follow construction. UV/intersection approximations are explicitly
recorded, while unsupported variants remain rejected.

<!-- BEGIN GENERATED I7 GEOMETRY COVERAGE -->
| Category | Geometry kind | Parser | OCCT | STEP | Exact constraints |
|---|---|---:|---:|---:|---|
| curve | `line` | exact | exact | exact | two vertices |
| curve | `circle` | exact | exact | exact | vertex-free full period, or the arc between its two vertices |
| curve | `ellipse` | exact | exact | exact | vertex-free full period, or the arc between its two vertices; major radius >= minor radius |
| curve | `parabola` | exact | exact | exact | two vertices on one exact branch |
| curve | `hyperbola` | exact | exact | exact | two vertices on one exact branch |
| curve | `trimmed` | exact | exact | exact | explicit basis, parameters, endpoint positions, and two vertices |
| curve | `nurbs` | exact | conditional | conditional | nonrational or rational 3D curves, open or in the stored periodic form, or 2D curves used by SP_CURVE |
| curve | `surface_parametric` | exact | conditional | conditional | nonrational or rational 2D NURBS on a supported surface, as FIN or edge curve; bounded 3D approximation and source FIN checks |
| curve | `intersection` | exact | conditional | conditional | source-identified CHART/LIMIT branch on two supports, open or closed; bounded numerical fitting |
| curve | `polyline` | exact | unsupported | unsupported | source points, sense, closure flag, and base parameter are retained; OCCT polyline edge construction is not implemented |
| curve | `unsupported` | unsupported | unsupported | unsupported | unknown source semantics are retained without inference |
| surface | `plane` | exact | exact | exact | explicit trim loops |
| surface | `cylinder` | exact | exact | exact | two vertex-free circular boundary loops, or face trimmed by its source loops |
| surface | `cone` | exact | exact | exact | frustum with two positive-radius circular boundary loops, or face trimmed by its source loops; the source v is the distance along the axis; an edge through the apex is split there |
| surface | `xt_cone` | exact | conditional | conditional | frustum with two positive-radius circular boundary loops; XT v advances against the axis while the radius increases; apex-crossing edge splitting is not covered by the current parametric topology path |
| surface | `sphere` | exact | exact | exact | untrimmed closed face, or face trimmed by its source loops; OCCT seam and pole topology is generated; an edge through a pole is split there and an edge on the seam carries a pcurve per orientation |
| surface | `torus` | exact | exact | exact | closed ring torus, or ring, horn, apple or lemon torus trimmed by its source loops |
| surface | `spun` | exact | conditional | conditional | revolution of a mapped profile curve about the spin axis; faces trimmed by their source loops |
| surface | `swept` | exact | conditional | conditional | linear extrusion of a mapped section curve along the sweep direction; faces trimmed by their source loops |
| surface | `nurbs` | exact | conditional | conditional | nonrational or rational 3D row-major grid, open or in the stored periodic form; one or more trim loops |
| surface | `offset` | exact | conditional | conditional | supported exact basis surface, offset along its normal and against it for a basis of negative sense; I7 verifies a non-periodic NURBS basis |
| surface | `blended_edge` | exact | conditional | conditional | rolling-ball and cliff-edge blends of equal offsets as the B-spline pipe of the blend radius around the spine, within the validation tolerance; a zero-offset blend is a construction reference only |
| surface | `blend_boundary` | exact | conditional | conditional | construction surface only: its intersection with a support is the blend's contact curve, interpolated from the spine within the validation tolerance |
| surface | `mesh` | exact | unsupported | unsupported | raw embedded pools, linked transform, and unembedded source fields are retained; faceted surface conversion is not implemented |
| surface | `unsupported` | unsupported | unsupported | unsupported | unknown source semantics are retained without inference |
<!-- END GENERATED I7 GEOMETRY COVERAGE -->

## Derived metrics

The parser-only package intentionally has no geometry-kernel or tessellation
dependency. It can derive:

- a bounding box from mapped topological vertex points;
- area when every face is planar and every loop is polygonal; and
- volume when the body is solid and its complete boundary is planar.

Metrics whose exact preconditions are not met are `None`. Curved area and
volume are not approximated from endpoints or from a hidden mesh.
`BrepSummary` labels these values as source transmit units because the parser
does not currently establish the physical length unit.

## Known exclusions

- No X_T writer or general-purpose edited-document serializer.
- No native iCAD `.icd` or other CAD-container parser.
- Assembly structure is returned as `BrepModel.assemblies` and `instances`
  with the stored transforms; `map_brep` does not apply them, and
  `to_occt(place_instances=True)` places transformed copies on request; no
  visibility or appearance model.
- Attributes are returned as `BrepModel.attributes` with their definition
  identifiers and integer, real, text, unicode, point, vector, direction,
  axis and tag values; a pointer field is kept by name only, and no attribute
  is applied to the OCCT, STEP or preview output.
- No parser-core tessellation, remote/cloud viewer, appearance reconstruction,
  or general-purpose mesh export; I6 is an optional OCCT-derived inspection
  view.
- No STEP assembly/product semantics, names, colors, PMI, or edited feature
  history; I4 exports geometry/topology only.
- No inferred CadQuery assembly, Workplane chain, or editable feature history;
  I5 returns shapes only.
- Application-owned user fields are retained separately as integer words in
  `RawNode.user_fields` and included in document comparisons. `USFLD_SIZE`
  words are read after every node: the April 2008 XT reference restricts them
  to nodes visible at PK, but Parasolid 26 and 27 transmit files (Siemens'
  acceptance-test examples) append them to value nodes as well, and only the
  uniform framing parses those files to their terminators. Compiled profiles
  read them like caller-supplied catalogs. Synthetic X_T/X_B parity and those
  four public files are the evidence; no meaning is assigned to the words.
  This does not supply missing schema definitions or enable schema-free parsing.
- A transmitted opaque `q` field is rejected because its neutral-file byte
  representation is not defined by the current schema model.

Resource use is bounded by `ParseLimits`, including file size, node and schema
counts, field and string sizes, variable-length arrays, and retained
diagnostics.

Optional `[occt]` and `[cadquery]` installation profiles preserve the lazy,
guarded runtime boundary. The base parser and `[occt]` profile support Linux,
macOS, and Windows. The `[cadquery]` profile supports Linux and macOS only;
Windows CadQuery adapter calls fail with `interop.unsupported_platform` before
native import because the tested runtime crashes during process shutdown.
Windows users can use `[occt]` for conversion, STEP export, and preview.

I7 adds exact ellipse, parabola, hyperbola, explicit
trimmed curve, cone frustum, full sphere, full ring torus, non-rational 3D
NURBS, and exact offset-surface paths to the I3 point/line/circle/plane/cylinder
baseline. Vertex-bounded arcs, surface-parameter and intersection curves,
trimmed sphere and torus faces and multi-loop NURBS faces select the
parametric path, which resolves bounded conics using source orientation and
endpoints; the analytic path alone still requires explicit trim records for
arcs. Rational
NURBS surfaces are built with their weights, and a closed or periodic one over
its stored knot vector, checked to meet itself (unreleased; the released
packages reject the homogeneous control grid). The fixed Gauss rule of the
report's metrics integrates a multi-span rational face to about a tenth of a
percent, so its area is evidence, not an exact measure. The V13
source-storage campaigns establish bounded homogeneous-coefficient and
pole/knot evidence for parsing.
The adapter validates source references,
basis-reference cycles, OCCT topology, bounding box, area, and volume and
performs no source-geometry healing. The [parametric extension](v30-parametric-viewer.md)
records bounded UV/intersection approximation, seam construction and associated
source mappings in its reports.

I4 can export any complete, valid documented OCCT result directly as geometry/topology-only
AP242. Output is staged, bounded, accompanied by a schema-version-1 conversion
sidecar, and cold-reimported in a separate process. Validation requires reader
success, non-empty valid shape, body/face counts, bounding box, area, and
volume; it deliberately does not require identical STEP edge/vertex counts or
topology IDs. Existing outputs are not overwritten by default, and partial or
incomplete conversions are rejected.

I5 wraps the body subshapes produced by that same conversion. One source body
returns its most specific CadQuery `Shape` subclass; multiple bodies return an
explicit `cadquery.Compound`, and `to_cadquery_shapes()` preserves source-body
order. The adapter compares validity, face count, bounding box, area, and
solid-only volume with the OCCT result and does not infer assembly or
feature-history semantics.
Source mapping is not claimed to survive later CadQuery modeling operations.

I6 tessellates the unchanged `OcctConversionResult` into deterministic GLB
face triangles and edge line strips. Edge polylines reuse the same bounded OCCT
face mesh, so I7 analytic and NURBS edges do not require an unbounded secondary
curve sampler. A separate JSON manifest maps picking IDs
to conversion-local face/edge keys, Parasolid IDs, source node IDs and byte
ranges, geometry kinds, bodies, and diagnostics. Triangle, vertex, curve
sample, diagnostic, subshape, and total output sizes are independently bounded;
limits fail with `preview.limit_exceeded` and never trigger hidden
simplification. Missing mappings fail by default, while explicit partial mode
shows a visual warning and missing-entity list.

The viewer is a fixed-hash HTML/CSS/JavaScript bundle using three-cad-viewer 5.0.6,
with original third-party notices embedded in JS/CSS (see the [preview API](api.md)).
It uses WebGL without VTK, CDN, or a Node.js runtime. Its server binds an ephemeral
`127.0.0.1` port by default, serves only the five generated/reviewed files, and
requires explicit permission for a non-loopback bind. The browser receives no
source path or raw Parasolid bytes.

This optional conversion/export/preview does not expand the parser's source-format
coverage and does not make OCCT, CadQuery, or STEP the canonical parse result.
GLB is likewise a derived inspection artifact, not the canonical parse result.

### Viewer display boundary

The additional `--assembly` display mode uses explicit ASSEMBLY/INSTANCE links
and stored transforms; it does not infer assembly structure from geometry.
Repeated occurrences retain separate selection paths and visibility controls.
The independent `--stored-geometry --schema-dir ...` mode displays selected
POLYLINE and embedded MESH records from named caller catalogs without OCCT.
That mode is labelled a stored-geometry subset, reports no OCCT validity, and
does not establish whole-model coverage, topological trims, or solid closure.
See [geometry operations](api.md#dependency-free-geometry-operations) for the
supported pool layouts, explicit index-origin selection, and API boundaries.

`parasolid-kit viewer` (alias `view`) displays the documented OCCT conversion
subset above. Local browser checks cover solid boxes, a cylinder with a hole,
multiple bodies, and an open non-rational NURBS sheet, generated from public
synthetic B-Reps. They do not establish viewer compatibility with arbitrary
X_T/X_B exporters. The browser accepts this package's paired GLB/manifest format;
it is not a general GLB loader. Unsupported conversion geometry remains an error.

- Face/edge selection resolves to the original conversion's source IDs, nodes,
  byte ranges and diagnostics. Body groups show membership, including shared
  and unknown membership; they do not infer an assembly or feature history.
- Source completeness, conversion completeness and OCCT validity are separate
  statuses. `--allow-partial` permits incomplete source or missing mappings and
  displays a warning. Conversion must still be complete and OCCT-valid; invalid
  topology, unsupported geometry and exceeded limits remain errors. A partial
  preview can exit successfully while still omitting source geometry or mappings.
- The caller declares the actual source unit. OCCT applies the source-to-target
  scale once, and the browser retains target coordinates (default `mm`). Linear
  deflection and section position use target units; the mesh is not rescaled to
  glTF meters on loading. Colors are illustrative, not recovered CAD materials.
- Sections clip the display mesh, with caps only for complete known solid leaves.
  They do not calculate or export exact B-Rep sections. Filtering, body visibility
  and section changes clear selection; camera movement retains it.

Measurement, CAD vertex/whole-solid picking, material editing, animation and
assembly inference are outside this implementation. Source mapping describes the
original conversion and does not promise correspondence after downstream editing.
See the [viewer usage and controls](api.md#bounded-local-preview),
[development steps](../viewer/README.md), and
[local validation record](viewer-validation.md) for reproducible checks and their
platform/input boundaries.

## Current interoperability evidence

The [supported profiles](#supported-profiles) distinguish current built-in
evidence by exact key and processing stage. Earlier controlled V26/V30 pairs
were also parsed using explicit catalogs; those results do not establish
built-in support for their other keys. In particular, the earlier
`SCH_3700000_36001` path needs its exact caller catalog, while
`SCH_3701229_37102_13006` has the separate partition-only built-in profile.

These cases test the implemented paths but do not guarantee compatibility with
all exporters or modeling features. The public package deliberately contains
no proprietary schema catalog or CAD fixture; see the
[corpus policy](../corpus/README.md) for redistribution requirements.
