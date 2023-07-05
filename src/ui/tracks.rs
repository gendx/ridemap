//! Module to manage GPS tracks on the UI thread.

use super::camera::Camera;
use super::util::Color;
use crate::tracks::polyline::Point;
use crate::tracks::schema::ActivityType;
use log::debug;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Polyline with an associated color.
struct ColoredPolyline {
    /// Geometric shape of this polyline, in Mercator coordinates.
    points: Vec<Point<f64>>,
    /// Color attributed to this polyline.
    color: Rc<Cell<Color>>,
    /// Strava activity type associated with this polyline.
    r#type: ActivityType,
}

/// A polyline scaled to the current zoom level.
struct ZoomedPolyline {
    /// Geometric shape of this polyline, in world pixel coordinates.
    points: Vec<Point<i32>>,
    /// Bounding box of this polyline.
    bbox: Option<BBox>,
    /// Color attributed to this polyline.
    color: Rc<Cell<Color>>,
    /// Color of the activity type associated with this polyline.
    type_color: Rc<Cell<Color>>,
}

impl ZoomedPolyline {
    /// Derives a zoomed polyline from the given [`ColoredPolyline`] and zoom
    /// level.
    fn new(
        poly: &ColoredPolyline,
        zoom: f64,
        type_colors: &mut HashMap<ActivityType, Rc<Cell<Color>>>,
    ) -> Self {
        let mut points: Vec<Point<i32>> = poly
            .points
            .iter()
            .map(|p| Point {
                x: (p.x * zoom) as i32,
                y: (p.y * zoom) as i32,
            })
            .collect();
        points.dedup();

        let mut bbox = None;
        for &p in &points {
            match &mut bbox {
                None => bbox = Some(BBox::new(p)),
                Some(b) => b.update(p),
            }
        }

        let type_color = type_colors
            .entry(poly.r#type)
            .or_insert_with(|| Rc::new(Cell::new(Color::new_random())));

        ZoomedPolyline {
            points,
            bbox,
            color: poly.color.clone(),
            type_color: type_color.clone(),
        }
    }

    /// Checks whether the rectangle defined by the offset and window size
    /// intersects with this polyline's bounding box.
    fn visible(&self, offset: Point<i32>, wsize: Point<i32>) -> bool {
        self.bbox
            .as_ref()
            .map_or(false, |bbox| bbox.visible(offset, wsize))
    }
}

/// A polyline scaled to the current zoom level, with additional filtering of
/// invisible segments.
pub struct VisiblePolyline<'a> {
    /// Geometric shape of this polyline, in world pixel coordinates.
    points: &'a [Point<i32>],
    /// Color attributed to this polyline.
    pub color: Color,
    /// Window size.
    iwsize: Point<i32>,
    /// Camera offset.
    ioffset: Point<i32>,
}

impl VisiblePolyline<'_> {
    /// Returns the first point of the polyline, if it is not empty.
    pub fn first_point(&self) -> Option<Point<i32>> {
        self.points.first().map(|p| self.convert(p))
    }

    /// Returns the last point of the polyline, if it is not empty.
    pub fn last_point(&self) -> Option<Point<i32>> {
        self.points.last().map(|p| self.convert(p))
    }

    /// Returns the number of segments in the polyline.
    pub fn segments_count(&self) -> usize {
        let points_count = self.points.len();
        if points_count >= 2 {
            points_count - 1
        } else {
            0
        }
    }

    /// Returns an iterator over the visible segments of the polyline.
    pub fn segments(&self) -> impl Iterator<Item = (usize, Point<i32>, Point<i32>)> + '_ {
        self.points
            .windows(2)
            .enumerate()
            .filter_map(|(i, segment)| {
                let p0 = self.convert(&segment[0]);
                let p1 = self.convert(&segment[1]);

                // Filter out invisible segments.
                if (p0.x < 0 && p1.x < 0)
                    || (p0.y < 0 && p1.y < 0)
                    || (p0.x > self.iwsize.x && p1.x > self.iwsize.x)
                    || (p0.y > self.iwsize.y && p1.y > self.iwsize.y)
                {
                    None
                } else {
                    Some((i, p0, p1))
                }
            })
    }

    /// Converts a point from world pixel coordinates to window pixel
    /// coordinates.
    fn convert(&self, point: &Point<i32>) -> Point<i32> {
        Point {
            x: self.ioffset.x + point.x,
            y: self.ioffset.y + point.y,
        }
    }
}

/// A bounding box for a set of points.
struct BBox {
    min: Point<i32>,
    max: Point<i32>,
}

impl BBox {
    /// Creates a new bounding box enclosing the given point.
    fn new(p: Point<i32>) -> Self {
        Self { min: p, max: p }
    }

    /// Extends the bounding box to enclose the given point.
    fn update(&mut self, p: Point<i32>) {
        self.min.x = std::cmp::min(self.min.x, p.x);
        self.min.y = std::cmp::min(self.min.y, p.y);
        self.max.x = std::cmp::max(self.max.x, p.x);
        self.max.y = std::cmp::max(self.max.y, p.y);
    }

    /// Checks whether the bounding box is visible based on the given pixel
    /// offset and window size.
    fn visible(&self, offset: Point<i32>, wsize: Point<i32>) -> bool {
        self.max.x + offset.x >= 0
            && self.max.y + offset.y >= 0
            && self.min.x + offset.x < wsize.x
            && self.min.y + offset.y < wsize.y
    }
}

/// State on the UI thread to manage GPS tracks.
pub struct TrackState {
    /// Colored polylines loaded on the UI thread.
    polylines: Vec<ColoredPolyline>,
    /// Mapping from activity types to colors.
    type_colors: HashMap<ActivityType, Rc<Cell<Color>>>,
    /// Polylines scaled to the current zoom level.
    zoomed_polylines: Vec<ZoomedPolyline>,
    /// Whether to choose the color based on the activity type.
    color_by_type: bool,
}

#[allow(clippy::new_without_default)]
impl TrackState {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self {
            polylines: Vec::new(),
            type_colors: HashMap::new(),
            zoomed_polylines: Vec::new(),
            color_by_type: false,
        }
    }

    /// Returns the number of polylines loaded in this state.
    pub fn polylines_count(&self) -> usize {
        self.zoomed_polylines.len()
    }

    /// Toggles whether tracks should be displayed based on their own color or
    /// activity type.
    pub fn toggle_color_by_type(&mut self) {
        self.color_by_type = !self.color_by_type;
    }

    /// Re-generate random colors of either the tracks or activity types, based
    /// on the `color_by_type` state.
    pub fn randomize_colors(&mut self) {
        if self.color_by_type {
            for color in self.type_colors.values_mut() {
                color.set(Color::new_random());
            }
        } else {
            for poly in &mut self.polylines {
                poly.color.set(Color::new_random());
            }
        }
    }

    /// Re-generate the zoomed polylines based on the given camera view.
    pub fn refresh_zoom(&mut self, camera: &Camera) {
        self.zoomed_polylines = self
            .polylines
            .iter()
            .map(|poly| ZoomedPolyline::new(poly, camera.zoom(), &mut self.type_colors))
            .collect();
    }

    /// Processes the given activity sent by the background thread.
    pub fn process_activity(
        &mut self,
        r#type: ActivityType,
        points: Vec<Point<f64>>,
        camera: &Camera,
    ) {
        let poly = ColoredPolyline {
            points,
            r#type,
            color: Rc::new(Cell::new(Color::new_random())),
        };
        self.zoomed_polylines.push(ZoomedPolyline::new(
            &poly,
            camera.zoom(),
            &mut self.type_colors,
        ));
        self.polylines.push(poly);
    }

    /// Returns an iterator over the visible polylines, based on the given
    /// camera position.
    pub fn visible_polylines(&self, camera: &Camera) -> impl Iterator<Item = VisiblePolyline<'_>> {
        let iwsize = camera.iwsize();
        let ioffset = camera.ioffset();
        self.zoomed_polylines
            .iter()
            .filter(move |poly| poly.visible(ioffset, iwsize))
            .map(move |poly| {
                let color = if self.color_by_type {
                    poly.type_color.get()
                } else {
                    poly.color.get()
                };
                VisiblePolyline {
                    points: poly.points.as_slice(),
                    color,
                    iwsize,
                    ioffset,
                }
            })
    }

    /// Returns debugging statistics based on the given camera position.
    pub fn debug_statistics(&self, camera: &Camera) -> TrackStats {
        let iwsize = camera.iwsize();
        let ioffset = camera.ioffset();

        let visible_count = self
            .zoomed_polylines
            .iter()
            .filter(|poly| poly.visible(ioffset, iwsize))
            .count();
        debug!(
            "BBox deduplication: {} / {} polylines visible",
            visible_count,
            self.zoomed_polylines.len()
        );

        let total_points: usize = self.polylines.iter().map(|p| p.points.len()).sum();
        let deduped_points: usize = self.zoomed_polylines.iter().map(|p| p.points.len()).sum();
        let visible_points: usize = self
            .zoomed_polylines
            .iter()
            .filter_map(|p| {
                if p.visible(ioffset, iwsize) {
                    Some(p.points.len())
                } else {
                    None
                }
            })
            .sum();
        debug!(
            "Deduped {} / {} / {} points",
            visible_points, deduped_points, total_points
        );

        TrackStats {
            total_points,
            deduped_points,
            visible_points,
        }
    }
}

/// Debugging statistics about the tracks currently displayed.
pub struct TrackStats {
    /// Total number of points loaded in all the polylines of the
    /// [`TrackState`].
    pub total_points: usize,
    /// Number of unique points in the [`TrackState`] after zooming and
    /// de-duplication.
    pub deduped_points: usize,
    /// Number of points visible in the UI window after zooming and
    /// de-duplication.
    pub visible_points: usize,
}
