//! Fallback for platforms we do not (yet) target. Lists nothing.

use crate::error::{Error, Result};
use crate::monitor::{Monitor, MonitorManager};

pub struct Manager;

impl Manager {
    pub fn new() -> Result<Self> {
        Err(Error::Unsupported("brightness-core has no backend for this platform"))
    }
}

impl MonitorManager for Manager {
    fn list(&self) -> Result<Vec<Monitor>> {
        Ok(Vec::new())
    }
    fn refresh(&self) -> Result<Vec<Monitor>> {
        Ok(Vec::new())
    }
}
