//! The typesetter: fixes for typesetting warnings, tried on a copy of the book before the author sees them.
//!
//! - [`fix`] is what a fix may be, and how it is written into a chapter.
//! - [`trial`] lays a copy out with a fix and compares the warnings, which decides whether the fix is kept.
//! - [`tool`] is the trial as the agent calls it over MCP, and the prompt that starts a run.
//!
//! None of it holds gpui types or needs an agent. The run, the queue of kept fixes, and their review are the agent
//! module's, in `agent/fixing.rs`.

pub mod fix;
pub mod tool;
pub mod trial;
