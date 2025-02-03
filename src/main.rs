use std::{
    fs::{OpenOptions, create_dir_all},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context as _, Result};
use clap::Parser as _;
use dirs::data_local_dir;
use event_loop::event_loop;
use zbus::Connection;

mod args;
mod dbus;
mod event_loop;
mod output;
mod player;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    args.init_tracing_subscriber();
    let filter_keys = args.skip_metadata.into_iter().collect();

    let filename = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(
            &time::format_description::parse_borrowed::<2>(&args.filename)
                .context("Failed to parse filename formatter")?,
        )
        .context("Failed to format time as specified by filename")?;
    let mut outfile = std::env::var_os("MPRIS_STATS_DIR")
        .map(PathBuf::from)
        .ok_or(())
        .or_else(|()| {
            data_local_dir()
                .context("Failed to get data directory")
                .map(|p| p.join("mpris-stats"))
        })?;
    if !outfile.exists() {
        create_dir_all(&outfile).context("Failed to create data directory")?;
    }
    outfile.push(filename);
    let outfile = OpenOptions::new()
        .append(true)
        .create(true)
        .open(outfile)
        .context("Failed to open output file")?;

    let connection = Connection::session().await?;
    event_loop(
        connection,
        Duration::from_secs_f64(args.log_every),
        outfile,
        filter_keys,
    )
    .await
}
