//! Module to render the map on the user interface.

mod camera;
mod tiles;
mod tracks;
pub mod util;
pub mod window;

use crate::map::tiles::TileIndex;
use crate::tracks::polyline::Point;
use crate::tracks::schema::ActivityType;
use image::RgbaImage;

/// Message sent from the background thread to the UI.
pub enum UiMessage {
    /// GPS track to display on the UI.
    Activity {
        /// Index of this activity (counter among all the activities requested
        /// by the program).
        id: usize,
        /// Strava activity type.
        r#type: ActivityType,
        /// Series of points on this activity, in Mercator coordinates.
        points: Vec<Point<f64>>,
    },
    /// Tile of the background map.
    Tile {
        /// Position of this tile on the world map.
        index: TileIndex,
        /// Raw PNG bytes of this tile.
        png_image: Box<[u8]>,
        /// Decoded tile in RGBA format.
        rgba_image: RgbaImage,
    },
}
