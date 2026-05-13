use thiserror::Error;

/// Errors returned by the brightness control core.
#[derive(Debug, Error)]
pub enum Error {
    #[error("display not found: {0}")]
    NotFound(String),

    #[error("operation not supported on this monitor or platform: {0}")]
    Unsupported(&'static str),

    #[error("DDC/CI protocol error: {0}")]
    Protocol(String),

    #[error("DDC/CI checksum mismatch")]
    Checksum,

    #[error("invalid VCP value {value} for code {code:#04x} (max {max})")]
    InvalidValue { code: u8, value: u16, max: u16 },

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("platform error: {0}")]
    Platform(String),

    #[error("timeout waiting for monitor response")]
    Timeout,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
