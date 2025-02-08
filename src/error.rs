use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DashboardError {
    #[error("Failed to start server: {0}")]
    ServerError(#[from] tonic::transport::Error),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Channel error: {0}")]
    ChannelError(String),
}
