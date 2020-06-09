pub mod curve;
pub mod effect;
pub mod lerp;
pub mod simplify;
pub mod smooth;
mod snippet;
pub mod span_cursor;
pub mod time;

pub use crate::curve::{SegmentStyle, StrokeSeq};
pub use crate::effect::{Effect, Effects, FadeEffect};
pub use crate::lerp::Lerp;
pub use crate::time::{Diff, Time};
pub use snippet::{SnippetData, SnippetId, SnippetsCursor, SnippetsData};
