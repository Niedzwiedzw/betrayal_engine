use crate::{
    error::{BetrayalError, BetrayalResult},
    reclass::{config_file::Config, display::Printable},
};
use notify::{DebouncedEvent, RecursiveMode, Watcher, watcher};
use serde_yaml::{from_str, to_string};
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::channel;
use std::{fs::read_to_string, io::Write, path::PathBuf, time::Duration};

pub fn run(pid: i32) -> BetrayalResult<()> {
    println!("running reclass");
    let mut tempfile = tempfile::Builder::new()
        .suffix(".yaml")
        .tempfile()
        .map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;
    let config =
        to_string(&Config::default()).map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;
    write!(tempfile, "{config}").map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;

    // set correct permissions
    let path = PathBuf::from(tempfile.path());
    {
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?
            .permissions();
        perms.set_mode(0o666);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;
    }
    println!(" :: edit [{path:?}] file and see the live output");

    let (tx, rx) = channel();

    // Create a watcher object, delivering raw events.
    // The notification back-end is selected based on the platform.
    let mut watcher = watcher(tx, Duration::from_millis(500)).map_err(|e| {
        BetrayalError::ConfigFileError(format!("failed to spawn a file watcher :: {e}"))
    })?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| {
            BetrayalError::ConfigFileError(format!("failed to spawn a file watcher :: {e}"))
        })?;

    loop {
        match rx.recv() {
            Ok(DebouncedEvent::Write(_)) => {
                let config = read_to_string(&path).map_err(|e| {
                    BetrayalError::ConfigFileError(format!("failed to read config file :: {e}"))
                })?;
                match from_str::<Config>(&config) {
                    Ok(c) => {
                        let result = c.result(pid);
                        match result {
                            Ok(result) => println!("{}", result.print(0)),
                            Err(e) => {
                                eprintln!("ERROR: \n {e}")
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("bad format :: {e}")
                    }
                }
            }
            Err(e) => {
                eprintln!("watch error: {e:?}");
                break;
            }
            _ => {}
        }
    }

    // editor_task
    //     .join()
    //     .map_err(|e| BetrayalError::ConfigFileError(format!("editor closed :: {:?}", e)))??;

    Ok(())
}
