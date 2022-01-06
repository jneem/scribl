mod curve;
mod draw_snippet;
mod effect;
mod lerp;
pub mod save;
mod shape_detect;
mod simplify;
mod smooth;
mod span_cursor;
mod time;

pub use crate::curve::{StrokeInProgress, StrokeRef, StrokeSeq, StrokeStyle};
pub use crate::draw_snippet::{DrawCursor, DrawSnippet, DrawSnippetId, DrawSnippets};
pub use crate::effect::{Effect, Effects, FadeEffect};
pub use crate::lerp::Lerp;
pub use crate::simplify::simplify;
pub use crate::smooth::smooth;
pub use crate::span_cursor::{Cursor, Span};
pub use crate::time::{Time, TimeDiff, TimeSpan};
