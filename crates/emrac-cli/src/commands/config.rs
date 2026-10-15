use emrac_core::{say, Voice};
use std::env;
use std::process::Command;

use emrac_core::{Config, Error, Result};

use crate::cli::ConfigCommand;

pub fn run(command: &ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show => {
            let config = Config::load()?;
            print!("{}", config.to_pretty_string());
        }
        ConfigCommand::Get { key } => {
            let config = Config::load()?;
            println!("{}", config.get(key)?);
        }
        ConfigCommand::Set { key, value } => {
            let mut config = Config::load()?;
            config.set(key, value)?;
            config.save()?;
            say!(Voice::Confirms, "{key} = {value}");
        }
        ConfigCommand::Unset { key } => {
            let mut config = Config::load()?;
            config.unset(key)?;
            config.save()?;
            say!(Voice::Confirms, "{key} reset to its default");
        }
        ConfigCommand::Refresh => refresh()?,
        ConfigCommand::Edit => edit()?,
        ConfigCommand::Validate => match Config::load() {
            Ok(_) => say!(Voice::Confirms, "config is valid"),
            Err(err) => return Err(err),
        },
    }
    Ok(())
}

/// Rewrites the file in the current style when it is not already; values are
/// kept. Returns whether it wrote anything.
fn restyle() -> Result<bool> {
    let path = Config::path()?;
    let config = Config::load()?;
    let styled = config.to_pretty_string();
    if std::fs::read_to_string(&path).is_ok_and(|now| now == styled) {
        return Ok(false);
    }
    config.save()?;
    Ok(true)
}

fn refresh() -> Result<()> {
    if restyle()? {
        say!(
            Voice::Confirms,
            "{} rewritten in the current style; every value is kept.",
            Config::path()?.display()
        );
    } else {
        say!(
            Voice::Notes,
            "the config file is already in the current style."
        );
    }
    Ok(())
}

fn edit() -> Result<()> {
    // Make sure a valid file exists first, so $EDITOR isn't opening nothing
    // and `config validate` has something sane to fall back on if the edit
    // itself introduces a mistake.
    let path = Config::path()?;
    if !path.exists() {
        Config::default().save()?;
    } else if Config::load().is_ok() {
        // An older plain file takes the current style before it is opened.
        restyle()?;
    }

    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|err| Error::CommandSpawn(editor.clone(), err))?;
    if !status.success() {
        return Err(Error::CommandFailed {
            command: format!("{editor} {}", path.display()),
            stderr: format!("exited with {status}"),
        });
    }

    match Config::load() {
        Ok(_) => say!(Voice::Confirms, "config is valid"),
        Err(err) => {
            say!(Voice::Says, "{err} — your edit was kept as-is; fix it and re-run `emrac config edit` or `emrac config validate`.");
        }
    }
    Ok(())
}
