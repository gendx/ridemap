//! Module to manage map tiles on the UI thread.

use super::camera::Camera;
use super::util::warn_on_error;
use super::util::Tile;
use crate::caching::lru::Lru;
use crate::map::tile_box::TileBox;
use crate::map::tile_channel::TileRequestSender;
use crate::map::tiles::TileIndex;
use image::RgbaImage;
use log::{debug, trace};
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

/// State on the UI thread to manage map tiles.
pub struct TileState<Image> {
    /// Channel to request tiles from the background thread.
    tiles_tx: TileRequestSender,
    /// LRU cache of tiles loaded in memory and ready to use on the UI thread.
    tiles: Lru<TileIndex, Tile<Image>>,
    /// Box of tiles currently visible in the UI window.
    tile_box: TileBox,
    /// Whether to speculatively load tiles based on the mouse movements.
    speculative_tile_load: bool,
    /// Maximum number of pixels that a tile is allowed to be zoomed to.
    ///
    /// Beyond this limit, tiles should be displayed at the next zoom level,
    /// unless that would exceed the maximum tile level.
    max_pixels_per_tile: usize,
    /// Maximum zoom level to load tiles at.
    max_tile_level: i32,
    /// Iteration counter of the UI window, for debug purposes only.
    iteration: Rc<Cell<usize>>,
}

impl<Image> TileState<Image> {
    /// Capacity of the in-memory LRU cache of tiles.
    const LRU_CAPACITY: usize = 200;

    /// Creates a new tile state based on the given parameters.
    ///
    /// This doesn't trigger any request, see the [`Self::start()`] function.
    pub fn new(
        tiles_tx: TileRequestSender,
        speculative_tile_load: bool,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
        iteration: Rc<Cell<usize>>,
    ) -> Self {
        Self {
            tiles_tx,
            tiles: Lru::with_capacity(Self::LRU_CAPACITY),
            tile_box: TileBox::root(),
            speculative_tile_load,
            max_pixels_per_tile,
            max_tile_level,
            iteration,
        }
    }

    /// Starts requesting tiles, i.e. request the root tile.
    pub fn start(&self) {
        self.request_tiles(Some(self.get_request_tiles()), Box::new([]));
    }

    /// Stops the processing, closing the tile request channel to the background
    /// thread.
    pub fn stop(&self) {
        self.close_tile_request();
    }

    /// Updates the state based on the given camera position and mouse
    /// direction.
    pub fn update(
        &mut self,
        camera: &mut Camera,
        x_dir: Ordering,
        y_dir: Ordering,
        z_dir: Ordering,
    ) {
        let new_tile_box = camera.refresh(self.max_pixels_per_tile, self.max_tile_level);

        // Request tiles.
        let tiles = if self.tile_box != new_tile_box {
            debug!(
                "[{}] New tile box: {:?}",
                self.iteration.get(),
                new_tile_box
            );
            self.tile_box = new_tile_box;
            Some(self.get_request_tiles())
        } else {
            None
        };

        let speculative = if self.speculative_tile_load {
            self.get_speculate_tiles(x_dir, y_dir, z_dir)
        } else {
            Box::new([])
        };

        self.request_tiles(tiles, speculative);
    }

    /// Processes the given tile sent by the background thread, returning `true`
    /// if the tile was successfully inserted in the LRU cache.
    ///
    /// Insertion can fail in either of the following cases.
    /// * The tile couldn't be loaded as a UI texture.
    /// * The LRU cache is full, and this tile has lower priority than any other
    ///   tile in the cache.
    pub fn process_tile(
        &mut self,
        index: TileIndex,
        png_image: Box<[u8]>,
        rgba_image: RgbaImage,
        create_image: impl FnOnce(RgbaImage) -> Option<Image>,
    ) -> bool {
        debug!(
            "[{}] Received tile {:?} = {} bytes",
            self.iteration.get(),
            index,
            png_image.len()
        );

        let (inserted, evicted) = self.tiles.or_insert_with(
            index,
            |idx| {
                // Priority order of tiles for LRU cache.
                if self.tile_box.contains(idx) {
                    // Visible tiles.
                    0
                } else if idx.z == 0 {
                    // The root tile.
                    1
                } else if let Some(level) = self.tile_box.is_ancestor(idx) {
                    // Ancestor tiles, by level distance.
                    level as usize + 1
                } else if self.tile_box.is_neighbor(idx) {
                    // Neighbor tiles.
                    self.max_tile_level as usize + 1
                } else {
                    // Other tiles.
                    self.max_tile_level as usize + 2
                }
            },
            || create_image(rgba_image).map(|image| Tile { image, png_image }),
        );

        if let Some(evicted) = evicted {
            self.evict_tile(evicted);
        }

        inserted
    }

    /// Returns the current set of tiles to draw, based on the camera position
    /// and available tiles.
    ///
    /// If a tile is not available at the current zoom level, one of its
    /// ancestors is returned from the cache instead.
    ///
    /// The returned tiles are sorted from small to large zoom level (so that
    /// tiles with a larger zoom level will appear on top if tiles are
    /// rendered in order).
    pub fn tiles_to_draw(&self) -> Vec<(TileIndex, &Tile<Image>)> {
        let mut tiles_to_draw: HashMap<TileIndex, &Tile<Image>> = HashMap::new();
        for mut index in self.tile_box.tile_indices() {
            loop {
                if let Some(tile) = self.tiles.get(&index) {
                    tiles_to_draw.insert(index, tile);
                    break;
                }
                match index.parent() {
                    Some(p) => index = p,
                    None => break,
                }
            }
        }

        let mut tiles_to_draw: Vec<(TileIndex, &Tile<Image>)> = tiles_to_draw.drain().collect();
        tiles_to_draw.sort_by(|a, b| a.0.cmp(&b.0));

        tiles_to_draw
    }

    /// Filters out tiles from the input list that are already contained in the
    /// LRU cache.
    fn filter_new_tiles(&self, mut tiles: Vec<TileIndex>) -> Box<[TileIndex]> {
        tiles.retain(|index| !self.tiles.contains_key(index));
        tiles.into_boxed_slice()
    }

    /// Returns the list of tiles in the current window box that are not yet
    /// contained in the LRU cache.
    fn get_request_tiles(&self) -> Box<[TileIndex]> {
        self.filter_new_tiles(self.tile_box.tile_indices())
    }

    /// Returns the list of tiles speculated based on the given mouse direction,
    /// filtering out those that are already contained in the LRU cache.
    fn get_speculate_tiles(
        &self,
        x_dir: Ordering,
        y_dir: Ordering,
        z_dir: Ordering,
    ) -> Box<[TileIndex]> {
        let mut all_tiles = Vec::new();
        match x_dir {
            Ordering::Less => {
                trace!("[{}] Speculate tiles on the right", self.iteration.get());
                all_tiles.append(&mut self.tile_box.right());
            }
            Ordering::Greater => {
                trace!("[{}] Speculate tiles on the left", self.iteration.get());
                all_tiles.append(&mut self.tile_box.left());
            }
            Ordering::Equal => (),
        };
        match y_dir {
            Ordering::Less => {
                trace!("[{}] Speculate tiles on the bottom", self.iteration.get());
                all_tiles.append(&mut self.tile_box.bottom());
            }
            Ordering::Greater => {
                trace!("[{}] Speculate tiles on the top", self.iteration.get());
                all_tiles.append(&mut self.tile_box.top());
            }
            Ordering::Equal => (),
        };
        match z_dir {
            Ordering::Less => {
                if let Some(p) = self.tile_box.parent() {
                    trace!("[{}] Speculate parent tiles", self.iteration.get());
                    all_tiles.append(&mut p.tile_indices());
                }
            }
            Ordering::Greater | Ordering::Equal => (),
        };
        self.filter_new_tiles(all_tiles)
    }

    /// Closes the channel requesting tiles to the background thread.
    fn close_tile_request(&self) {
        debug!("[{}] Closing TileRequestSender", self.iteration.get());
        Self::warn_on_tile_error(self.tiles_tx.close());
    }

    /// Sends a request to the background thread to fetch the given list of
    /// tiles.
    fn request_tiles(&self, tiles: Option<Box<[TileIndex]>>, speculative: Box<[TileIndex]>) {
        if let Some(ref tiles) = tiles {
            for tile in tiles.iter() {
                debug!("[{}] Request tile {:?}", self.iteration.get(), tile);
            }
        }
        for tile in speculative.iter() {
            debug!("[{}] Speculate tile {:?}", self.iteration.get(), tile);
        }
        Self::warn_on_tile_error(self.tiles_tx.request_tiles(tiles, speculative));
    }

    /// Indicates to the background thread that the given tile was evicted.
    fn evict_tile(&self, tile: TileIndex) {
        debug!("[{}] Evicted tile {:?}", self.iteration.get(), tile);
        Self::warn_on_tile_error(self.tiles_tx.evict_tile(tile));
    }

    /// Prints a warning message based on the error if the given result is not
    /// OK.
    fn warn_on_tile_error(x: anyhow::Result<()>) {
        warn_on_error(x, "BatchTileRequest");
    }
}
