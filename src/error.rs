use thiserror::Error;

#[derive(Error, Debug)]
pub enum BetrayalError {
    #[error("Unable to find a process with specified PID")]
    BadPid,
    #[error(transparent)]
    ProcError(#[from] procfs::ProcError),
    #[error("Improper command: {0}")]
    BadCommand(String),
    #[error("Partial read occured - aborting")]
    PartialRead,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("memory write resulted in an error {0}")]
    BadWrite(String),
    #[error("problem with the config file :: {0}")]
    ConfigFileError(String),
    #[error("script has some error :: {0}")]
    ScriptingError(String),
}

pub type BetrayalResult<T> = Result<T, BetrayalError>;
