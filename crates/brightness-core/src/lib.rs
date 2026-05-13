//! Cross-platform monitor brightness, contrast and DDC/CI control.
//!
//! Platform support:
//! * **Windows** — Low-Level Monitor Configuration API (`dxva2`) for external
//!   displays, WMI (`WmiMonitorBrightnessMethods`) for the laptop panel.
//!   Physical-monitor handles are cached and the manager listens to
//!   `WM_DEVICECHANGE` for hot-plug events.
//! * **macOS** — `IOAVService` direct I²C for external displays, the private
//!   `DisplayServices` framework (loaded at runtime) for the built-in panel,
//!   with `CoreDisplay_Display_SetUserBrightness` as fallback.
//! * **Linux** — `/dev/i2c-*` for external displays, `/sys/class/backlight/*`
//!   for internal panels, `udev` for hot-plug.
//!
//! ## Example
//!
//! ```no_run
//! use brightness_core::platform::default_manager;
//! use brightness_core::vcp::VcpFeature;
//!
//! let mgr = default_manager().expect("backend init");
//! for monitor in mgr.list().expect("list") {
//!     let pct = monitor.get_brightness_percent().unwrap_or(50.0);
//!     monitor.set_brightness_percent((pct + 10.0).min(100.0)).ok();
//!     if monitor.info().kind == brightness_core::monitor::MonitorKind::External {
//!         let _ = monitor.get_feature(VcpFeature::Contrast);
//!     }
//! }
//! ```

pub mod caps;
pub mod ddc;
pub mod edid;
pub mod error;
pub mod monitor;
pub mod platform;
pub mod vcp;

pub use caps::Capabilities;
pub use error::{Error, Result};
pub use monitor::{Monitor, MonitorHandle, MonitorId, MonitorInfo, MonitorKind, MonitorManager};
pub use vcp::{ColorPreset, VcpFeature, VcpValue};
