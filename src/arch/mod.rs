//! Architecture-specific accelerated batch span operations.
//!
//! Provides hardware-accelerated and vector/unrolled implementations for:
//! - `blend_span_rgb565`: Blends a horizontal span of pixels with a constant color and alpha.
//! - `blit_mask_span`: Blends a horizontal span of pixels using an 8-bit alpha mask.
//! - `fill_span_rgb565`: Fills a horizontal span of pixels with an opaque color.

#[cfg(target_arch = "xtensa")]
#[path = "xtensa.rs"]
mod imp;

#[cfg(not(target_arch = "xtensa"))]
#[path = "fallback.rs"]
mod imp;

pub use imp::*;
