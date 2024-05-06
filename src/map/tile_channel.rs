//! An asynchronous channel for the UI to request tiles to a background thread.

use super::tiles::TileIndex;
use anyhow::Context;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::future;
use futures::{stream, Stream, StreamExt};
use log::debug;
use std::sync::{Arc, RwLock};

/// Similar to [`TileRequest`], but multiple tiles can be requested in a batch.
#[derive(Clone, Debug)]
enum BatchTileRequest {
    /// Tile requested.
    Tiles(Box<[TileIndex]>),
    /// Notification that the UI evicted a tile.
    Evicted(TileIndex),
    /// End of stream.
    End,
}

/// Incoming request from the UI.
#[derive(Clone, Copy, Debug)]
pub enum TileRequest {
    /// Tile requested.
    Tile(TileIndex),
    /// Speculative request of a tile.
    Speculate(TileIndex),
    /// Notification that the UI evicted a tile.
    Evicted(TileIndex),
    /// End of stream.
    End,
}

/// List of currently visible and speculated tiles.
#[derive(Default)]
struct CurrentTiles {
    tiles: Box<[TileIndex]>,
    speculative: Box<[TileIndex]>,
}

/// Sending side of the channel, for the UI to request tiles.
pub struct TileRequestSender {
    tx: UnboundedSender<BatchTileRequest>,
    current: Arc<RwLock<CurrentTiles>>,
}

/// Receiving side of the channel, to process requests sent by the UI.
pub struct TileRequestReceiver {
    rx: UnboundedReceiver<BatchTileRequest>,
    current: Arc<RwLock<CurrentTiles>>,
}

/// Constructs a channel to communicate [`TileRequest`]s.
pub fn tile_channel() -> (TileRequestSender, TileRequestReceiver) {
    let (tx, rx) = unbounded();
    let current_tx = Arc::new(RwLock::new(CurrentTiles::default()));
    let current_rx = current_tx.clone();
    (
        TileRequestSender {
            tx,
            current: current_tx,
        },
        TileRequestReceiver {
            rx,
            current: current_rx,
        },
    )
}

impl TileRequestSender {
    /// Requests a set of tiles.
    ///
    /// This replaces the set of [`CurrentTiles`].
    pub fn request_tiles(
        &self,
        tiles: Option<Box<[TileIndex]>>,
        speculative: Box<[TileIndex]>,
    ) -> anyhow::Result<()> {
        // Replaces the current set of tiles.
        let mut current = self.current.write().unwrap();
        if let Some(ref tiles) = tiles {
            current.tiles.clone_from(tiles);
        }
        current.speculative.clone_from(&speculative);

        // Send the visible tiles in priority.
        if let Some(tiles) = tiles {
            self.send(BatchTileRequest::Tiles(tiles))?;
        }

        // Then send the speculated tiles.
        self.send(BatchTileRequest::Tiles(speculative))
    }

    /// Notifies that a tile is evicted.
    pub fn evict_tile(&self, tile: TileIndex) -> anyhow::Result<()> {
        self.send(BatchTileRequest::Evicted(tile))
    }

    /// Notifies that the channel is ready to close.
    pub fn close(&self) -> anyhow::Result<()> {
        self.send(BatchTileRequest::End)
    }

    /// Sends a [`BatchTileRequest`].
    fn send(&self, msg: BatchTileRequest) -> anyhow::Result<()> {
        self.tx
            .unbounded_send(msg)
            .context("Failed to send tile request on the channel")
    }
}

impl TileRequestReceiver {
    /// Transforms the receiving end of the channel into an asynchronous stream
    /// of [`TileRequest`]s.
    pub fn into_stream(self) -> impl Stream<Item = TileRequest> {
        let current = self.current;

        self.rx
            .flat_map(|batch_request| {
                let requests = match batch_request {
                    BatchTileRequest::Tiles(tiles) => {
                        tiles.iter().map(|&tile| TileRequest::Tile(tile)).collect()
                    }
                    BatchTileRequest::Evicted(tile) => vec![TileRequest::Evicted(tile)],
                    BatchTileRequest::End => vec![TileRequest::End],
                };
                stream::iter(requests)
            })
            .take_while(|tile_request| future::ready(!matches!(tile_request, TileRequest::End)))
            .filter_map(move |tile_request| {
                future::ready(match tile_request {
                    TileRequest::Tile(tile) => {
                        // Check whether the request is still valid, and categorize it (visible or
                        // speculated).
                        let current = current.read().unwrap();
                        if current.tiles.iter().any(|&x| tile == x) {
                            debug!("Request {:?}", tile);
                            Some(TileRequest::Tile(tile))
                        } else if current.speculative.iter().any(|&x| tile == x) {
                            debug!("Speculate {:?}", tile);
                            Some(TileRequest::Speculate(tile))
                        } else {
                            debug!("Drop {:?}", tile);
                            None
                        }
                    }
                    _ => Some(tile_request),
                })
            })
    }
}
