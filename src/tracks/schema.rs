//! JSON schemas for [Strava's API](https://developers.strava.com/docs/reference/).

use anyhow::bail;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// A [Fault](https://developers.strava.com/docs/reference/#api-models-Fault) message in Strava's
/// API.
// The Rust compiler considers the fields as dead code, even though we Debug them in logs.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Fault {
    message: String,
    errors: Vec<Error>,
}

/// An [Error](https://developers.strava.com/docs/reference/#api-models-Error) message in Strava's
/// API.
// The Rust compiler considers the fields as dead code, even though we Debug them in logs.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Error {
    code: String,
    field: String,
    resource: String,
}

/// The result of a [OAuth token
/// exchange](https://developers.strava.com/docs/authentication/#tokenexchange) performed on
/// Strava's API.
#[derive(Debug, Deserialize)]
pub struct Token {
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_at: u32,
    #[allow(dead_code)]
    expires_in: u32,
    #[allow(dead_code)]
    refresh_token: String,
    /// OAuth access token.
    pub access_token: String,
}

/// An [DetailedAthlete](https://developers.strava.com/docs/reference/#api-models-DetailedAthlete)
/// message in Strava's API.
// The Rust compiler considers the fields as dead code, even though we Debug them in logs.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct DetailedAthlete {
    id: u64,
    firstname: String,
    lastname: String,
}

/// A [SummaryActivity](https://developers.strava.com/docs/reference/#api-models-SummaryActivity)
/// in Strava's API.
///
/// Note: although `SummaryActivity` has a map field, the full polyline is only
/// available in [`DetailedActivity`].
#[derive(Debug, Deserialize)]
pub struct SummaryActivity {
    /// The unique identifier of the activity.
    pub id: u64,
    /// The type of activity.
    // TODO: Deprecated in favor of SportType.
    pub r#type: ActivityType,
}

/// A [DetailedActivity](https://developers.strava.com/docs/reference/#api-models-DetailedActivity)
/// in Strava's API.
#[derive(Debug, Deserialize, Serialize)]
pub struct DetailedActivity {
    id: u64,
    name: String,
    distance: f64,
    moving_time: u32,
    elapsed_time: u32,
    total_elevation_gain: f64,
    /// The type of activity.
    // TODO: Deprecated in favor of SportType.
    pub r#type: ActivityType,
    workout_type: Option<u32>,
    description: Option<String>,
    /// The polyline track of the activity on the map.
    pub map: PolylineMap,
}

/// A [PolylineMap](https://developers.strava.com/docs/reference/#api-models-PolylineMap) in
/// Strava's API.
#[derive(Debug, Deserialize, Serialize)]
pub struct PolylineMap {
    id: String,
    /// The detailed polyline of the map, encoded with [Google's
    /// algorithm](https://developers.google.com/maps/documentation/utilities/polylinealgorithm).
    pub polyline: Option<String>,
    /// A summary polyline of the map, encoded with [Google's
    /// algorithm](https://developers.google.com/maps/documentation/utilities/polylinealgorithm).
    pub summary_polyline: Option<String>,
}

/// An [ActivityType](https://developers.strava.com/docs/reference/#api-models-ActivityType) in
/// Strava's API.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[allow(missing_docs)]
pub enum ActivityType {
    AlpineSki,
    BackcountrySki,
    Canoeing,
    Crossfit,
    EBikeRide,
    Elliptical,
    Golf,
    Handcycle,
    Hike,
    IceSkate,
    InlineSkate,
    Kayaking,
    Kitesurf,
    NordicSki,
    Ride,
    RockClimbing,
    RollerSki,
    Rowing,
    Run,
    Sail,
    Skateboard,
    Snowboard,
    Snowshoe,
    Soccer,
    StairStepper,
    StandUpPaddling,
    Surfing,
    Swim,
    Velomobile,
    VirtualRide,
    VirtualRun,
    Walk,
    WeightTraining,
    Wheelchair,
    Windsurf,
    Workout,
    Yoga,
}

impl FromStr for ActivityType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AlpineSki" => Ok(ActivityType::AlpineSki),
            "BackcountrySki" => Ok(ActivityType::BackcountrySki),
            "Canoeing" => Ok(ActivityType::Canoeing),
            "Crossfit" => Ok(ActivityType::Crossfit),
            "EBikeRide" => Ok(ActivityType::EBikeRide),
            "Elliptical" => Ok(ActivityType::Elliptical),
            "Golf" => Ok(ActivityType::Golf),
            "Handcycle" => Ok(ActivityType::Handcycle),
            "Hike" => Ok(ActivityType::Hike),
            "IceSkate" => Ok(ActivityType::IceSkate),
            "InlineSkate" => Ok(ActivityType::InlineSkate),
            "Kayaking" => Ok(ActivityType::Kayaking),
            "Kitesurf" => Ok(ActivityType::Kitesurf),
            "NordicSki" => Ok(ActivityType::NordicSki),
            "Ride" => Ok(ActivityType::Ride),
            "RockClimbing" => Ok(ActivityType::RockClimbing),
            "RollerSki" => Ok(ActivityType::RollerSki),
            "Rowing" => Ok(ActivityType::Rowing),
            "Run" => Ok(ActivityType::Run),
            "Sail" => Ok(ActivityType::Sail),
            "Skateboard" => Ok(ActivityType::Skateboard),
            "Snowboard" => Ok(ActivityType::Snowboard),
            "Snowshoe" => Ok(ActivityType::Snowshoe),
            "Soccer" => Ok(ActivityType::Soccer),
            "StairStepper" => Ok(ActivityType::StairStepper),
            "StandUpPaddling" => Ok(ActivityType::StandUpPaddling),
            "Surfing" => Ok(ActivityType::Surfing),
            "Swim" => Ok(ActivityType::Swim),
            "Velomobile" => Ok(ActivityType::Velomobile),
            "VirtualRide" => Ok(ActivityType::VirtualRide),
            "VirtualRun" => Ok(ActivityType::VirtualRun),
            "Walk" => Ok(ActivityType::Walk),
            "WeightTraining" => Ok(ActivityType::WeightTraining),
            "Wheelchair" => Ok(ActivityType::Wheelchair),
            "Windsurf" => Ok(ActivityType::Windsurf),
            "Workout" => Ok(ActivityType::Workout),
            "Yoga" => Ok(ActivityType::Yoga),
            _ => bail!("Unknown activity type: {s}"),
        }
    }
}
