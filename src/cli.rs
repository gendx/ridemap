//! Command-line interface.

use crate::config::MapProvider;
use crate::tracks::schema::ActivityType;
use crate::tracks::strava::StravaConfig;
use clap::{Parser, Subcommand};

/// Map your rides.
#[derive(Parser, Debug)]
#[command(name = "Ridemap")]
#[command(version)]
#[command(author)]
#[command(about = "Map your rides", long_about = None)]
pub struct Cli {
    /// Sub-command to load tracks.
    #[command(subcommand)]
    pub track_params: Option<TrackParams>,

    /// JSON file containing the map provider configuration.
    #[arg(long = "map-config", value_parser = clap::value_parser!(MapProvider))]
    pub map_provider: MapProvider,

    /// Path of the cache directory.
    #[arg(long, short = 'c')]
    pub cache_directory: Option<String>,

    /// Refresh UI only when graphics change, instead of on each frame.
    #[arg(long)]
    pub lazy_ui_refresh: bool,

    /// Speculatively load map tiles in the direction of movement.
    #[arg(long)]
    pub speculative_tile_load: bool,

    /// Run the UI loop in a background thread.
    #[arg(long)]
    pub background_ui_thread: bool,

    /// Maximum number of requests to send in parallel to a server.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub parallel_requests: u32,

    /// Maximum number of pixels a tile should be scaled to, before switching to
    /// the next zoom level.
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(100..=10000))]
    pub max_pixels_per_tile: u32,

    /// Maximum zoom level to fetch tiles for.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(i32).range(0..=20))]
    pub max_tile_level: i32,
}

/// Parameters to load tracks.
#[derive(Subcommand, Debug)]
pub enum TrackParams {
    /// Fetch activities from Strava.
    Strava(StravaParams),

    /// Fetch activities from GPX file(s).
    Gpx(GpxParams),

    /// Fetch activities from GeoJSON file(s).
    Geojson(GeoJsonParams),
}

/// Parameters to load Strava activities.
#[derive(Parser, Debug)]
pub struct StravaParams {
    /// JSON file containing the Strava API configuration.
    #[arg(long, value_parser = clap::value_parser!(StravaConfig))]
    pub strava_config: StravaConfig,

    /// TCP port to listen to when exchanging a Strava OAuth token.
    #[arg(long = "port", short = 'p', default_value_t = 8080)]
    pub authorize_redirect_port: u16,

    /// Number of activities to list per page.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
    pub activities_per_page: u32,

    /// Number of activity pages to list.
    #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub activity_pages: u32,

    /// Total number of activities to show on the map.
    #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u32).range(0..=10000))]
    pub activity_count: u32,

    /// Activity(ies) to display.
    #[arg(long, value_delimiter = ',', value_enum)]
    pub activity_types: Vec<ActivityType>,
}

/// Parameters to load GPX files.
#[derive(Parser, Debug)]
pub struct GpxParams {
    /// GPX file(s) to read.
    #[arg(long = "file", short = 'f', required = true, value_delimiter = ',')]
    pub files: Vec<String>,
}

/// Parameters to load GeoJSON files.
#[derive(Parser, Debug)]
pub struct GeoJsonParams {
    /// GeoJSON file(s) to read.
    #[arg(long = "file", short = 'f', required = true, value_delimiter = ',')]
    pub files: Vec<String>,
}
