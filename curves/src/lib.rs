mod curve;
mod effect;
mod lerp;
pub mod save;
mod simplify;
mod smooth;
mod snippet;
mod span_cursor;
mod time;

pub use crate::curve::{StrokeRef, StrokeSeq, StrokeStyle};
pub use crate::effect::{Effect, Effects, FadeEffect};
pub use crate::lerp::Lerp;
pub use crate::snippet::{SnippetData, SnippetId, SnippetsCursor, SnippetsData};
pub use crate::span_cursor::{Cursor, Span};
pub use crate::time::{Time, TimeDiff, TimeSpan};
