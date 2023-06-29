//! Module to manage polylines, and convert them between latitude-longitude
//! coordinates and Mercator's projection.

use std::str::Bytes;

/// Data structure representing a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point<T> {
    /// X coordinate.
    pub x: T,
    /// Y coordinate.
    pub y: T,
}

/// Data structure representing a latitude-longitude coordinate.
#[derive(Clone, Copy, Debug)]
pub struct LatLon {
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
}

impl LatLon {
    /// Converts the coordinates into Mercator's projection.
    fn as_mercator(&self) -> Point<f64> {
        let x = 0.5 + self.lon / 360.0;
        let s = (self.lat * std::f64::consts::PI / 180.0).tan().asinh();
        let y = 0.5 - s / (2.0 * std::f64::consts::PI);

        Point { x, y }
    }
}

/// A trait to convert a polyline into Mercator's projection.
pub trait ToMercator {
    /// Returns the number of points on the polyline.
    fn len(&self) -> usize;
    /// Checks whether the polyline contains any point.
    fn is_empty(&self) -> bool;
    /// Returns the points converted into Mercator coordinates.
    fn mercator_points(&self) -> Vec<Point<f64>>;
}

/// A polyline made of latitude-longitude coordinates.
#[derive(Clone, Debug)]
pub struct LatLonLine {
    coords: Vec<LatLon>,
}

impl LatLonLine {
    /// Creates a new polyline with the given coordinates.
    pub fn new(coords: Vec<LatLon>) -> Self {
        LatLonLine { coords }
    }
}

impl ToMercator for LatLonLine {
    fn len(&self) -> usize {
        self.coords.len()
    }

    fn is_empty(&self) -> bool {
        self.coords.is_empty()
    }

    fn mercator_points(&self) -> Vec<Point<f64>> {
        self.coords.iter().map(LatLon::as_mercator).collect()
    }
}

/// A polyline encoded as a starting point followed by relative increments, all
/// in scaled latitude-longitude coordinates.
///
/// See [Google's polyline
/// algorithm](https://developers.google.com/maps/documentation/utilities/polylinealgorithm).
#[derive(Clone, Debug)]
pub struct Polyline {
    points: Vec<Point<i32>>,
}

impl ToMercator for Polyline {
    fn len(&self) -> usize {
        self.points.len()
    }

    fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    fn mercator_points(&self) -> Vec<Point<f64>> {
        let mut it = self.points.iter();
        let mut result = Vec::with_capacity(self.points.len());

        if let Some(mut cursor) = it.next().copied() {
            result.push(Polyline::point_to_mercator(cursor));

            for p in it {
                cursor.x += p.x;
                cursor.y += p.y;
                result.push(Polyline::point_to_mercator(cursor));
            }
        }

        result
    }
}

impl Polyline {
    /// Decodes a polyline encoded with [Google's
    /// algorithm](https://developers.google.com/maps/documentation/utilities/polylinealgorithm).
    pub fn new(encoded: &str) -> Option<Self> {
        let mut bytes = encoded.bytes();
        let mut points = Vec::new();

        while let Some(x) = Polyline::get_value(&mut bytes) {
            let y = Polyline::get_value(&mut bytes)?;
            points.push(Point { x, y });
        }

        Some(Polyline { points })
    }

    /// Converts an encoded point into Mercator's coordinates.
    fn point_to_mercator(p: Point<i32>) -> Point<f64> {
        LatLon {
            lat: p.x as f64 / 1e5,
            lon: p.y as f64 / 1e5,
        }
        .as_mercator()
    }

    /// Reads an encoded signed value.
    fn get_value(bytes: &mut Bytes) -> Option<i32> {
        let mut x = Polyline::get_raw_value(bytes)? as i32;
        if x & 1 == 1 {
            x = !x;
        }
        Some(x >> 1)
    }

    /// Reads an encoded unsigned value.
    fn get_raw_value(bytes: &mut Bytes) -> Option<u32> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            let x = Polyline::get_base64_digit(bytes)?;
            result |= (x & 0x1F) << shift;
            if x & 0x20 == 0 {
                return Some(result);
            }
            shift += 5;
        }
    }

    /// Reads a base-64 digit.
    fn get_base64_digit(bytes: &mut Bytes) -> Option<u32> {
        let x = bytes.next()? - 63;
        if x < 64 {
            Some(x as u32)
        } else {
            None
        }
    }
}
