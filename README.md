# Ridemap - interactive map to visualize GPS tracks

[![Safety Dance](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![Minimum Rust 1.74](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://releases.rs/docs/1.74.0/)
[![Dependencies](https://deps.rs/repo/github/gendx/ridemap/status.svg)](https://deps.rs/repo/github/gendx/ridemap)
[![Lines of Code](https://www.aschey.tech/tokei/github/gendx/ridemap?category=code)](https://github.com/aschey/vercel-tokei)
[![Build Status](https://github.com/gendx/ridemap/workflows/Build/badge.svg)](https://github.com/gendx/ridemap/actions/workflows/build.yml)
[![Test Status](https://github.com/gendx/ridemap/workflows/Tests/badge.svg)](https://github.com/gendx/ridemap/actions/workflows/tests.yml)

This program allows you to visualize GPS tracks on an interactive map of the world.
GPS tracks can be loaded from local GPX files, or fetched via the Strava API.
It is written in [Rust](https://www.rust-lang.org/).

## Usage

Basic usage, to display a background map only (see the configuration [below](#map-providers)):

``` bash
$ cargo run --release -- \
    --cache-directory cache/ \
    --map-config map-provider.json \
    --lazy-ui-refresh
```

Loading tracks from local GPX files:

```bash
$ cargo run --release -- \
    --cache-directory cache/ \
    --map-config map-provider.json \
    --lazy-ui-refresh \
    gpx \
    --file track1.gpx,track2.gpx
```

Loading tracks from Strava activities (see the configuration [below](#strava-api)):

```bash
$ cargo run --release -- \
    --cache-directory cache/ \
    --map-config map-provider.json \
    --lazy-ui-refresh \
    strava \
    --strava-config strava-config.json
```

Advanced usage:

``` bash
$ cargo run --release -- \
    --cache-directory cache/ \
    --map-config map-provider.json \
    --lazy-ui-refresh \
    --speculative-tile-load \
    --background-ui-thread \
    --parallel-requests 10 \
    --max-pixels-per-tile 512 \
    --max-tile-level 15 \
    strava \
    --strava-config strava-config.json \
    --port 8080 \
    --activities-per-page 50 \
    --activity-pages 12 \
    --activity-count 500 \
    --activity-types ride,hike,swim
```

## Map providers

To display the background map, you need to specify a [tiled map provider](https://en.wikipedia.org/wiki/Tiled_web_map), that must serve PNG tiles of the world via HTTP addresses of the format `https://{server}/{z}/{x}/{y}.{extension}`.

Here is an example of `map-provider.json` file that you pass to the `--map-config` parameter.

```json
{
    "server": "example.com/tiles",
    "cache_folder": "local/cache/sub/folder",
    "extension": "@2x.png",
    "referer": "https://example.com/",
    "user_agent": "User agent",
}
```

- The `server` is the address of the HTTPS tile server, including any sub-folder.
  It will be prefixed by `https://`.
- The `cache_folder` is a sub-folder of the `--cache-directory` on your disk where tiles for this provider will be cached.
- The `extension` is a suffix to append to each HTTP query, typically `png`.
  Depending on the provider, you can also use it to request larger tiles (via `@2x`), or pass an access token.
- The `referer` is an optional value to put in the [Referer HTTP header](https://en.wikipedia.org/wiki/HTTP_referer) on requests to this tile server.
- The `user_agent` is an optional value to put in the [User-Agent HTTP header](https://en.wikipedia.org/wiki/User-Agent_header) on requests to this tile server.

For now, only tiles in PNG format are supported.

## Strava API

To automatically fetch GPS tracks from your recent Strava activities, please follow these steps.

First, you need to create a Strava application via your account, following the [getting started guide](https://developers.strava.com/docs/getting-started/).

From there, copy the `client_id` and `client_secret` given to you by Strava, and paste them into a `strava-config.json` file.

```json
{
    "client_id": "<your client id>",
    "client_secret": "<your client secret>",
}
```

You can then direct Ridemap to use these API credentials via the `--strava-config` parameters.

```bash
$ ridemap ... strava --strava-config strava-config.json
```

When running this, Ridemap will print a URL in the terminal output, that you have to click to authenticate a OAuth token to your app for your account and fetch your rides.

```
Please visit: https://www.strava.com/oauth/authorize?client_id=<client_id>&redirect_uri=http%3A%2F%2F127.0.0.1%3A8080&response_type=code&approval_prompt=auto&scope=read,activity:read_all
```

Once you click the link and authorize Ridemap, activities will start being fetched and displayed on the map.

Various parameters allow to filter which activities to display, you can list these parameters via the CLI help.

```bash
$ ridemap strava --help
```

## Other configuration

The `FONT_PATH` variable in `src/config.rs` needs to point to a valid font file on your system.
The hard-coded value is a good default on Linux systems.

## License

MIT
