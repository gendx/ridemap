//! Module to extract GPS tracks from a GeoJSON file.

use super::polyline::{LatLon, LatLonLine, ToMercator};
use super::schema::ActivityType;
use crate::ui::UiMessage;
use anyhow::Context;
use futures::{stream, StreamExt};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, LineStringType, Value};
use log::{debug, error, trace};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc;
use tokio::task;

struct GeoJsonFile {
    inner: GeoJson,
}

impl GeoJsonFile {
    /// Parses the given GeoJSON file.
    fn read_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open GeoJSON file: {}", path.display()))?;
        let reader = BufReader::new(file);
        let geojson = geojson::GeoJson::from_reader(reader)
            .with_context(|| format!("Failed to parse GeoJSON file: {}", path.display()))?;

        Ok(Self { inner: geojson })
    }
}

/// New-type encapsulating a latitude-longitude line found in a GeoJSON object.
#[derive(Debug)]
struct Track {
    line: LatLonLine,
}

impl From<&LineStringType> for Track {
    fn from(line: &LineStringType) -> Self {
        let coords = line
            .iter()
            .map(|point| LatLon {
                lat: point[1],
                lon: point[0],
            })
            .collect();
        Track {
            line: LatLonLine::new(coords),
        }
    }
}

impl From<GeoJsonFile> for Vec<Track> {
    fn from(geo: GeoJsonFile) -> Self {
        fn visit_feature_collection(collection: &FeatureCollection, tracks: &mut Vec<Track>) {
            for feature in &collection.features {
                visit_feature(feature, tracks);
            }
        }

        fn visit_feature(feature: &Feature, tracks: &mut Vec<Track>) {
            if let Some(geometry) = &feature.geometry {
                visit_geometry(geometry, tracks);
            }
        }

        fn visit_geometry(geometry: &Geometry, tracks: &mut Vec<Track>) {
            match &geometry.value {
                Value::LineString(line) => {
                    tracks.push(Track::from(line));
                }
                Value::MultiLineString(lines) => {
                    for line in lines {
                        tracks.push(Track::from(line));
                    }
                }
                Value::Point(_)
                | Value::MultiPoint(_)
                | Value::Polygon(_)
                | Value::MultiPolygon(_) => (),
                Value::GeometryCollection(collection) => {
                    for geometry in collection {
                        visit_geometry(geometry, tracks);
                    }
                }
            }
        }

        let mut tracks = Vec::new();
        match &geo.inner {
            GeoJson::FeatureCollection(collection) => {
                visit_feature_collection(collection, &mut tracks)
            }
            GeoJson::Feature(feature) => visit_feature(feature, &mut tracks),
            GeoJson::Geometry(geometry) => visit_geometry(geometry, &mut tracks),
        }
        tracks
    }
}

/// Parses the tracks contained in the given GeoJSON file (paths), and sends the
/// results as UI messages to the given sending channel.
///
/// This reads up to `parallel_requests` files in parallel.
pub async fn get_tracks_parallel(
    tx: &mpsc::Sender<UiMessage>,
    files: &[String],
    parallel_requests: usize,
) -> anyhow::Result<()> {
    let tracks = stream::iter(files)
        .enumerate()
        .map(|(i, path)| async move { get_tracks(path.clone(), i).await.map(|t| (i, t)) })
        .buffer_unordered(parallel_requests);

    tracks
        .for_each(|tracks| async {
            match tracks {
                Ok((i, tracks)) => {
                    debug!("GeoJson has {} tracks", tracks.len());
                    for track in tracks {
                        trace!("Track = {track:#?}");
                        debug!("Polyline has {} points", track.line.len());
                        tx.send(UiMessage::Activity {
                            id: i,
                            // TODO: Track type?
                            r#type: ActivityType::Ride,
                            points: track.line.mercator_points(),
                        })
                        .unwrap();
                    }
                }
                Err(e) => error!("Got an error: {e}"),
            }
        })
        .await;

    Ok(())
}

/// Reads and parses the track contained in the given GPX file.
async fn get_tracks(path: String, i: usize) -> anyhow::Result<Vec<Track>> {
    debug!("Get tracks {i}");
    let path2 = path.clone();
    task::spawn_blocking(move || GeoJsonFile::read_from_file(path).map(Vec::<Track>::from))
        .await
        .with_context(|| format!("Failed to join background task to get GeoJSON track: {path2}"))?
}
