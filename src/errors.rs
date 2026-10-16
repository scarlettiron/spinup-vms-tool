//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The kinds of failure a command can end with. Each one maps to an exit code, so
//scripts can tell "you typed it wrong" from "it isn't there" from "it broke".

use crate::constants::exit_codes;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    //The command line was valid to clap but makes no sense, for example two
    //options that can't be combined.
    #[error("{0}")]
    Usage(String),

    //A project, VM or blueprint that was named does not exist.
    #[error("{0}")]
    NotFound(String),

    //The person pressed Ctrl+C or Esc at a prompt.
    #[error("cancelled")]
    Cancelled,

    //Anything else, with its chain of causes.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Usage(_) => exit_codes::USAGE,
            AppError::NotFound(_) => exit_codes::NOT_FOUND,
            AppError::Cancelled => exit_codes::CANCELLED,
            AppError::Other(_) => exit_codes::FAILURE,
        }
    }
}

//Prompts report Esc and Ctrl+C as errors; turn those into Cancelled so the
//program exits quietly with 130.
impl From<inquire::InquireError> for AppError {
    fn from(error: inquire::InquireError) -> Self {
        match error {
            inquire::InquireError::OperationCanceled
            | inquire::InquireError::OperationInterrupted => AppError::Cancelled,
            other => AppError::Other(anyhow::Error::new(other)),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
