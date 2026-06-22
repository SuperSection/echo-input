use thiserror::Error;

#[derive(Error, Debug)]
pub enum WaylandError {
    #[error("Wayland connection failed: {0}")]
    Connection(String),

    #[error("Required Wayland protocol not available: {0}")]
    MissingProtocol(String),

    #[error("No monitors found")]
    NoOutputs,

    #[error("Monitor not found: {0}")]
    OutputNotFound(String),

    #[error("SHM allocation failed: {0}")]
    ShmAllocation(String),

    #[error("Cairo rendering error: {0}")]
    Cairo(String),

    #[error("Event loop closed")]
    EventLoopClosed,

    #[error("Channel send failed: {0}")]
    ChannelSend(String),
}
