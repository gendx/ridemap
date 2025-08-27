//! Module containing various UI utilities.

use crate::ui::tracks::TrackStats;
use image::{ImageError, ImageFormat, RgbaImage};
use log::warn;
use rand::distr::Open01;
use rand::{rng, Rng};

/// RGBA color.
#[derive(Clone, Copy)]
pub struct Color(pub [f32; 4]);

impl Color {
    /// Creates a new random color.
    pub fn new_random() -> Self {
        let mut rng = rng();
        let r = rng.sample(Open01);
        let g = rng.sample(Open01);
        let b = rng.sample(Open01);
        Self([r, g, b, 1.0])
    }
}

/// A loaded map tile.
pub struct Tile<Image> {
    /// Decoded image pixels of this tile, loaded for the current UI framework.
    pub image: Image,
}

/// Decode an image in RGBA format from PNG data.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    let dynamic_image =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png).decode()?;
    Ok(dynamic_image.to_rgba8())
}

/// Prints a warning message based on the error if the given result is not OK.
pub fn warn_on_error<E: std::fmt::Debug>(x: Result<(), E>, msg: &str) {
    match x {
        Ok(()) => {}
        Err(e) => warn!("Failed to send {}: {:?}", msg, e),
    }
}

/// Rendering statistics.
pub struct RenderStats {
    /// Number of map tiles drawn.
    pub drawn_tiles_count: usize,
    /// Statistics about GPS tracks.
    pub track_stats: TrackStats,
    /// Total number of segments, including invisible ones.
    pub segment_count: usize,
    /// Number of segments drawn.
    pub drawn_segment_count: usize,
}
