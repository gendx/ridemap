//! Local on-disk cache.

use crate::config::MapProvider;
use crate::map::tiles::TileIndex;
use crate::tracks::schema::DetailedActivity;
use anyhow::Context;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use tokio::task::spawn_blocking;

/// Handle to the on-disk cache.
pub struct Cache {
    /// Root directory.
    cache_root: PathBuf,
    /// Folder name for the current map provider.
    map_provider_folder: String,
}

impl Cache {
    /// Initializes the cache at the given root directory for the given map
    /// provider.
    pub fn new(cache_directory: &str, map_provider: &MapProvider) -> anyhow::Result<Self> {
        let cache_root = PathBuf::from(cache_directory);
        let map_provider_folder = map_provider.cache_folder.clone();
        fs::create_dir_all(cache_root.join("strava/activities"))
            .context("Failed to create the strava/activities cache")?;
        fs::create_dir_all(cache_root.join(format!("tiles/{}", map_provider_folder)))
            .with_context(|| {
                format!("Failed to create the tile cache for provider: {map_provider_folder}")
            })?;
        Ok(Self {
            cache_root,
            map_provider_folder,
        })
    }

    /// Writes the given Strava activity.
    pub fn set_activity(&self, id: u64, activity: &DetailedActivity) -> anyhow::Result<()> {
        let file = File::create(self.activity_path(id)).with_context(|| {
            format!("Failed to create file for Strava activity (id = {id}) in cache")
        })?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, activity)
            .with_context(|| format!("Failed to serialize Strava activity (id = {id}) in cache"))
    }

    /// Reads the given Strava activity.
    pub async fn get_activity(&self, id: u64) -> anyhow::Result<DetailedActivity> {
        let path = self.activity_path(id);

        spawn_blocking(move || -> anyhow::Result<DetailedActivity> {
            let path = &path;
            let file = File::open(path).with_context(|| {
                format!(
                    "Failed to open file for Strava activity (id = {id}) from cache: {}",
                    path.display()
                )
            })?;
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).with_context(|| {
                format!(
                    "Failed to parse file for Strava activity (id = {id}) from cache: {}",
                    path.display()
                )
            })
        })
        .await
        .with_context(|| {
            format!("Failed to join background task to load Strava activity (id = {id}) from cache")
        })?
    }

    /// Reads the given map tile.
    pub fn get_tile(&self, index: &TileIndex) -> anyhow::Result<Box<[u8]>> {
        let mut file = File::open(self.tile_path(index))
            .with_context(|| format!("Failed to open file for tile: {index:?}"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .with_context(|| format!("Failed to read file for tile: {index:?}"))?;
        Ok(buf.into_boxed_slice())
    }

    /// Writes the given map tile.
    pub fn set_tile(&self, index: &TileIndex, tile: &[u8]) -> anyhow::Result<()> {
        let mut file = File::create(self.tile_path(index))
            .with_context(|| format!("Failed to create file for tile: {index:?}"))?;
        file.write_all(tile)
            .with_context(|| format!("Failed to write file for tile: {index:?}"))?;
        Ok(())
    }

    /// Computes the path associated to the given activity.
    fn activity_path(&self, id: u64) -> PathBuf {
        self.cache_root
            .join(format!("strava/activities/{}.json", id))
    }

    /// Computes the path associated to the given map tile.
    fn tile_path(&self, index: &TileIndex) -> PathBuf {
        self.cache_root.join(format!(
            "tiles/{provider}/{z}-{x}-{y}.png",
            provider = self.map_provider_folder,
            z = index.z,
            x = index.x,
            y = index.y
        ))
    }
}
