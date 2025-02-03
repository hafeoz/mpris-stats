mod update_listener;

use anyhow::{Context as _, Result, bail};
use futures_lite::StreamExt as _;
use std::{collections::{HashMap, HashSet}, fs::File, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{select, sync::mpsc, time};
use update_listener::get_player_info;
use zbus::{Connection, names::OwnedBusName};

use crate::{
    dbus::{BusActivity, BusChange, player_buses},
    output,
    player::{PlaybackStatus, PlayerInformation},
};

pub async fn event_loop(
    conn: Connection,
    write_interval: Duration,
    mut activity_file: File,
    filter_keys: HashSet<String>
) -> Result<()> {
    let mut dbus_stream = player_buses(&conn).await?;

    let (player_update_sender, mut player_update_receiver) = mpsc::channel(1);

    let mut available_players: HashMap<_, (PlayerInformation, _)> = HashMap::new();

    let mut write_interval = time::interval(write_interval);
    let mut write = |available_players: &mut HashMap<
        Arc<OwnedBusName>,
        (PlayerInformation, tokio::task::JoinHandle<Result<()>>),
    >|
     -> Result<()> {
        let players = available_players
            .iter()
            .filter(|(_, (i, _))| i.status == PlaybackStatus::Playing)
            .map(|(i, (j, _))| {
                (
                    i.to_string(),
                    j.metadata(&filter_keys)
                        .map(|(i, j)| (i.to_owned(), j.to_string()))
                        .collect(),
                )
            })
            .collect();
        let state = output::MPRISActivity {
            players,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("System time earlier than UNIX_EPOCH")?
                .as_secs(),
        };
        output::write_activity(&state, &mut activity_file)?;
        Ok(())
    };

    loop {
        select! {
            bus_change = dbus_stream.next() => {
                let Some(BusChange { name: bus_name, activity: bus_activity }) = bus_change else {
                    tracing::error!("DBus NameOwnerChanged stream closed");
                    continue
                };
                let bus_name = Arc::new(bus_name);
                match bus_activity {
                    BusActivity::Created => {
                        tracing::info!(%bus_name, "New player registered");
                        let (player_info, player_updater) = match get_player_info(Arc::clone(&bus_name), conn.clone(), player_update_sender.clone()).await {
                            Ok(i) => i,
                            Err(e) => {
                                tracing::error!(?e, "Failed to get player information from DBus");
                                continue
                            }
                        };

                        available_players.insert(bus_name, (player_info, player_updater));
                    },
                    BusActivity::Destroyed => {
                        let Some((_, updater)) = available_players.remove(&bus_name) else { bail!("Attempting to destroy a non-existent player {bus_name}") };
                        updater.abort();
                    }
                };
                write(&mut available_players)?;
                write_interval.reset();
            }
            Some((bus_name, player_update)) = player_update_receiver.recv() => {
                tracing::debug!(%bus_name, ?player_update, "Player status updated");
                let Some((info, _)) = available_players.get_mut(&bus_name) else { bail!("Attempting to update a non-existent player {bus_name}") };
                info.apply_update(player_update);

                write(&mut available_players)?;
                write_interval.reset();
            }
            _ = write_interval.tick() => {
                write(&mut available_players)?;
            }
            else => { bail!("Player stream closed"); }
        }
    }
}
