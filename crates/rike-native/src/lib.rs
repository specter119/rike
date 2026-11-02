//! Everything the Rike UI needs that is not drawing: the document model, Markdown and
//! formula rendering, the vault session, the recent-vaults state file and the startup decision.
//!
//! The TSX UI reaches this crate through the `rike` native module (`api`). The `typst-math`
//! spike uses `math` directly.

pub mod api;
pub mod doc;
pub mod math;
pub mod md;
pub mod session;
pub mod startup;
pub mod state;
pub mod today;

use nsh_core::prelude::*;

/// The `rike` module of the TSX UI. The host passes [`registry`] to
/// `set_ui_sets`; the entry point must sit at the crate root.
#[native_module("rike")]
fn native(registry: &mut Registry) {
    registry.module::<api::Rike>();
}
