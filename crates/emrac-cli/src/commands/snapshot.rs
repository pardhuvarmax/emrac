use emrac_core::{say, Voice};
use emrac_core::{Result, SnapshotStore, Sources};

use crate::cli::SnapshotCommand;
use crate::output;

/// `snapshot` and `snapshot create`: saves the installed set.
pub fn create(sources: &Sources, command: Option<&SnapshotCommand>, json: bool) -> Result<()> {
    let (name, note) = match command {
        Some(SnapshotCommand::Create { name, note }) => (name.as_deref(), note.as_deref()),
        _ => (None, None),
    };
    let snapshot = sources.create_snapshot(name, note)?;
    output::print_snapshot_created(&snapshot, json);
    Ok(())
}

/// `list`, `show` and `delete` — nothing here needs libalpm.
pub fn run_stored(command: &SnapshotCommand, json: bool) -> Result<()> {
    match command {
        SnapshotCommand::List => output::print_snapshots(&SnapshotStore::list()?, json),
        SnapshotCommand::Show { name } => output::print_snapshot(&SnapshotStore::load(name)?, json),
        SnapshotCommand::Delete { name } => {
            SnapshotStore::delete(name)?;
            if json {
                println!("{}", serde_json::json!({ "deleted": name }));
            } else {
                say!(Voice::Confirms, "deleted snapshot '{name}'.");
            }
        }
        SnapshotCommand::Create { .. } => unreachable!("handled with Sources"),
    }
    Ok(())
}
