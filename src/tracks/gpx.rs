//! Module to extract a GPS track from a GPX file.

use super::polyline::{LatLon, LatLonLine, ToMercator};
use super::schema::ActivityType;
use crate::ui::UiMessage;
use anyhow::Context;
use futures::{stream, StreamExt};
use log::{debug, error, trace};
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc;
use tokio::task;

/// Schema for a GPX file.
#[derive(Deserialize)]
struct Gpx {
    trk: GpxTrack,
}

/// Schema for a track in a GPX file.
#[derive(Deserialize)]
struct GpxTrack {
    trkseg: GpxTrackSegment,
}

/// Schema for a segment within a GPX track.
#[derive(Deserialize)]
struct GpxTrackSegment {
    trkpt: Vec<GpxTrackPoint>,
}

/// Schema for a track point within a GPX track segment.
#[derive(Deserialize)]
struct GpxTrackPoint {
    #[serde(rename = "@lat")]
    lat: f64,
    #[serde(rename = "@lon")]
    lon: f64,
    #[allow(dead_code)]
    #[serde(rename = "@ele")]
    ele: Option<f64>,
    #[allow(dead_code)]
    #[serde(rename = "@time")]
    time: Option<String>,
}

impl Gpx {
    /// Parses the given GPX file.
    fn read_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open GPX file: {}", path.display()))?;
        let reader = BufReader::new(file);
        let gpx: Self = serde_xml_rs::from_reader(reader)
            .with_context(|| format!("Failed to parse GPX file: {}", path.display()))?;

        Ok(gpx)
    }
}

/// New-type encapsulating a latitude-longitude line found in a GPX track.
#[derive(Debug)]
struct Track {
    line: LatLonLine,
}

impl From<Gpx> for Track {
    fn from(gpx: Gpx) -> Self {
        let coords = gpx
            .trk
            .trkseg
            .trkpt
            .iter()
            .map(|point| LatLon {
                lat: point.lat,
                lon: point.lon,
            })
            .collect();
        Track {
            line: LatLonLine::new(coords),
        }
    }
}

/// Parses the tracks contained in the given GPX file (paths), and sends the
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
        .map(|(i, path)| async move { get_track(path.clone(), i).await.map(|t| (i, t)) })
        .buffer_unordered(parallel_requests);

    tracks
        .for_each(|track| async {
            match track {
                Ok((i, track)) => {
                    trace!("Track = {:#?}", track);
                    debug!("Polyline has {} points", track.line.len());
                    tx.send(UiMessage::Activity {
                        id: i,
                        // TODO: Track type?
                        r#type: ActivityType::Ride,
                        points: track.line.mercator_points(),
                    })
                    .unwrap();
                }
                Err(e) => error!("Got an error: {e}"),
            }
        })
        .await;

    Ok(())
}

/// Reads and parses the track contained in the given GPX file.
async fn get_track(path: String, i: usize) -> anyhow::Result<Track> {
    debug!("Get track {i}");
    let path2 = path.clone();
    task::spawn_blocking(move || Gpx::read_from_file(path).map(Track::from))
        .await
        .with_context(|| format!("Failed to join background task to get GPX track: {path2}"))?
}
