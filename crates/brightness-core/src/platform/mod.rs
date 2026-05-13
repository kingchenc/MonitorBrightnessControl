//! Platform implementations of `MonitorManager`.
//!
//! Each module exposes a public type `Manager` implementing
//! [`crate::monitor::MonitorManager`].

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub mod stub;

/// Construct the platform-default manager. Returns an error if the platform
/// is not supported or initialization fails.
pub fn default_manager() -> crate::error::Result<std::sync::Arc<dyn crate::monitor::MonitorManager>>
{
    #[cfg(windows)]
    {
        let m = windows::Manager::new()?;
        Ok(std::sync::Arc::new(m))
    }
    #[cfg(target_os = "macos")]
    {
        let m = macos::Manager::new()?;
        Ok(std::sync::Arc::new(m))
    }
    #[cfg(target_os = "linux")]
    {
        let m = linux::Manager::new()?;
        Ok(std::sync::Arc::new(m))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let m = stub::Manager::new()?;
        Ok(std::sync::Arc::new(m))
    }
}
