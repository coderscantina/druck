//! Druck's engine as a library for the desktop spike. This is not a public API: phase 2 replaces it with a
//! designed crate, and every module here may change without notice.

pub mod bibliography;
pub mod citations;
pub mod config;
pub mod date;
pub mod diagnostic;
pub mod document;
pub mod epub;
pub mod highlight;
pub mod image;
pub mod index;
pub mod installed;
pub mod layout;
pub mod markdown;
pub mod page;
pub mod pdf;
pub mod pipeline;
pub mod spacing;
pub mod statistics;
pub mod tags;
pub mod text;
pub mod theme_path;
