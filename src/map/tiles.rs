//! Background service to request tiles from a map provider.

use super::tile_channel::{TileRequest, TileRequestReceiver};
use crate::caching::cache::Cache;
use crate::config::MapProvider;
use crate::ui::util::decode_png;
use crate::ui::UiMessage;
use anyhow::bail;
use anyhow::Context;
use futures::StreamExt;
use image::RgbaImage;
use log::{debug, error, info, trace, warn};
use reqwest::header::{REFERER, USER_AGENT};
use reqwest::{Client, StatusCode};
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// Index of a tile in Mercator coordinates.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TileIndex {
    /// Zoom level.
    pub z: u32,
    /// Index from West to East.
    pub x: u32,
    /// Index from North to South.
    pub y: u32,
}

impl TileIndex {
    /// Returns the (left, top, width, height) rectangle occupied by this tile
    /// w.r.t. a world square of length 1.0.
    pub fn rect(&self) -> [f64; 4] {
        let size = 0.5_f64.powi(self.z as i32);
        [self.x as f64 * size, self.y as f64 * size, size, size]
    }

    /// Returns the parent tile (one zoom level less), or `None` if this is the
    /// root tile.
    pub fn parent(&self) -> Option<TileIndex> {
        if self.z == 0 {
            None
        } else {
            Some(TileIndex {
                z: self.z - 1,
                x: self.x >> 1,
                y: self.y >> 1,
            })
        }
    }
}

/// Handle to access the tiles.
pub struct Tiles<'a> {
    /// Provider to fetch the tiles from (on the Internet).
    map_provider: &'a MapProvider,
    /// On-disk cache of tiles.
    cache: Option<&'a Cache>,
    /// Network client.
    client: &'a Client,
    /// Channel to send tiles to the UI thread.
    ui_tx: &'a Sender<UiMessage>,
    /// Set of currently requested tiles.
    requested: Mutex<HashSet<TileIndex>>,
}

impl<'a> Tiles<'a> {
    /// Creates a new handle to fetch tiles.
    pub fn new(
        map_provider: &'a MapProvider,
        cache: Option<&'a Cache>,
        client: &'a Client,
        ui_tx: &'a Sender<UiMessage>,
    ) -> Self {
        Self {
            map_provider,
            cache,
            client,
            ui_tx,
            requested: Mutex::new(HashSet::new()),
        }
    }

    /// Loop that fetches the tiles requested by the given
    /// [`TileRequestReceiver`].
    ///
    /// This loop terminates when the EOF message is sent through the tile
    /// request channel.
    pub async fn query_loop(
        &self,
        tiles_rx: TileRequestReceiver,
        parallel_requests: usize,
    ) -> anyhow::Result<()> {
        tiles_rx
            .into_stream()
            .map(|tile_request| async move { self.get_tile(tile_request).await })
            .buffer_unordered(parallel_requests)
            .for_each(|result| async {
                match result {
                    Ok(()) => (),
                    Err(e) => error!("Got an error: {e}"),
                }
            })
            .await;

        info!("End of Tiles::query_loop");
        Ok(())
    }

    /// Processes the given tile request.
    async fn get_tile(&self, tile_request: TileRequest) -> anyhow::Result<()> {
        match tile_request {
            TileRequest::Evicted(index) => {
                if !self.requested.lock().unwrap().remove(&index) {
                    warn!("Tile {index:?} was already evicted");
                }
            }
            TileRequest::Speculate(index) | TileRequest::Tile(index) => {
                // TODO: don't speculatively request on network.
                let is_new = self.requested.lock().unwrap().insert(index);
                if is_new {
                    debug!("New tile {:?}", index);
                    match self.get_tile_png(&index).await {
                        Ok((png_image, rgba_image)) => {
                            debug!("Sending tile {index:?} to UI = {} bytes", png_image.len());
                            self.ui_tx.send(UiMessage::Tile {
                                index,
                                png_image,
                                rgba_image,
                            })?;
                        }
                        Err(e) => {
                            error!("Requesting tile {index:?} returned an error: {e}");
                            // TODO: send error
                        }
                    }
                } else {
                    trace!("Tile {index:?} already requested");
                }
            }
            _ => error!("Unexpected tile request: {tile_request:?}"),
        }
        Ok(())
    }

    /// Fetches the given tile, and decodes it as a PNG image.
    async fn get_tile_png(&self, index: &TileIndex) -> anyhow::Result<(Box<[u8]>, RgbaImage)> {
        let bytes = self.get_tile_index(index).await?;
        debug!("Decoding tile {index:?} = {} bytes", bytes.len());
        let rgba_image = decode_png(bytes.as_ref())
            .with_context(|| format!("Failed to decode PNG data for tile: {index:?}"))?;
        Ok((bytes, rgba_image))
    }

    /// Fetches the given tile from the local cache or the network.
    async fn get_tile_index(&self, index: &TileIndex) -> anyhow::Result<Box<[u8]>> {
        if let Some(cache) = self.cache {
            let cached = cache.get_tile(index);
            if cached.is_ok() {
                debug!("Obtained tile {index:?} from cache");
                return cached;
            }
        }

        // TODO: only request once from server
        debug!("Requesting tile {index:?} from server");

        let url = format!(
            "https://{server}/{z}/{x}/{y}{extension}",
            server = self.map_provider.server,
            z = index.z,
            x = index.x,
            y = index.y,
            extension = self.map_provider.extension
        );

        let mut request = self.client.get(&url);
        if let Some(user_agent) = &self.map_provider.user_agent {
            request = request.header(USER_AGENT, user_agent);
        }
        if let Some(referer) = &self.map_provider.referer {
            request = request.header(REFERER, referer);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to request tile {index:?} from the server"))?;

        let status_code = response.status();
        if status_code != StatusCode::OK {
            error!("Tile server replied with status code {status_code}");
            bail!("Tile server replied with status code {status_code} for tile {index:?}");
        }

        let bytes = response.bytes().await?;
        if let Some(cache) = self.cache {
            if let Err(e) = cache.set_tile(index, bytes.as_ref()) {
                error!("Couldn't write tile {index:?} to cache: {e:?}");
            }
        }

        Ok(Box::from(bytes.as_ref()))
    }
}
