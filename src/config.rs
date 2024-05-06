//! Configuration utilities.

use anyhow::Context;
use clap::builder;
use clap::error::ErrorKind;
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Path to the font to use when displaying text on the UI.
pub const FONT_PATH: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf";

/// Description of a web service providing tiles.
#[derive(Clone, Debug, Deserialize)]
pub struct MapProvider {
    /// Address of the HTTPS tile server, including the domain name and any
    /// sub-directories.
    pub server: String,
    /// Local sub-folder (relative to the root `--cache-directory`) where tiles
    /// for this provider should be cached.
    pub cache_folder: String,
    /// File extension to append to each tile request.
    ///
    /// A simple example is `.png`. Additionally, some services may require an
    /// access token parameter, provide higher-resolution tiles under a `@2x`
    /// suffix, etc.
    ///
    /// Note: the current implementation only supports tiles in PNG format.
    pub extension: String,
    /// Referer HTTP header to attach to each tile request.
    pub referer: Option<String>,
    /// User-agent HTTP header to attach to each tile request.
    pub user_agent: Option<String>,
}

impl MapProvider {
    /// Reads a map provider configuration from the given JSON file.
    fn read_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).with_context(|| {
            format!(
                "Failed to read map provider configuration from: {}",
                path.display()
            )
        })?;
        let reader = BufReader::new(file);
        let provider: Self = serde_json::from_reader(reader).with_context(|| {
            format!(
                "Failed to parse map provider configuration from: {}",
                path.display()
            )
        })?;

        Ok(provider)
    }
}

/// Helper struct to parse a [`MapProvider`] configuration directly from a Clap
/// argument.
#[derive(Clone)]
pub struct MapProviderParser;

impl builder::TypedValueParser for MapProviderParser {
    type Value = MapProvider;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        MapProvider::read_from_file(value).map_err(|e| {
            let arg_str = arg.map(|a| a.to_string());
            // TODO: use clap::builder::StyledStr once it supports coloring the arguments.
            let msg = format!(
                "Failed to parse map provider configuration{}{}: {}\n",
                arg_str.map(|a| format!(" ({})", a)).unwrap_or_default(),
                value
                    .to_str()
                    .map(|f| format!(" from file `{}`", f))
                    .unwrap_or_default(),
                e
            );
            clap::Error::raw(ErrorKind::Io, msg).with_cmd(cmd)
        })
    }
}

impl builder::ValueParserFactory for MapProvider {
    type Parser = MapProviderParser;

    fn value_parser() -> Self::Parser {
        MapProviderParser
    }
}
