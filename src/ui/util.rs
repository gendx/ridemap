//! Module containing various UI utilities.

use graphics::image::Image;
use image::{ImageError, ImageFormat, RgbaImage};
use log::warn;
use piston_window::G2dTexture;
use piston_window::ImageSize;
use rand::distributions::Open01;
use rand::{thread_rng, Rng};
use std::borrow::Borrow;

/// RGBA color.
#[derive(Clone, Copy)]
pub struct Color(pub [f32; 4]);

impl Color {
    /// Creates a new random color.
    pub fn new_random() -> Self {
        let mut rng = thread_rng();
        let r = rng.sample(Open01);
        let g = rng.sample(Open01);
        let b = rng.sample(Open01);
        Self([r, g, b, 1.0])
    }
}

/// A loaded map tile.
pub struct Tile {
    /// Decoded image pixels of this tile.
    pub image: Image,
    /// 2D texture loaded with this image.
    pub texture: G2dTexture,
    /// PNG data of this tile.
    pub png_image: Box<[u8]>,
}

/// Trait to convert a [`Tile`] to a texture.
pub trait TextureTile<'a>: ImageSize {
    /// Texture type.
    type Target: Borrow<Self>;

    /// Converts the given tile to a texture.
    fn from_tile(tile: &'a Tile) -> Self::Target;
}

impl<'a> TextureTile<'a> for G2dTexture {
    type Target = &'a Self;

    fn from_tile(tile: &'a Tile) -> Self::Target {
        &tile.texture
    }
}

/// Decode an image in RGBA format from PNG data.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    let dynamic_image =
        image::io::Reader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png).decode()?;
    Ok(dynamic_image.to_rgba8())
}

/// Prints a warning message based on the error if the given result is not OK.
pub fn warn_on_error<E: std::fmt::Debug>(x: Result<(), E>, msg: &str) {
    match x {
        Ok(()) => {}
        Err(e) => warn!("Failed to send {}: {:?}", msg, e),
    }
}
