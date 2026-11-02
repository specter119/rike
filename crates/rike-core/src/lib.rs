//! rike-core: model, parsing, serialization and naming rules.
//!
//! Pure functions only: no IO, no UI, no async runtime. All offsets are byte
//! offsets into the original `&str`.

pub mod config;
pub mod inline;
pub mod journal;
pub mod model;
pub mod names;
pub mod ops;
pub mod parse;
pub mod serialize;
pub mod task;

pub use model::{Block, BlockId, Item, Page, Property, WalkEntry};
