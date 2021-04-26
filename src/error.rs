use thiserror::Error;

#[derive(Error, Debug)]
pub enum BetrayalError {
    #[error("Unable to find a process with specified PID")]
    BadPid,
    #[error(transparent)]
    ProcError(#[from] procfs::ProcError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("Partial read occured - aborting")]
    PartialRead,
}

pub type BetrayalResult<T> = Result<T, BetrayalError>;
