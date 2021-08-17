use crate::{
    error::{BetrayalError, BetrayalResult},
    reclass::config_file::{Config, ReclassStruct},
};
use std::{fs::read_to_string, io::{BufWriter, Write}, path::PathBuf, process::Command, time::Duration};

use notify::{DebouncedEvent, RawEvent, RecursiveMode, Watcher, raw_watcher, watcher};
use serde_yaml::{from_str, to_string};
use std::sync::mpsc::channel;

pub fn run(pid: i32) -> BetrayalResult<()> {
    println!("running reclass");
    let mut tempfile = tempfile::NamedTempFile::new()
        .map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;
    let config =
        to_string(&Config::default()).map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;
    write!(tempfile, "{}", config).map_err(|e| BetrayalError::ConfigFileError(e.to_string()))?;

    let editor = std::env::var("EDITOR").map_err(|e| {
        BetrayalError::ConfigFileError(format!("EDITOR env var is required :: {}", e))
    })?;
    let path = PathBuf::from(tempfile.path().clone());
    let path_for_editor = path.clone();
    let editor_task = std::thread::spawn(|| {
        std::process::Command::new(editor)
            .arg(path_for_editor)
            .output()
            .map_err(|e| BetrayalError::ConfigFileError(format!("editor closed :: {}", e)))
    });

    let (tx, rx) = channel();

    // Create a watcher object, delivering raw events.
    // The notification back-end is selected based on the platform.
    let mut watcher = watcher(tx, Duration::from_millis(500)).map_err(|e| {
        BetrayalError::ConfigFileError(format!("failed to spawn a file watcher :: {}", e))
    })?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| {
            BetrayalError::ConfigFileError(format!("failed to spawn a file watcher :: {}", e))
        })?;

    loop {
        match rx.recv() {
            Ok(DebouncedEvent::Write(_)) => {
                let config = read_to_string(&path).map_err(|e| BetrayalError::ConfigFileError(format!("failed to read config file :: {}", e)))?;
                match from_str::<Config>(&config) {
                    Ok(c) => {
                        let result = c.result(pid);
                        match result {
                            Ok(result) => {
                                match to_string(&result) {
                                    Ok(r) => println!("{}", r),
                                    Err(e) => eprintln!("ERROR: {}", e.to_string()),
                                }
                            },
                            Err(e) => {
                                eprintln!("ERROR: \n {}", e.to_string())
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("bad format :: {}", e)
                    }
                }
            },
            Err(e) => {
                eprintln!("watch error: {:?}", e);
                break;
            },
            _ => {},
        }
    }

    editor_task
        .join()
        .map_err(|e| BetrayalError::ConfigFileError(format!("editor closed :: {:?}", e)))??;

    Ok(())
}
