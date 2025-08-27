//! Client to access [Strava's API](https://developers.strava.com/docs/reference/).

use super::polyline::{Polyline, ToMercator};
use super::schema::*;
use crate::caching::cache::Cache;
use crate::ui::UiMessage;
use anyhow::bail;
use clap::builder;
use clap::error::ErrorKind;
use futures::{stream, Stream, StreamExt};
use log::{debug, error, info, trace};
use regex::Regex;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Base URL for Strava's API.
const API_URL: &str = "https://www.strava.com/api/v3";
/// Base URL for Strava's OAuth authorization.
const AUTHORIZE_URL: &str = "https://www.strava.com/oauth/authorize";
/// Address to redirect the OAuth authorization to, i.e. localhost.
const AUTHORIZE_REDIRECT_ADDR: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// OAuth token scope(s) to request from Strava.
const AUTHORIZE_SCOPE: &str = "read,activity:read_all";

/// Configuration to identify an application on Strava's API. See
/// <https://developers.strava.com/docs/getting-started/#account>.
#[derive(Clone, Debug, Deserialize)]
pub struct StravaConfig {
    /// Strava application ID.
    client_id: String,
    /// Client secret for this Strava application.
    client_secret: String,
}

impl StravaConfig {
    /// Reads a Strava application configuration from the given file.
    fn read_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: Self = serde_json::from_reader(reader)?;

        if !config.client_id.chars().all(|c| c.is_ascii_digit()) {
            bail!("Expected only digits for client_id: {}", config.client_id);
        }
        if !config.client_secret.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!(
                "Expected only hexdigits for client_secret: {}",
                config.client_secret
            );
        }

        Ok(config)
    }
}

/// Adapter to parse a Strava configuration directly from a Clap parameter.
#[derive(Clone)]
pub struct StravaConfigParser;

impl builder::TypedValueParser for StravaConfigParser {
    type Value = StravaConfig;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        StravaConfig::read_from_file(value).map_err(|e| {
            let arg_str = arg.map(|a| a.to_string());
            // TODO: use clap::builder::StyledStr once it supports coloring the arguments.
            let msg = format!(
                "Failed to parse Strava configuration{}{}: {}\n",
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

impl builder::ValueParserFactory for StravaConfig {
    type Parser = StravaConfigParser;

    fn value_parser() -> Self::Parser {
        StravaConfigParser
    }
}

/// Client to connect to [Strava's API](https://developers.strava.com/docs/reference/). This
/// maintains state for the authenticated Strava athlete.
pub struct StravaClient<'a> {
    cache: Option<&'a Cache>,
    client: &'a Client,
    bearer_token: String,
}

impl<'a> StravaClient<'a> {
    /// Creates a new client for the given Strava application, performing an
    /// OAuth token exchange on the given redirection port on localhost.
    pub async fn new(
        cache: Option<&'a Cache>,
        client: &'a Client,
        config: &StravaConfig,
        authorize_redirect_port: u16,
    ) -> anyhow::Result<StravaClient<'a>> {
        let oauth_code = StravaClient::oauth_authorize(config, authorize_redirect_port).await?;

        let bearer_token = StravaClient::oauth_exchange(client, config, &oauth_code).await?;

        Ok(Self {
            cache,
            client,
            bearer_token,
        })
    }

    /// Performs an OAuth token exchange for the given Strava application, using
    /// the given redirection port on localhost.
    async fn oauth_authorize(
        config: &StravaConfig,
        authorize_redirect_port: u16,
    ) -> anyhow::Result<String> {
        println!(
            "Please visit: {}?client_id={}&redirect_uri=http%3A%2F%2F{}%3A{}&response_type=code&approval_prompt=auto&scope={}",
            AUTHORIZE_URL,
            config.client_id,
            AUTHORIZE_REDIRECT_ADDR,
            authorize_redirect_port,
            AUTHORIZE_SCOPE
        );

        let listener =
            tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, authorize_redirect_port)).await?;

        let (mut socket, addr) = listener.accept().await?;
        info!("Request from: {}", addr);

        let authorized_ip = match addr.ip() {
            IpAddr::V4(ip4) => ip4.is_loopback() || ip4.is_private(),
            IpAddr::V6(_) => false,
        };
        if !authorized_ip {
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nUnauthorized IP address")
                .await?;
            bail!("Unauthorized IP address: {}", addr);
        }

        // TODO: newline
        const REGEX: &str = r"^GET /\?state=&code=([0-9a-f]+)&scope=([a-z_,:]+) HTTP/1\.1";
        let re = Regex::new(REGEX).unwrap();

        let mut buf = [0; 128];
        socket.read_exact(&mut buf).await?;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nAll good!")
            .await?;

        debug!("Parsing OAuth code");
        let req_utf8 = std::str::from_utf8(&buf)?;
        let captures = re.captures(req_utf8).ok_or_else(|| {
            Box::new(io::Error::other(
                "Invalid request from the browser".to_owned(),
            ))
        })?;

        let oauth_code = &captures[1];
        let scope = &captures[2];
        assert_eq!(scope, AUTHORIZE_SCOPE);

        Ok(oauth_code.to_owned())
    }

    /// Performs an [OAuth token
    /// exchange](https://developers.strava.com/docs/authentication/#tokenexchange) with Strava's
    /// API, using the given `oauth_code`, and returns the corresponding access
    /// token.
    async fn oauth_exchange(
        client: &Client,
        config: &StravaConfig,
        oauth_code: &str,
    ) -> anyhow::Result<String> {
        debug!("Exchanging OAuth token");
        let response = client
            .post("https://www.strava.com/oauth/token")
            .query(&[
                ("client_id", config.client_id.as_str()),
                ("client_secret", config.client_secret.as_str()),
                ("code", oauth_code),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await?;

        let response = StravaClient::check_response_status(response).await?;

        let token: Token = response.json().await?;
        debug!("Token = {:#?}", token);

        Ok(token.access_token)
    }

    /// Gets the authenticated athlete in Strava's API
    /// ([getLoggedInAthlete](https://developers.strava.com/docs/reference/#api-Athletes-getLoggedInAthlete)).
    pub async fn get_athlete(&self) -> anyhow::Result<DetailedAthlete> {
        debug!("Query authenticated athlete");
        let response = self
            .client
            .get(format!("{API_URL}/athlete"))
            .bearer_auth(&self.bearer_token)
            .send()
            .await?;

        let response = StravaClient::check_response_status(response).await?;

        let athlete = response.json().await?;
        Ok(athlete)
    }

    /// Returns a stream of summary activities for the authenticated athlete in
    /// Strava's API.
    ///
    /// This fetches the most recent `count_pages` pages, each containing
    /// `count_per_page` activities.
    pub fn get_activity_list(
        &self,
        count_per_page: usize,
        count_pages: usize,
        parallel_requests: usize,
    ) -> impl Stream<Item = SummaryActivity> + '_ {
        stream::iter(0..count_pages)
            .map(move |i| async move {
                match self.get_activity_list_page(count_per_page, i).await {
                    Ok(list) => Ok((i, list)),
                    Err(e) => Err(e),
                }
            })
            .buffered(parallel_requests)
            .flat_map(|result| {
                let list = match result {
                    Ok((i, list)) => {
                        debug!("Received page #{}", i);
                        list
                    }
                    Err(e) => {
                        error!("Error receiving page: {}", e);
                        Vec::new()
                    }
                };
                stream::iter(list)
            })
    }

    /// Gets the list of activities for the authenticated athlete in Strava's
    /// API ([getLoggedInAthleteActivities](https://developers.strava.com/docs/reference/#api-Activities-getLoggedInAthleteActivities)).
    async fn get_activity_list_page(
        &self,
        count_per_page: usize,
        i: usize,
    ) -> anyhow::Result<Vec<SummaryActivity>> {
        debug!("Query page {}", i);
        let response = self
            .client
            .get(format!(
                "{API_URL}/athlete/activities?per_page={}&page={}",
                count_per_page,
                i + 1
            ))
            .bearer_auth(&self.bearer_token)
            .send()
            .await?;

        let response = StravaClient::check_response_status(response).await?;

        let activity_list = response.json().await?;
        Ok(activity_list)
    }

    /// Fetches the detailed activities corresponding to a stream of summary
    /// activities, and sends the results as UI messages to the given sending
    /// channel.
    ///
    /// This makes up to `parallel_requests` in parallel.
    pub async fn get_detailed_activities_parallel(
        &self,
        tx: &mpsc::Sender<UiMessage>,
        activity_stream: impl Stream<Item = SummaryActivity>,
        parallel_requests: usize,
    ) -> anyhow::Result<()> {
        let detailed_activities = activity_stream
            .enumerate()
            .map(|(i, activity)| async move {
                self.get_detailed_activity(&activity, i)
                    .await
                    .map(|a| (i, a))
            })
            .buffer_unordered(parallel_requests);

        detailed_activities
            .for_each(|activity| async {
                match activity {
                    Ok((i, a)) => {
                        trace!("Activity = {:#?}", a);
                        let summary = a
                            .map
                            .summary_polyline
                            .as_ref()
                            .and_then(|p| Polyline::new(p));
                        let polyline = a.map.polyline.as_ref().and_then(|p| Polyline::new(p));
                        debug!(
                            "Summary polyline has {:?} points in {:?} bytes",
                            summary.map(|p| p.len()),
                            a.map.summary_polyline.map(|p| p.len())
                        );
                        debug!(
                            "Polyline has {:?} points in {:?} bytes",
                            polyline.as_ref().map(|p| p.len()),
                            a.map.polyline.map(|p| p.len())
                        );
                        if let Some(p) = polyline {
                            tx.send(UiMessage::Activity {
                                id: i,
                                r#type: a.r#type,
                                points: p.mercator_points(),
                            })
                            .unwrap();
                        }
                    }
                    Err(e) => error!("Got an error: {}", e),
                }
            })
            .await;

        Ok(())
    }

    /// Gets the detailed activity corresponding to a summary activity in
    /// Strava's API ([getActivityById](https://developers.strava.com/docs/reference/#api-Activities-getActivityById)).
    async fn get_detailed_activity(
        &self,
        activity: &SummaryActivity,
        i: usize,
    ) -> anyhow::Result<DetailedActivity> {
        let id = activity.id;

        if let Some(cache) = self.cache {
            let cached = cache.get_activity(id).await;
            if cached.is_ok() {
                debug!("Obtained activity {} from cache", i);
                return cached;
            }
        }

        debug!("Query activity {}", i);
        let response = self
            .client
            .get(format!("{API_URL}/activities/{}", activity.id))
            .bearer_auth(&self.bearer_token)
            .send()
            .await?;

        debug!("Checking response for activity {}", i);
        let response = StravaClient::check_response_status(response).await?;

        let activity_bytes = response.bytes().await?;
        let activity = match serde_json::from_slice(&activity_bytes) {
            Ok(a) => a,
            Err(e) => {
                error!("Invalid activity:\n{:#?}", activity_bytes);
                return Err(e.into());
            }
        };

        debug!("Parsed response for activity {}", i);
        if let Some(cache) = self.cache {
            if let Err(e) = cache.set_activity(id, &activity) {
                error!("Couldn't write activity {} to cache: {:?}", i, e);
            }
        }
        Ok(activity)
    }

    /// Checks that a response from Strava's API contains an OK status code,
    /// returning an error with more details otherwise.
    async fn check_response_status(response: Response) -> anyhow::Result<Response> {
        let status_code = response.status();
        if status_code != StatusCode::OK {
            error!("Strava server replied with status code {status_code}");
            let fault: Fault = response.json().await?;
            error!("Strava error = {:#?}", fault);
            bail!("Strava server replied with status code {status_code}");
        } else {
            Ok(response)
        }
    }
}
