//! The chapter's source. The Markdown text is the only truth: blocks and inline spans are ranges over it, recomputed
//! from it, and never stored back. Holds no gpui types.
//!
//! - [`Buffer`] owns the text, applies edits as transactions, and keeps undo history.
//! - [`Blocks`] partitions the text into typed blocks and updates incrementally after an edit.
//! - [`inline`] finds the markers inside one block, for a view that hides them.
//! - [`motion`] finds caret targets in the text.
//! - [`typing`] decides what the Enter key writes.

pub mod blocks;
pub mod buffer;
pub mod inline;
pub mod motion;
pub mod typing;

pub use blocks::{BlockKind, Blocks};
pub use buffer::{Buffer, Edit, EditKind, Selection};

#[cfg(test)]
mod tests;
