use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    str::FromStr,
};

use anyhow::{Context as _, Result, anyhow};
use futures_lite::{StreamExt as _, stream::Fuse};
use tokio::select;
use zbus::{
    proxy::PropertyStream,
    zvariant::{OwnedValue, Value},
};

use crate::dbus::player::PlayerProxy;

/// Current playback status of a MPRIS-compliant player
#[derive(Eq, PartialEq, Debug)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}
impl FromStr for PlaybackStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_ref() {
            "playing" => Ok(Self::Playing),
            "paused" => Ok(Self::Paused),
            "stopped" => Ok(Self::Stopped),
            _ => Err(anyhow!("Unknown PlaybackStatus {s}")),
        }
    }
}

#[derive(Debug)]
pub struct PlayerInformation {
    pub metadata: std::collections::HashMap<String, OwnedValue>,
    pub rate: f64,
    pub status: PlaybackStatus,
}
impl PlayerInformation {
    #[must_use]
    fn format_value<'a>(v: &'a Value<'_>) -> Cow<'a, str> {
        match v {
            zbus::zvariant::Value::U8(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Bool(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I16(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U16(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I32(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U32(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::F64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Str(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Signature(s) => Cow::Owned(s.to_string()),
            zbus::zvariant::Value::ObjectPath(o) => Cow::Borrowed(o.as_str()),
            zbus::zvariant::Value::Value(v) => Self::format_value(v),
            zbus::zvariant::Value::Array(a) => Cow::Owned(
                a.iter()
                    .map(Self::format_value)
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Dict(d) => Cow::Owned(
                d.iter()
                    .map(|(k, v)| format!("{}={}", Self::format_value(k), Self::format_value(v)))
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Structure(s) => Cow::Owned(
                s.fields()
                    .iter()
                    .map(Self::format_value)
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Fd(_) => Cow::Borrowed("fd"),
        }
    }
    pub fn metadata<'a>(
        &'a self,
        filter_keys: &'a HashSet<String>,
    ) -> impl Iterator<Item = (&'a String, Cow<'a, str>)> {
        self.metadata
            .iter()
            .filter(|(k, _)| filter_keys.get(k.as_str()).is_none())
            .map(|(k, v)| (k, Self::format_value(v)))
    }
}
pub struct PlayerInformationUpdateListener<'a> {
    metadata_stream: Fuse<PropertyStream<'a, HashMap<String, OwnedValue>>>,
    rate_stream: Fuse<PropertyStream<'a, f64>>,
    status_stream: Fuse<PropertyStream<'a, String>>,
}
#[derive(Debug)]
pub enum PlayerInformationUpdate {
    Metadata(HashMap<String, OwnedValue>),
    Rate(f64),
    Status(PlaybackStatus),
}
impl PlayerInformation {
    pub async fn new(player: &PlayerProxy<'_>) -> Result<Self> {
        Ok(Self {
            metadata: player
                .metadata()
                .await
                .context("Failed to get player metadata")?,
            rate: player
                .rate()
                .await
                .context("Failed to get player playback rate")?,
            status: player
                .playback_status()
                .await
                .context("Failed to get player playback status")?
                .parse()?,
        })
    }

    pub fn apply_update(&mut self, update: PlayerInformationUpdate) {
        match update {
            PlayerInformationUpdate::Metadata(metadata) => {
                self.metadata = metadata;
            }
            PlayerInformationUpdate::Rate(rate) => {
                self.rate = rate;
            }
            PlayerInformationUpdate::Status(status) => {
                self.status = status;
            }
        }
    }
}

impl<'a> PlayerInformationUpdateListener<'a> {
    pub async fn new(player: PlayerProxy<'a>) -> Result<Self> {
        Ok(Self {
            metadata_stream: player.receive_metadata_changed().await.fuse(),
            rate_stream: player.receive_rate_changed().await.fuse(),
            status_stream: player.receive_playback_status_changed().await.fuse(),
        })
    }
    pub async fn update(&mut self) -> Result<PlayerInformationUpdate> {
        select! {
            metadata = self.metadata_stream.next() => {
                metadata.context("Failed to receive metadata update event")?.get().await.context("Failed to get player metadata").map(PlayerInformationUpdate::Metadata)
            },
            rate = self.rate_stream.next() => {
                rate.context("Failed to receive rate update event")?.get().await.context("Failed to get player playback rate").map(PlayerInformationUpdate::Rate)
            },
            status = self.status_stream.next() => {
                status.context("Failed to receive status update event")?.get().await.context("Failed to get player playback status")?.parse().map(PlayerInformationUpdate::Status)
            }
        }
    }
}
