//! A rectangular set of a tiles at a given zoom level.

use crate::map::tiles::TileIndex;
use crate::tracks::polyline::Point;
use log::trace;

/// A rectangular set of a tiles at a given zoom level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileBox {
    /// Zoom level.
    z: u32,
    /// Inclusive minimum of the box.
    min: Point<u32>,
    /// Exclusive maximum of the box.
    max: Point<u32>,
}

impl TileBox {
    /// The root tile box, containing the whole world at the lowest zoom level.
    pub fn root() -> Self {
        TileBox {
            z: 0,
            min: Point { x: 0, y: 0 },
            max: Point { x: 1, y: 1 },
        }
    }

    /// Checks that this tile box is valid, i.e. the min and max points are
    /// properly ordered, and within bounds of the zoom level.
    #[cfg(test)]
    fn is_valid(&self) -> bool {
        self.min.x < self.max.x
            && self.min.y < self.max.y
            && self.min.x >> self.z == 0
            && self.min.y >> self.z == 0
            && (self.max.x - 1) >> self.z == 0
            && (self.max.y - 1) >> self.z == 0
    }

    /// Counts the number of tiles within the box.
    fn len(&self) -> usize {
        (self.max.x - self.min.x) as usize * (self.max.y - self.min.y) as usize
    }

    /// Checks whether the given tile is contained in this tile box.
    pub fn contains(&self, index: &TileIndex) -> bool {
        self.z == index.z
            && self.min.x <= index.x
            && index.x < self.max.x
            && self.min.y <= index.y
            && index.y < self.max.y
    }

    /// Checks whether the given tile is contained in an ancestor of this tile
    /// box.
    pub fn is_ancestor(&self, index: &TileIndex) -> Option<u32> {
        if index.z < self.z {
            let shift = self.z - index.z;
            if self.ancestor(shift).unwrap().contains(index) {
                return Some(shift);
            }
        }
        None
    }

    /// Checks whether the given tile is contained in or is an immediate
    /// neighbor of this tile box.
    pub fn is_neighbor(&self, index: &TileIndex) -> bool {
        self.z == index.z
            && self.min.x <= index.x + 1
            && index.x < self.max.x + 1
            && self.min.y <= index.y + 1
            && index.y < self.max.y + 1
    }

    /// Returns the smallest tile box at the lower zoom level that fully
    /// contains this box, or [`None`] if this box is at the lowest zoom
    /// level.
    pub fn parent(&self) -> Option<Self> {
        if self.z == 0 {
            None
        } else {
            Some(TileBox {
                z: self.z - 1,
                min: Point {
                    x: self.min.x >> 1,
                    y: self.min.y >> 1,
                },
                max: Point {
                    x: (self.max.x + 1) >> 1,
                    y: (self.max.y + 1) >> 1,
                },
            })
        }
    }

    /// Returns the ancestor of this tile box at the given zoom level `n`, or
    /// [`None`] if this box is at a zoom level lower than `n`.
    fn ancestor(&self, n: u32) -> Option<Self> {
        if self.z < n {
            None
        } else {
            Some(TileBox {
                z: self.z - n,
                min: Point {
                    x: self.min.x >> n,
                    y: self.min.y >> n,
                },
                max: Point {
                    x: ((self.max.x - 1) >> n) + 1,
                    y: ((self.max.y - 1) >> n) + 1,
                },
            })
        }
    }

    /// Returns all the tiles contained in this box, at the current zoom level.
    pub fn tile_indices(&self) -> Vec<TileIndex> {
        let mut result = Vec::with_capacity(self.len());
        for x in self.min.x..self.max.x {
            for y in self.min.y..self.max.y {
                result.push(TileIndex { z: self.z, x, y });
            }
        }

        result
    }

    /// Returns all the tiles immediately left of this box, or an empty set if
    /// this box is at the left edge.
    pub fn left(&self) -> Vec<TileIndex> {
        if self.min.x > 0 {
            let z = self.z;
            let x = self.min.x - 1;
            (self.min.y..self.max.y)
                .map(|y| TileIndex { z, x, y })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns all the tiles immediately right of this box, or an empty set if
    /// this box is at the right edge.
    pub fn right(&self) -> Vec<TileIndex> {
        if self.max.x >> self.z == 0 {
            let z = self.z;
            let x = self.max.x;
            (self.min.y..self.max.y)
                .map(|y| TileIndex { z, x, y })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns all the tiles immediately above this box, or an empty set if
    /// this box is at the top edge.
    pub fn top(&self) -> Vec<TileIndex> {
        if self.min.y > 0 {
            let z = self.z;
            let y = self.min.y - 1;
            (self.min.x..self.max.x)
                .map(|x| TileIndex { z, x, y })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns all the tiles immediately below this box, or an empty set if
    /// this box is at the bottom edge.
    pub fn bottom(&self) -> Vec<TileIndex> {
        if self.max.y >> self.z == 0 {
            let z = self.z;
            let y = self.max.y;
            (self.min.x..self.max.x)
                .map(|x| TileIndex { z, x, y })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns the smallest tile box containing the given UI window. The box's
    /// zoom level is chosen such that each tile is displayed with at most
    /// `max_pixels_per_tile` pixels per tile based on the UI's zoom,
    /// clamped to the `max_tile_level` supported by the tile provider service.
    pub fn new(
        width: f64,
        height: f64,
        zoom: f64,
        ioffset: Point<i32>,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
    ) -> Self {
        let mut ideal_level = -((max_pixels_per_tile as f64) / zoom).log2().floor() as i32;

        if ideal_level < 0 {
            trace!("Clamping negative level {ideal_level} => 0");
            ideal_level = 0;
        } else if ideal_level > max_tile_level {
            trace!("Clamping too large level {ideal_level} => {max_tile_level}");
            ideal_level = max_tile_level;
        }

        trace!("Ideal tile level = {ideal_level} (zoom = {zoom})");
        let pixels_per_tile = 0.5_f64.powi(ideal_level) * zoom;
        trace!("Pixels per tile at level {ideal_level} = {pixels_per_tile}");

        let ideal_factor = 0.5_f64.powi(ideal_level);
        let ideal_zoom = ideal_factor * zoom;
        let ideal_max = 1 << ideal_level;

        let min = Point {
            x: (-ioffset.x as f64) / ideal_zoom,
            y: (-ioffset.y as f64) / ideal_zoom,
        };
        let max = Point {
            x: (width - ioffset.x as f64) / ideal_zoom,
            y: (height - ioffset.y as f64) / ideal_zoom,
        };

        trace!("Window is [{}, {}, {}, {}]", min.x, min.y, max.x, max.y);

        let imin = Point {
            x: min.x.floor() as i32,
            y: min.y.floor() as i32,
        };
        let imax = Point {
            x: max.x.floor() as i32,
            y: max.y.floor() as i32,
        };

        trace!("Range is [{}, {}, {}, {}]", imin.x, imin.y, imax.x, imax.y);

        let result = if ideal_level < 0 {
            TileBox::root()
        } else {
            TileBox {
                z: ideal_level as u32,
                min: Point {
                    x: std::cmp::max(imin.x, 0) as u32,
                    y: std::cmp::max(imin.y, 0) as u32,
                },
                max: Point {
                    x: std::cmp::min(imax.x + 1, ideal_max) as u32,
                    y: std::cmp::min(imax.y + 1, ideal_max) as u32,
                },
            }
        };

        trace!("Ideal tiles: {result:#?}");
        trace!("Ideal tile count: {}", result.len());

        result
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn tile_box_root() {
        let root = TileBox::root();
        assert!(root.is_valid());
        assert_eq!(root.len(), 1);
        assert_eq!(root.parent(), None);
    }

    #[test]
    fn tile_box_len_one() {
        let z = 4;
        let max = 1 << z;
        for x in 0..max {
            for y in 0..max {
                let tile_box = TileBox {
                    z,
                    min: Point { x, y },
                    max: Point { x: x + 1, y: y + 1 },
                };

                assert!(tile_box.is_valid());
                assert_eq!(tile_box.len(), 1);
            }
        }
    }

    #[test]
    fn tile_box_len() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        assert_eq!(tile_box.len(), tile_box.tile_indices().len());
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_ancestors() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        let mut p = Some(tile_box);
                        for n in 0..=z {
                            assert_eq!(p, tile_box.ancestor(n));
                            let pp = p.unwrap();
                            assert!(pp.is_valid());
                            assert!(pp.len() <= tile_box.len());
                            p = pp.parent();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_contains() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        assert!(!tile_box.contains(&TileIndex { z: 0, x: 0, y: 0 }));
                        for x in minx..maxx {
                            for y in miny..maxy {
                                assert!(tile_box.contains(&TileIndex { z, x, y }));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_contains_itself() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        for index in tile_box.tile_indices() {
                            assert!(tile_box.contains(&index));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_parent_is_parents() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        let mut parents_of_list: Vec<TileIndex> = tile_box
                            .tile_indices()
                            .iter()
                            .map(|index| index.parent().unwrap())
                            .collect();
                        let mut list_of_parent: Vec<TileIndex> =
                            tile_box.parent().unwrap().tile_indices();
                        parents_of_list.sort();
                        parents_of_list.dedup();
                        list_of_parent.sort();
                        assert_eq!(parents_of_list, list_of_parent);
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_contains_its_parents() {
        let z = 4;
        let max = 1 << z;
        for maxx in 1..=max {
            for minx in 0..maxx {
                for maxy in 1..=max {
                    for miny in 0..maxy {
                        let tile_box = TileBox {
                            z,
                            min: Point { x: minx, y: miny },
                            max: Point { x: maxx, y: maxy },
                        };

                        for mut index in tile_box.tile_indices() {
                            assert!(tile_box.is_ancestor(&index).is_none());
                            while let Some(p) = index.parent() {
                                assert!(tile_box.is_ancestor(&p).is_some());
                                index = p;
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tile_box_contains_only_its_parents() {
        let z = 4;
        let max = 1 << z;
        for x in 0..max {
            for y in 0..max {
                let tile_box = TileBox {
                    z,
                    min: Point { x, y },
                    max: Point { x: x + 1, y: y + 1 },
                };

                let mut index = TileIndex { z, x, y };
                while let Some(p) = index.parent() {
                    assert!(tile_box.is_ancestor(&p).is_some());
                    if p.x + 1 < max {
                        assert!(tile_box
                            .is_ancestor(&TileIndex {
                                z: p.z,
                                x: p.x + 1,
                                y: p.y,
                            })
                            .is_none());
                    }
                    if p.y + 1 < max {
                        assert!(tile_box
                            .is_ancestor(&TileIndex {
                                z: p.z,
                                x: p.x,
                                y: p.y + 1,
                            })
                            .is_none());
                    }
                    if p.x > 0 {
                        assert!(tile_box
                            .is_ancestor(&TileIndex {
                                z: p.z,
                                x: p.x - 1,
                                y: p.y,
                            })
                            .is_none());
                    }
                    if p.y > 0 {
                        assert!(tile_box
                            .is_ancestor(&TileIndex {
                                z: p.z,
                                x: p.x,
                                y: p.y - 1,
                            })
                            .is_none());
                    }
                    index = p;
                }
            }
        }
    }
}
