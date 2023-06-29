//! 2D camera representing where the UI window is looking at.

use crate::map::tile_box::TileBox;
use crate::tracks::polyline::Point;
use std::cmp::Ordering;

/// 2D camera representing where the UI window is looking at.
pub struct Camera {
    /// Window width, in pixels.
    width: f64,
    /// Window height, in pixels.
    height: f64,
    /// Minimal zoom level that is acceptable, based on the window size.
    min_zoom: f64,
    /// Current zoom level, measured in pixels per Mercator unit.
    ///
    /// Under Mercator coordinates, the whole world is a unit square (i.e. of
    /// size 1.0 x 1.0).
    zoom: f64,
    /// Offset of the top-left corner of the world w.r.t the center of the
    /// window, in Mercator coordinates.
    offset: Point<f64>,
}

impl Camera {
    /// Maximum zoom level that is acceptable, in pixels per Mercator unit.
    const MAX_ZOOM: f64 = /* 2^25 */ 33_554_432.0;

    /// Returns a default camera view, based on the given window size.
    pub fn new(width: u32, height: u32) -> Self {
        let min_zoom = std::cmp::min(width, height) as f64;

        Self {
            width: width as f64,
            height: height as f64,
            min_zoom,
            zoom: min_zoom,
            offset: Point { x: -0.5, y: -0.5 },
        }
    }

    /// Returns the window width, in pixels.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// Returns the window height, in pixels.
    pub fn height(&self) -> f64 {
        self.height
    }

    /// Returns the zoom level, in pixels per Mercator unit.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Adjusts the camera based on the given new window size, and indicates
    /// whether a further refresh is required.
    pub fn resize(
        &mut self,
        width: f64,
        height: f64,
        need_zoom_refresh: &mut bool,
        need_offset_refresh: &mut bool,
    ) {
        self.width = width;
        self.height = height;
        self.min_zoom = self.width.min(self.height);
        if self.zoom < self.min_zoom {
            self.zoom = self.min_zoom;
            *need_zoom_refresh = true;
        } else {
            *need_offset_refresh = true;
        }
    }

    /// Adjusts the zoom level based on the given mouse scroll, indicating
    /// whether a further refresh is required, and in which direction along
    /// the Z axis this scroll was.
    pub fn scroll(&mut self, scroll: f64, need_zoom_refresh: &mut bool, z_dir: &mut Ordering) {
        *z_dir = scroll.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
        self.zoom *= (scroll / 10.0).exp2();
        if self.zoom < self.min_zoom {
            self.zoom = self.min_zoom;
            *z_dir = Ordering::Equal;
        }
        self.zoom = self.zoom.min(Self::MAX_ZOOM);
        *need_zoom_refresh = true;
    }

    /// Adjusts the offset based on the given mouse drag, indicating whether a
    /// further refresh is required, and in which direction along the X and
    /// Y axes this movement was.
    pub fn drag_relative(
        &mut self,
        dx: f64,
        dy: f64,
        need_offset_refresh: &mut bool,
        x_dir: &mut Ordering,
        y_dir: &mut Ordering,
    ) {
        *x_dir = dx.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
        *y_dir = dy.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
        self.offset.x += dx / self.zoom;
        self.offset.y += dy / self.zoom;
        *need_offset_refresh = true;
    }

    /// Adjusts the camera position based on the constraints (the world
    /// shouldn't go out of the window), and returns the visible tile box.
    pub fn refresh(&mut self, max_pixels_per_tile: usize, max_tile_level: i32) -> TileBox {
        // Offset (in Mercator coordinates) such that the top-left corner of the world
        // `(0.0, 0.0)` coincides with the top-left corner of the window.
        //
        // Proof: The point `(0.0, 0.0)` needs to be at `(-width/2, -height/2)` from the
        // center in pixels coordinates, which are scaled by a factor `zoom`
        // w.r.t. Mercator coordinates.
        let offset00 = Point {
            x: -self.width / (2.0 * self.zoom),
            y: -self.height / (2.0 * self.zoom),
        };

        // Offset (in Mercator coordinates) such that the bottom-right corner of the
        // world `(1.0, 1.0)` coincides with the bottom-right corner of the
        // window.
        //
        // Proof: The point `(1.0, 1.0)` needs to be at `(width/2, height/2)` from the
        // center in pixel coordinates, which are scaled by a factor `zoom`
        // w.r.t. Mercator coordinates. Once in Mercator coordinates, the point
        // `(0.0, 0.0)` is at `(-1.0, -1.0)` respective to `(1.0, 1.0)`.
        let offset11 = Point {
            x: self.width / (2.0 * self.zoom) - 1.0,
            y: self.height / (2.0 * self.zoom) - 1.0,
        };

        // For the x axis, allow the map border to be in the middle of the window.
        // - offset.x >= offset00.x || offset.x >= -1.0, i.e. either the left side of
        //   the world is within the window, or the right side of the world is in the
        //   right half of the window.
        self.offset.x = self.offset.x.max(offset00.x.min(-1.0));
        // - offset.x <= offset11.x || offset.x <= 0.0, i.e. either the right side of
        //   the world is within the window, or the left side of the world is in the
        //   left half of the window.
        self.offset.x = self.offset.x.min(offset11.x.max(0.0));
        // For the y axis, clamp to the window.
        // - offset.y >= min(offset00.y, offset11.y), i.e. either the world is "zoomed
        //   out" and the top side of the world must be within the window, or the world
        //   is "zoomed in" and the bottom side of the world must be outside of the
        //   window.
        self.offset.y = self.offset.y.max(offset00.y.min(offset11.y));
        // - offset.y <= max(offset00.y, offset11.y), i.e. either the world is "zoomed
        //   out" and the bottom side of the world must be within the window, or the
        //   world is "zoomed in" and the top side of the world must be outside of the
        //   window.
        self.offset.y = self.offset.y.min(offset00.y.max(offset11.y));

        // Update tile box.
        let ioffset = self.ioffset();
        TileBox::new(
            self.width,
            self.height,
            self.zoom,
            ioffset,
            max_pixels_per_tile,
            max_tile_level,
        )
    }

    /// Returns the window size.
    pub fn iwsize(&self) -> Point<i32> {
        Point {
            x: self.width as i32,
            y: self.height as i32,
        }
    }

    /// Returns the offset of the top-left corner of the world w.r.t. the
    /// top-left corner of the window, measured in pixels.
    pub fn ioffset(&self) -> Point<i32> {
        Point {
            x: (self.offset.x * self.zoom + self.width / 2.0) as i32,
            y: (self.offset.y * self.zoom + self.height / 2.0) as i32,
        }
    }
}
