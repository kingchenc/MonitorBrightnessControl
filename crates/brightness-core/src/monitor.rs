//! Public abstractions: a `Monitor` handle and a `MonitorManager` that
//! enumerates and gives back handles. Each platform provides an implementation
//! under `crate::platform` that exposes types matching this API.

use std::fmt;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::caps::Capabilities;
use crate::error::Result;
use crate::vcp::{VcpFeature, VcpValue};

/// What kind of display this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MonitorKind {
    /// Built-in laptop / iMac / all-in-one panel — uses backlight APIs.
    Internal,
    /// External display — uses DDC/CI over I²C.
    External,
}

impl fmt::Display for MonitorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Internal => "internal",
            Self::External => "external",
        })
    }
}

/// Stable, plaintext identifier for a monitor. The format is platform-defined
/// but guaranteed to remain stable across reboots for the same physical
/// display+port combination so the UI can persist per-monitor settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MonitorId(pub String);

impl MonitorId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MonitorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Static information about a monitor.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    pub kind: MonitorKind,
    /// Manufacturer ID (3-letter PNP code, if known).
    pub manufacturer: Option<String>,
    /// Manufacturer-supplied product name (from EDID), if known.
    pub model: Option<String>,
    /// Decoded capability string contents (only populated after a refresh).
    pub capabilities: Option<Capabilities>,
}

/// Trait every monitor handle implements. Methods are sync because DDC/CI is
/// inherently serial and most callers want to drive them from a worker.
pub trait MonitorHandle: Send + Sync {
    fn info(&self) -> &MonitorInfo;

    /// Read brightness as 0..=100 percent. May be slower than a raw VCP get
    /// because it has to fetch the maximum.
    fn get_brightness_percent(&self) -> Result<f32>;

    /// Write brightness 0..=100 percent. Internally clamped.
    fn set_brightness_percent(&self, percent: f32) -> Result<()>;

    /// Read the current+max value of a VCP code via DDC/CI. Returns
    /// `Error::Unsupported` for internal panels.
    fn get_vcp(&self, code: u8) -> Result<VcpValue>;

    /// Write a 16-bit VCP value via DDC/CI.
    fn set_vcp(&self, code: u8, value: u16) -> Result<()>;

    /// Convenience wrapper over `get_vcp` with a known feature.
    fn get_feature(&self, feature: VcpFeature) -> Result<VcpValue> {
        self.get_vcp(feature.code())
    }

    /// Convenience wrapper over `set_vcp`.
    fn set_feature(&self, feature: VcpFeature, value: u16) -> Result<()> {
        self.set_vcp(feature.code(), value)
    }

    /// Fetch and parse the MCCS capability string, if the device supports it.
    fn capabilities(&self) -> Result<Capabilities>;
}

/// Boxed dynamic handle so the rest of the application can hold one type.
pub type Monitor = Arc<dyn MonitorHandle>;

/// Top-level manager: enumerate, watch hot-plug events, give out handles.
pub trait MonitorManager: Send + Sync {
    fn list(&self) -> Result<Vec<Monitor>>;

    /// Re-enumerate now and return the fresh list. Equivalent to `list` for
    /// platforms that always re-enumerate, but exposed as a separate method to
    /// let callers force a refresh after a hot-plug notification.
    fn refresh(&self) -> Result<Vec<Monitor>>;
}
