//! Layout viewport size.

use dpi::PhysicalSize;

/// Layout viewport size in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Viewport {
    width: u32,
    height: u32,
}

impl Viewport {
    /// Largest accepted width or height of the layout viewport.
    pub const MAX_DIMENSION: u32 = 4096;

    /// Create a viewport with both sides between 1 and [`Viewport::MAX_DIMENSION`].
    pub const fn new(width: u32, height: u32) -> Result<Self, InvalidViewport> {
        if width == 0 || height == 0 || width > Self::MAX_DIMENSION || height > Self::MAX_DIMENSION {
            return Err(InvalidViewport { width, height });
        }
        Ok(Self { width, height })
    }

    /// Width in CSS pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Height in CSS pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    // Servo renders at a device pixel ratio of 1, so CSS and device pixels coincide.
    pub(crate) const fn physical_size(self) -> PhysicalSize<u32> {
        PhysicalSize::new(self.width, self.height)
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

/// Error returned by [`Viewport::new`] when a side is outside 1 to [`Viewport::MAX_DIMENSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("viewport {width}x{height} must have both sides between 1 and {max}", max = Viewport::MAX_DIMENSION)]
pub struct InvalidViewport {
    width: u32,
    height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sides_within_bounds() {
        let max = Viewport::MAX_DIMENSION;
        assert_eq!(Viewport::new(1, max).map(|v| (v.width(), v.height())), Ok((1, max)));
    }

    #[test]
    fn rejects_zero_and_oversized_sides() {
        let max = Viewport::MAX_DIMENSION;
        for (width, height) in [(0, 800), (1280, 0), (max + 1, 800), (1280, max + 1)] {
            assert_eq!(Viewport::new(width, height), Err(InvalidViewport { width, height }));
        }
    }

    #[test]
    fn error_names_the_rejected_size() {
        let error = Viewport::new(0, 800).unwrap_err();
        assert_eq!(
            error.to_string(),
            "viewport 0x800 must have both sides between 1 and 4096"
        );
    }
}
