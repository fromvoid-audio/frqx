//! Local saved-scale slots, separate from the VST3 host's project state.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::scale::{Scale, MAX_DEGREES};

/// Number of named locations in the local frqx scale library.
pub const SLOT_COUNT: usize = 8;

fn directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
    Ok(PathBuf::from(home).join("Library/Application Support/PORTALSURFER/Freqs/scales"))
}

fn path(slot: usize) -> Result<PathBuf, String> {
    if !(1..=SLOT_COUNT).contains(&slot) {
        return Err("Scale slot is out of range".into());
    }
    Ok(directory()?.join(format!("slot-{slot}.freqs")))
}

/// Whether this saved-scale slot already has a scale.
pub fn slot_exists(slot: usize) -> bool {
    path(slot).is_ok_and(|path| path.is_file())
}

/// Save a validated scale to one local library slot.
pub fn save_slot(slot: usize, scale: Scale) -> Result<(), String> {
    if !scale.valid() {
        return Err("The current scale is invalid".into());
    }
    let destination = path(slot)?;
    let parent = destination
        .parent()
        .ok_or("Scale library path is invalid")?;
    fs::create_dir_all(parent).map_err(|error| format!("Cannot create scale library: {error}"))?;
    let mut contents = format!(
        "FREQS 2\n{}\n{}\n{}\n{:.9}\n{:.9}\n",
        scale.count, scale.root_key, scale.root_degree, scale.root_hz, scale.period_cents
    );
    for cents in scale.cents {
        writeln!(&mut contents, "{cents:.9}").map_err(|error| error.to_string())?;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = parent.join(format!(".slot-{slot}-{}-{unique}.tmp", std::process::id()));
    fs::write(&temporary, contents).map_err(|error| format!("Cannot write scale: {error}"))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Cannot replace scale slot: {error}"));
    }
    Ok(())
}

/// Read and validate a scale from one local library slot.
pub fn load_slot(slot: usize) -> Result<Scale, String> {
    let contents =
        fs::read_to_string(path(slot)?).map_err(|error| format!("Cannot read scale: {error}"))?;
    parse(&contents)
}

fn parse(contents: &str) -> Result<Scale, String> {
    let mut lines = contents.lines();
    let stored_capacity = match lines.next() {
        Some("FREQS 1") => 16,
        Some("FREQS 2") => MAX_DEGREES,
        _ => return Err("Unsupported frqx scale file".into()),
    };
    fn next<T: std::str::FromStr>(lines: &mut std::str::Lines<'_>) -> Result<T, String> {
        lines
            .next()
            .ok_or("Scale file is incomplete".to_owned())?
            .parse()
            .map_err(|_| "Scale file contains an invalid value".to_owned())
    }
    let count = next(&mut lines)?;
    let root_key = next(&mut lines)?;
    let root_degree = next(&mut lines)?;
    let root_hz = next(&mut lines)?;
    let period_cents = next(&mut lines)?;
    let mut cents = [0.0; MAX_DEGREES];
    for value in cents.iter_mut().take(stored_capacity) {
        *value = next(&mut lines)?;
    }
    if lines.next().is_some() {
        return Err("Scale file has unexpected data".into());
    }
    let scale = Scale {
        count,
        cents,
        root_key,
        root_hz,
        root_degree,
        period_cents,
    };
    if !scale.valid() {
        return Err("Saved scale is outside supported limits".into());
    }
    Ok(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_scale_round_trips_and_rejects_corruption() {
        let mut scale = Scale {
            root_degree: 4,
            ..Scale::default()
        };
        scale.set_period(1901.955);
        let text = {
            let mut text = format!(
                "FREQS 2\n{}\n{}\n{}\n{:.9}\n{:.9}\n",
                scale.count, scale.root_key, scale.root_degree, scale.root_hz, scale.period_cents
            );
            for cents in scale.cents {
                writeln!(&mut text, "{cents:.9}").unwrap();
            }
            text
        };
        let loaded = parse(&text).unwrap();
        assert_eq!(loaded, scale);
        assert!(parse("FREQS 1\n7\n").is_err());
    }
}
