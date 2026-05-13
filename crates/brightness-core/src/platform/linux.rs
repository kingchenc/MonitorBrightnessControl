//! Linux backend.
//!
//! Strategy:
//!
//! * **External monitors** — `/dev/i2c-*` character devices. We open every
//!   `i2c-N` whose name (read via `name` sysfs attribute) contains a known
//!   "DDC" or "card[0-9]+-..." token, set the slave address to `0x37`, then
//!   talk DDC/CI using the wire frames built in [`crate::ddc`].
//!
//!   Permissions: by default, `/dev/i2c-*` requires the `i2c` group. The
//!   distribution packaging ships a `udev` rule that adds the binary's user
//!   to that group. Running as root also works but is discouraged.
//!
//! * **Internal panel** — `/sys/class/backlight/<dev>/brightness` paired with
//!   `max_brightness`. Writing requires either udev permissions or
//!   `pkexec`/`logind`-issued capability. The application ships a
//!   `90-monitor-brightness.rules` udev rule granting write access to the
//!   `video` group.
//!
//! * **Hot-plug** — `udev::MonitorBuilder` watches the `drm` and `i2c-dev`
//!   subsystems; `Manager::invalidate_cache` is wired to the udev event loop
//!   in the application layer.

use std::ffi::CStr;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::caps::{self, Capabilities};
use crate::ddc::{
    self, decode_capabilities_reply, decode_get_vcp_reply, DDC_ADDR, MIN_INTERVAL_MS,
    VCP_REQUEST_REPLY_DELAY_MS,
};
use crate::error::{Error, Result};
use crate::monitor::{Monitor, MonitorHandle, MonitorId, MonitorInfo, MonitorKind, MonitorManager};
use crate::vcp::VcpValue;

// ---------------------------------------------------------------------------
// I²C ioctl bindings
// ---------------------------------------------------------------------------
//
// `<linux/i2c-dev.h>` and `<linux/i2c.h>`:
//   #define I2C_SLAVE       0x0703
//   #define I2C_RDWR        0x0707
const I2C_SLAVE: libc::c_ulong = 0x0703;
#[allow(dead_code)]
const I2C_RDWR: libc::c_ulong = 0x0707;

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct Manager {
    cache: Mutex<Option<Vec<Monitor>>>,
}

impl Manager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cache: Mutex::new(None),
        })
    }

    pub fn invalidate_cache(&self) {
        *self.cache.lock() = None;
    }
}

impl MonitorManager for Manager {
    fn list(&self) -> Result<Vec<Monitor>> {
        if let Some(cached) = self.cache.lock().clone() {
            return Ok(cached);
        }
        let monitors = enumerate()?;
        *self.cache.lock() = Some(monitors.clone());
        Ok(monitors)
    }

    fn refresh(&self) -> Result<Vec<Monitor>> {
        self.invalidate_cache();
        self.list()
    }
}

fn enumerate() -> Result<Vec<Monitor>> {
    let mut out: Vec<Monitor> = Vec::new();

    for backlight in enumerate_backlights() {
        out.push(Arc::new(backlight));
    }
    for ext in enumerate_i2c_displays() {
        out.push(Arc::new(ext));
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Internal panels via /sys/class/backlight
// ---------------------------------------------------------------------------

struct BacklightDisplay {
    info: MonitorInfo,
    path: PathBuf,
    max_brightness: u32,
    /// Serializes read-modify-write cycles.
    lock: Mutex<()>,
}

impl BacklightDisplay {
    fn open(dir: &Path) -> Option<Self> {
        let max = read_u32(&dir.join("max_brightness")).ok()?;
        if max == 0 {
            return None;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("backlight")
            .to_string();
        let info = MonitorInfo {
            id: MonitorId::new(format!("linux:backlight:{name}")),
            name: format!("Internal Display ({name})"),
            kind: MonitorKind::Internal,
            manufacturer: None,
            model: None,
            capabilities: None,
        };
        Some(Self {
            info,
            path: dir.to_path_buf(),
            max_brightness: max,
            lock: Mutex::new(()),
        })
    }
}

impl MonitorHandle for BacklightDisplay {
    fn info(&self) -> &MonitorInfo {
        &self.info
    }

    fn get_brightness_percent(&self) -> Result<f32> {
        let cur = read_u32(&self.path.join("brightness"))?;
        Ok((cur as f32 / self.max_brightness as f32) * 100.0)
    }

    fn set_brightness_percent(&self, percent: f32) -> Result<()> {
        let _g = self.lock.lock();
        let pct = percent.clamp(0.0, 100.0);
        let abs = ((pct / 100.0) * self.max_brightness as f32).round() as u32;
        let abs = abs.min(self.max_brightness);
        write_u32(&self.path.join("brightness"), abs)
    }

    fn get_vcp(&self, _code: u8) -> Result<VcpValue> {
        Err(Error::Unsupported(
            "internal panel does not support DDC/CI VCP",
        ))
    }
    fn set_vcp(&self, _code: u8, _value: u16) -> Result<()> {
        Err(Error::Unsupported(
            "internal panel does not support DDC/CI VCP",
        ))
    }
    fn capabilities(&self) -> Result<Capabilities> {
        Err(Error::Unsupported(
            "internal panel has no capability string",
        ))
    }
}

fn enumerate_backlights() -> Vec<BacklightDisplay> {
    let mut out = Vec::new();
    let dir = Path::new("/sys/class/backlight");
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(b) = BacklightDisplay::open(&path) {
            out.push(b);
        }
    }
    out
}

fn read_u32(path: &Path) -> Result<u32> {
    let s = fs::read_to_string(path).map_err(Error::Io)?;
    s.trim()
        .parse::<u32>()
        .map_err(|e| Error::Platform(format!("parse {}: {e}", path.display())))
}

fn write_u32(path: &Path, value: u32) -> Result<()> {
    fs::write(path, value.to_string()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            Error::PermissionDenied(format!(
                "{} (add user to 'video' group or install udev rule)",
                path.display()
            ))
        } else {
            Error::Io(e)
        }
    })
}

// ---------------------------------------------------------------------------
// External displays via /dev/i2c-*
// ---------------------------------------------------------------------------

struct I2cDisplay {
    info: MonitorInfo,
    /// Open file descriptor for the i2c-dev device. Wrapped in a Mutex
    /// because each ioctl mutates the kernel's slave-address state.
    fd: Mutex<std::fs::File>,
}

impl I2cDisplay {
    fn open(dev: &Path, friendly: &str) -> Option<Self> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(dev)
            .ok()?;
        // SAFETY: ioctl(I2C_SLAVE, addr) sets a per-fd slave address; valid
        // for this fd until close.
        let kr = unsafe { libc::ioctl(f.as_raw_fd(), I2C_SLAVE as _, DDC_ADDR as libc::c_int) };
        if kr < 0 {
            return None;
        }
        let info = MonitorInfo {
            id: MonitorId::new(format!("linux:{}", dev.display())),
            name: friendly.to_string(),
            kind: MonitorKind::External,
            manufacturer: None,
            model: None,
            capabilities: None,
        };
        Some(Self {
            info,
            fd: Mutex::new(f),
        })
    }

    fn write_frame(&self, frame: &[u8]) -> Result<()> {
        let mut fd = self.fd.lock();
        // The first byte of `frame` is SRC (0x51), which the kernel's i2c-dev
        // does NOT prepend automatically when using the simple write/read
        // path — but DDC/CI expects SRC to be on the wire. We therefore write
        // the entire frame (SRC..CHK) directly. The kernel sends the slave
        // address byte before our payload as part of the I²C transaction.
        fd.write_all(frame).map_err(Error::Io)?;
        fd.flush().map_err(Error::Io)?;
        Ok(())
    }

    fn read_reply(&self, expected_len: usize) -> Result<Vec<u8>> {
        let mut fd = self.fd.lock();
        let mut buf = vec![0u8; expected_len];
        fd.read_exact(&mut buf).map_err(Error::Io)?;
        Ok(buf)
    }

    fn rewind(&self) {
        // Some i2c-dev implementations expose a position cursor; resetting is
        // a defensive no-op for character devices.
        if let Ok(mut fd) = self.fd.try_lock() {
            let _ = fd.seek(SeekFrom::Start(0));
        }
    }
}

impl MonitorHandle for I2cDisplay {
    fn info(&self) -> &MonitorInfo {
        &self.info
    }

    fn get_brightness_percent(&self) -> Result<f32> {
        let v = self.get_vcp(crate::vcp::VcpFeature::Luminance.code())?;
        Ok(v.percent())
    }

    fn set_brightness_percent(&self, percent: f32) -> Result<()> {
        let cur = self.get_vcp(crate::vcp::VcpFeature::Luminance.code())?;
        let abs = cur.percent_to_absolute(percent);
        self.set_vcp(crate::vcp::VcpFeature::Luminance.code(), abs)
    }

    fn get_vcp(&self, code: u8) -> Result<VcpValue> {
        let frame = ddc::encode_get_vcp(code);
        self.write_frame(&frame)?;
        std::thread::sleep(Duration::from_millis(VCP_REQUEST_REPLY_DELAY_MS));
        let raw = self.read_reply(11)?;
        let reply = decode_get_vcp_reply(&raw)?;
        std::thread::sleep(Duration::from_millis(MIN_INTERVAL_MS));
        Ok(VcpValue::new(reply.current, reply.maximum))
    }

    fn set_vcp(&self, code: u8, value: u16) -> Result<()> {
        let frame = ddc::encode_set_vcp(code, value);
        self.write_frame(&frame)?;
        std::thread::sleep(Duration::from_millis(MIN_INTERVAL_MS));
        Ok(())
    }

    fn capabilities(&self) -> Result<Capabilities> {
        let mut all = Vec::<u8>::new();
        let mut offset: u16 = 0;
        for _ in 0..64 {
            let frame = ddc::encode_capabilities_request(offset);
            self.write_frame(&frame)?;
            std::thread::sleep(Duration::from_millis(VCP_REQUEST_REPLY_DELAY_MS));
            let raw = self.read_reply(64)?;
            let frag = decode_capabilities_reply(&raw)?;
            if frag.data.is_empty() {
                break;
            }
            offset += frag.data.len() as u16;
            all.extend_from_slice(&frag.data);
            std::thread::sleep(Duration::from_millis(MIN_INTERVAL_MS));
        }
        let s = String::from_utf8_lossy(&all).to_string();
        Ok(caps::parse(&s))
    }
}

fn enumerate_i2c_displays() -> Vec<I2cDisplay> {
    let mut out = Vec::new();
    // Walk /sys/bus/i2c/devices/i2c-* and pair them with /dev/i2c-N. We accept
    // every adapter whose `name` attribute contains "DDC" — this is the
    // convention for adapters that the DRM subsystem publishes per connector.
    let bus = match fs::read_dir("/sys/bus/i2c/devices") {
        Ok(d) => d,
        Err(_) => return out,
    };
    for entry in bus.flatten() {
        let path = entry.path();
        let dir_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !dir_name.starts_with("i2c-") {
            continue;
        }
        let name = match fs::read_to_string(path.join("name")) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue,
        };
        // Adapters that aren't DDC (e.g. system management bus) lack "DDC".
        if !name.to_uppercase().contains("DDC") && !name.contains("i915 gmbus") {
            continue;
        }
        let dev_node = PathBuf::from(format!("/dev/{}", dir_name));
        if let Some(disp) = I2cDisplay::open(&dev_node, &name) {
            disp.rewind();
            out.push(disp);
        } else {
            log::debug!(
                "/dev/{} could not be opened for DDC/CI (permission?)",
                dir_name
            );
        }
    }
    out
}

#[allow(dead_code)]
fn _silence_unused() {
    let _ = CStr::from_bytes_with_nul(b"\0");
}
