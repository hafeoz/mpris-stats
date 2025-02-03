use std::{collections::HashMap, fs::File, io::Write};

use anyhow::{Context as _, Result};
use serde::Serialize;

#[derive(Serialize)]
pub struct MPRISActivity {
    pub players: HashMap<String, HashMap<String, String>>,
    pub timestamp: u64,
}

pub fn write_activity(activity: &MPRISActivity, mut file: &mut File) -> Result<()> {
    serde_json::to_writer(&mut file, &activity).context("Failed to serialize activity to file")?;
    file.write_all(b"\n")
        .context("Failed to write newline to activity file")?;
    file.flush().context("Failed to flush activity file")
}
