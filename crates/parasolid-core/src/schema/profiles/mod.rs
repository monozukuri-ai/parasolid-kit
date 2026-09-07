//! Reviewed compiled profiles, registered by `BuiltinProfileRegistry::compiled`.

mod sch13006;
mod sch30000;

pub(crate) use sch13006::bspline_surface_definitions as sch13006_bspline_surface_definitions;
pub(crate) use sch13006::definitions as sch13006_definitions;
pub(crate) use sch13006::sp_curve_definitions as sch13006_sp_curve_definitions;
pub use sch13006::{icad_sch30000_13006, onshape_sch13006};
pub use sch30000::onshape_sch30000;
