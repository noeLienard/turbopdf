//! # papier-tree
//!
//! The **document tree**: the only contract between the SDK (TypeScript) and the
//! engine (Rust). The SDK turns React / Markdown into this tree as JSON; the engine
//! turns the tree into a PDF and PNG previews.
//!
//! This crate is the source of truth. The TypeScript types are *generated* from it
//! (`cargo test -p papier-tree --features ts`), so both sides are identical by construction.
//!
//! See `engine/SPEC.md` for the full specification (units, inheritance, pagination rules).

pub mod node;
pub mod output;
pub mod style;
pub mod validate;
pub mod value;

pub use node::*;
pub use output::*;
pub use style::*;
pub use validate::{ValidationError, validate};
pub use value::*;

/// Version of the tree format. Bumped on any breaking change.
pub const TREE_VERSION: u32 = 1;
