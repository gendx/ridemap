//! Ridemap - map your rides!

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod caching;
mod cli;
mod config;
mod map;
mod tracks;
mod ui;

use anyhow::Context;
use caching::cache::Cache;
use clap::Parser;
use cli::{Cli, StravaParams, TrackParams};
use config::MapProvider;
use futures::channel::oneshot;
use futures::future::FutureExt;
use futures::{future, join, select, StreamExt};
use log::{debug, error, info, warn};
use map::tile_channel::{tile_channel, TileRequestReceiver};
use map::tiles::Tiles;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use tokio::runtime::Runtime;
use tracks::gpx::get_tracks_parallel;
use tracks::strava::StravaClient;
use ui::window::Window;
use ui::UiMessage;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let Cli {
        track_params,
        map_provider,
        cache_directory,
        lazy_ui_refresh,
        speculative_tile_load,
        background_ui_thread,
        parallel_requests,
        max_pixels_per_tile,
        max_tile_level,
    } = Cli::parse();

    let cache: Option<Cache> = match &cache_directory {
        Some(dir) => match Cache::new(dir, &map_provider) {
            Ok(c) => Some(c),
            Err(e) => {
                error!("Couldn't create cache: {e:?}");
                None
            }
        },
        None => {
            warn!("No cache configured. You can set one up with --cache-directory.");
            None
        }
    };

    // Separate threads for GUI and network.
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let (ui_tx, ui_rx) = channel();
    let (tiles_tx, tiles_rx) = tile_channel();

    let network = thread::spawn(move || {
        // Create the Tokio runtime.
        let rt = Runtime::new().unwrap();

        // Spawn the root task.
        rt.block_on(async {
            select!(
                _ = cancel_rx.fuse() => Ok(()),
                res = tokio_loop(
                    ui_tx,
                    tiles_rx,
                    cache.as_ref(),
                    &map_provider,
                    track_params.as_ref(),
                    parallel_requests as usize,
                ).fuse() => res,
            )
        })
        .unwrap();
        info!("End of network thread");
    });

    if background_ui_thread {
        // This generally fails with the following error:
        //
        // thread '<unnamed>' panicked at 'Initializing the event loop outside of the
        // main thread is a significant cross-platform compatibility hazard. If you
        // really, absolutely need to create an EventLoop on a different thread, please
        // use the `EventLoopExtUnix::new_any_thread` function.'
        let ui = thread::spawn(move || {
            match Window::ui_loop(
                ui_rx,
                cancel_tx,
                tiles_tx,
                lazy_ui_refresh,
                speculative_tile_load,
                max_pixels_per_tile as usize,
                max_tile_level,
            ) {
                Ok(()) => info!("End of UI thread"),
                Err(e) => error!("Failed to run UI thread: {e:?}"),
            }
        });
        ui.join().unwrap();
    } else {
        match Window::ui_loop(
            ui_rx,
            cancel_tx,
            tiles_tx,
            lazy_ui_refresh,
            speculative_tile_load,
            max_pixels_per_tile as usize,
            max_tile_level,
        ) {
            Ok(()) => info!("End of UI thread"),
            Err(e) => error!("Failed to run UI thread: {e:?}"),
        }
    }

    network.join().unwrap();

    Ok(())
}

/// Asynchronous loop fetching data from the network (tiles, tracks) and sending
/// it to the UI thread via a channel.
///
/// This is invoked with a Tokio runtime in a background thread by the main
/// function.
async fn tokio_loop(
    ui_tx: Sender<UiMessage>,
    tiles_rx: TileRequestReceiver,
    cache: Option<&Cache>,
    map_provider: &MapProvider,
    track_params: Option<&TrackParams>,
    parallel_requests: usize,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    let tiles = Tiles::new(map_provider, cache, &client, &ui_tx);

    let (a, b) = join!(
        tiles.query_loop(tiles_rx, parallel_requests),
        fetch_tracks(&ui_tx, cache, &client, track_params, parallel_requests)
    );

    match (a, b) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}

/// Asynchronous function fetching tracks based on the given parameters, and
/// sending them to the UI thread.
async fn fetch_tracks(
    ui_tx: &Sender<UiMessage>,
    cache: Option<&Cache>,
    client: &reqwest::Client,
    track_params: Option<&TrackParams>,
    parallel_requests: usize,
) -> anyhow::Result<()> {
    match track_params {
        None => Ok(()),
        Some(TrackParams::Strava(strava_params)) => {
            fetch_strava_activities(ui_tx, cache, client, strava_params, parallel_requests).await
        }
        Some(TrackParams::Gpx(gpx_params)) => {
            get_tracks_parallel(ui_tx, &gpx_params.files, parallel_requests).await
        }
    }
}

/// Asynchronous function fetching Strava activities based on the given
/// parameters, and sending them to the UI thread.
async fn fetch_strava_activities(
    ui_tx: &Sender<UiMessage>,
    cache: Option<&Cache>,
    client: &reqwest::Client,
    strava_params: &StravaParams,
    parallel_requests: usize,
) -> anyhow::Result<()> {
    let strava = StravaClient::new(
        cache,
        client,
        &strava_params.strava_config,
        strava_params.authorize_redirect_port,
    )
    .await
    .context("Failed to initialize Strava client")?;

    // Show athlete summary.
    let athlete = strava
        .get_athlete()
        .await
        .context("Failed to get Strava athlete information")?;
    debug!("Athlete = {athlete:#?}");

    // List activities.
    let activity_stream = strava.get_activity_list(
        strava_params.activities_per_page as usize,
        strava_params.activity_pages as usize,
        parallel_requests,
    );

    let activities = activity_stream
        .filter(|a| {
            future::ready(
                strava_params.activity_types.is_empty()
                    || strava_params.activity_types.contains(&a.r#type),
            )
        })
        .take(strava_params.activity_count as usize);

    strava
        .get_detailed_activities_parallel(ui_tx, activities, parallel_requests)
        .await
        .context("Failed to fetch Strava activities")?;

    Ok(())
}
