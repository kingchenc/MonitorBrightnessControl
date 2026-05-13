//! Windows backend.
//!
//! Strategy:
//!
//! * **External monitors** — Low-Level Monitor Configuration API (`dxva2.dll`):
//!   `EnumDisplayMonitors` → `GetPhysicalMonitorsFromHMONITOR` → cached
//!   `HANDLE` per monitor → `SetVCPFeature` / `GetVCPFeatureAndVCPFeatureReply`
//!   for arbitrary VCP codes.
//!
//!   Handles are cached in `Manager::cache` so subsequent operations skip the
//!   ~200 ms enumeration cost.
//!
//! * **Internal panel** — WMI `WmiMonitorBrightnessMethods` /
//!   `WmiMonitorBrightness` under `root\WMI` via the high-level `wmi` crate.
//!
//! * **Hot-plug** — the manager exposes [`Manager::invalidate_cache`] which
//!   drops cached handles; the application layer wires this to
//!   `WM_DEVICECHANGE`.

use std::sync::Arc;

use std::sync::mpsc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Devices::Display::{
    CapabilitiesRequestAndCapabilitiesReply, DestroyPhysicalMonitor, GetCapabilitiesStringLength,
    GetMonitorCapabilities, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetVCPFeature,
    MC_CAPS_BRIGHTNESS, MC_VCP_CODE_TYPE, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{BOOL, HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
    MONITORINFOEXW,
};

/// `EDD_GET_DEVICE_INTERFACE_NAME` flag: not exposed by `windows` 0.58.
/// Defined in `<wingdi.h>` as `0x00000001`.
const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x0000_0001;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE,
};
use wmi::{COMLibrary, WMIConnection};

use crate::edid;

use crate::caps::{self, Capabilities};
use crate::error::{Error, Result};
use crate::monitor::{Monitor, MonitorHandle, MonitorId, MonitorInfo, MonitorKind, MonitorManager};
use crate::vcp::VcpValue;

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Top-level Windows manager. Caches enumerated monitors so each operation
/// pays I/O only the first time.
pub struct Manager {
    cache: Mutex<Option<Vec<Monitor>>>,
}

impl Manager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cache: Mutex::new(None),
        })
    }

    /// Drop cached monitors. The next `list()` call will re-enumerate. Used by
    /// the application when `WM_DEVICECHANGE` fires.
    pub fn invalidate_cache(&self) {
        *self.cache.lock() = None;
    }

    fn enumerate(&self) -> Result<Vec<Monitor>> {
        let mut external = enumerate_physical_monitors()?;
        let mut out: Vec<Monitor> = Vec::new();

        match WmiBacklight::open() {
            Ok(Some(internal)) => out.push(Arc::new(internal)),
            Ok(None) => {
                log::debug!("no internal WmiMonitorBrightness instance");
            }
            Err(e) => {
                log::warn!("internal panel WMI init failed: {e}");
            }
        }

        out.append(&mut external);
        Ok(out)
    }
}

impl MonitorManager for Manager {
    fn list(&self) -> Result<Vec<Monitor>> {
        if let Some(cached) = self.cache.lock().clone() {
            return Ok(cached);
        }
        let fresh = self.enumerate()?;
        *self.cache.lock() = Some(fresh.clone());
        Ok(fresh)
    }

    fn refresh(&self) -> Result<Vec<Monitor>> {
        self.invalidate_cache();
        self.list()
    }
}

// ---------------------------------------------------------------------------
// External monitors via Low-Level Monitor Configuration API
// ---------------------------------------------------------------------------

/// One external monitor, holding an owned `HANDLE` from
/// `GetPhysicalMonitorsFromHMONITOR`. `HANDLE` is closed by
/// `DestroyPhysicalMonitor` when the struct is dropped.
struct PhysicalMonitor {
    handle: HANDLE,
    info: MonitorInfo,
    /// Mutex serializes DDC/CI access — concurrent `SetVCPFeature` calls on
    /// the same handle would confuse the display.
    lock: Mutex<()>,
}

// SAFETY: `HANDLE` is a thin wrapper around an integer pointer; the
// underlying object is safe to use across threads when access is serialized
// (which we do via `lock`).
unsafe impl Send for PhysicalMonitor {}
unsafe impl Sync for PhysicalMonitor {}

impl Drop for PhysicalMonitor {
    fn drop(&mut self) {
        // SAFETY: `handle` was obtained from GetPhysicalMonitorsFromHMONITOR
        // and is dropped exactly once.
        unsafe {
            let _ = DestroyPhysicalMonitor(self.handle);
        }
    }
}

impl MonitorHandle for PhysicalMonitor {
    fn info(&self) -> &MonitorInfo {
        &self.info
    }

    fn get_brightness_percent(&self) -> Result<f32> {
        let v = self.get_vcp(crate::vcp::VcpFeature::Luminance.code())?;
        Ok(v.percent())
    }

    fn set_brightness_percent(&self, percent: f32) -> Result<()> {
        // Re-read the maximum so we always send the right scale, even for
        // exotic monitors that don't max at 100.
        let cur = self.get_vcp(crate::vcp::VcpFeature::Luminance.code())?;
        let abs = cur.percent_to_absolute(percent);
        self.set_vcp(crate::vcp::VcpFeature::Luminance.code(), abs)
    }

    fn get_vcp(&self, code: u8) -> Result<VcpValue> {
        let _g = self.lock.lock();
        let mut vcp_code_type: MC_VCP_CODE_TYPE = MC_VCP_CODE_TYPE::default();
        let mut current: u32 = 0;
        let mut maximum: u32 = 0;
        // SAFETY: All pointers are valid stack locations; `self.handle` is
        // valid for the duration of `self`.
        let ok = unsafe {
            GetVCPFeatureAndVCPFeatureReply(
                self.handle,
                code,
                Some(&mut vcp_code_type),
                &mut current,
                Some(&mut maximum),
            )
        };
        if ok == 0 {
            return Err(Error::Platform(format!(
                "GetVCPFeatureAndVCPFeatureReply(0x{code:02X}) failed"
            )));
        }
        Ok(VcpValue::new(current as u16, maximum as u16))
    }

    fn set_vcp(&self, code: u8, value: u16) -> Result<()> {
        let _g = self.lock.lock();
        // SAFETY: `self.handle` is owned and valid; SetVCPFeature does not
        // retain any pointer.
        let ok = unsafe { SetVCPFeature(self.handle, code, value as u32) };
        if ok == 0 {
            return Err(Error::Platform(format!(
                "SetVCPFeature(0x{code:02X}, {value}) failed"
            )));
        }
        Ok(())
    }

    fn capabilities(&self) -> Result<Capabilities> {
        let _g = self.lock.lock();
        let mut len: u32 = 0;
        // SAFETY: `self.handle` is owned and the out pointer is a stack local.
        let ok = unsafe { GetCapabilitiesStringLength(self.handle, &mut len) };
        if ok == 0 || len == 0 {
            return Err(Error::Unsupported("display has no capability string"));
        }
        let mut buf: Vec<u8> = vec![0; len as usize];
        // SAFETY: `buf` is a valid writable buffer of `len` bytes.
        let ok =
            unsafe { CapabilitiesRequestAndCapabilitiesReply(self.handle, buf.as_mut_slice()) };
        if ok == 0 {
            return Err(Error::Platform(
                "CapabilitiesRequestAndCapabilitiesReply failed".into(),
            ));
        }
        if let Some(pos) = buf.iter().position(|b| *b == 0) {
            buf.truncate(pos);
        }
        let s = String::from_utf8_lossy(&buf).to_string();
        Ok(caps::parse(&s))
    }
}

/// Walk every HMONITOR returned by `EnumDisplayMonitors`, attach physical
/// monitors and wrap each as a `MonitorHandle`.
fn enumerate_physical_monitors() -> Result<Vec<Monitor>> {
    struct EnumCtx {
        hmonitors: Vec<(HMONITOR, String)>,
    }

    extern "system" fn cb(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        // SAFETY: `lparam` is the `&mut EnumCtx` we passed in.
        let ctx = unsafe { &mut *(lparam.0 as *mut EnumCtx) };
        let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` is a stack-allocated MONITORINFOEX of the right size;
        // the W variant lays out as MONITORINFO followed by a UTF-16 device
        // name buffer.
        let ok = unsafe { GetMonitorInfoW(hmonitor, &mut info.monitorInfo as *mut _ as *mut _) };
        if ok.as_bool() {
            let device = String::from_utf16_lossy(
                &info
                    .szDevice
                    .iter()
                    .copied()
                    .take_while(|c| *c != 0)
                    .collect::<Vec<_>>(),
            );
            ctx.hmonitors.push((hmonitor, device));
        }
        BOOL(1)
    }

    let mut ctx = EnumCtx {
        hmonitors: Vec::new(),
    };
    // SAFETY: callback only writes to `ctx` and returns before
    // EnumDisplayMonitors returns.
    let ok = unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(cb),
            LPARAM(&mut ctx as *mut _ as isize),
        )
    };
    if !ok.as_bool() {
        return Err(Error::Platform("EnumDisplayMonitors failed".into()));
    }

    let mut out: Vec<Monitor> = Vec::new();
    for (hmon, device) in ctx.hmonitors {
        let mut count: u32 = 0;
        // SAFETY: `&mut count` is a valid stack pointer.
        let res = unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) };
        if res.is_err() || count == 0 {
            continue;
        }
        let mut phys: Vec<PHYSICAL_MONITOR> = vec![PHYSICAL_MONITOR::default(); count as usize];
        // SAFETY: `phys.len() == count` as required by the API.
        let res = unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, &mut phys[..]) };
        if res.is_err() {
            continue;
        }
        // Look up the per-adapter monitor child once: this gives us the
        // PNP DeviceID we need to read EDID from the registry.
        let edid_for_adapter = lookup_edid_for_adapter(&device);

        for (idx, p) in phys.into_iter().enumerate() {
            // PHYSICAL_MONITOR is `#[repr(C, packed(2))]` in windows-rs, so we
            // must read the description via an unaligned pointer to avoid UB.
            let desc: [u16; 128] = {
                // SAFETY: `p` lives until the end of this loop iteration; the
                // pointer points to a valid, initialized field.
                let ptr = std::ptr::addr_of!(p.szPhysicalMonitorDescription);
                unsafe { ptr.read_unaligned() }
            };
            let raw_name = String::from_utf16_lossy(
                &desc
                    .iter()
                    .copied()
                    .take_while(|c| *c != 0)
                    .collect::<Vec<_>>(),
            );
            // Prefer the EDID-derived friendly name when we have one; fall
            // back to PHYSICAL_MONITOR's "Generic PnP Monitor" string.
            let edid_match = edid_for_adapter.get(idx).cloned().flatten();
            let display_name = match &edid_match {
                Some(e) if !e.model_name.is_empty() => {
                    if !e.manufacturer.is_empty() {
                        format!("{} {}", e.manufacturer, e.model_name)
                    } else {
                        e.model_name.clone()
                    }
                }
                _ if !raw_name.is_empty() => raw_name,
                _ => format!("Display {}", idx + 1),
            };
            // Probe basic capability bitmask. We do *not* drop monitors that
            // don't claim brightness here — many monitors omit the bit but
            // accept the VCP write anyway. The probe is informational.
            let mut caps_mask: u32 = 0;
            let mut color_caps: u32 = 0;
            // SAFETY: handle is valid; out-pointers are stack locals.
            let _ = unsafe {
                GetMonitorCapabilities(p.hPhysicalMonitor, &mut caps_mask, &mut color_caps)
            };
            let _supports_brightness = caps_mask & MC_CAPS_BRIGHTNESS != 0;
            let info = MonitorInfo {
                id: MonitorId::new(format!("win:{device}#{idx}")),
                name: display_name,
                kind: MonitorKind::External,
                manufacturer: edid_match
                    .as_ref()
                    .filter(|e| !e.manufacturer.is_empty())
                    .map(|e| e.manufacturer.clone()),
                model: edid_match
                    .as_ref()
                    .filter(|e| !e.model_name.is_empty())
                    .map(|e| e.model_name.clone()),
                capabilities: None,
            };
            out.push(Arc::new(PhysicalMonitor {
                handle: p.hPhysicalMonitor,
                info,
                lock: Mutex::new(()),
            }));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// EDID lookup via EnumDisplayDevices + Registry
// ---------------------------------------------------------------------------

/// Walk every monitor child of the GDI display adapter `device` (e.g.
/// `\\.\DISPLAY1`) and return their parsed EDIDs in enumeration order. The
/// list is parallel to `GetPhysicalMonitorsFromHMONITOR`'s output for the
/// same adapter — i.e. index `i` here matches index `i` of the physical
/// monitors array.
fn lookup_edid_for_adapter(adapter_device: &str) -> Vec<Option<edid::Edid>> {
    let mut out = Vec::new();
    let adapter_w = HSTRING::from(adapter_device);
    let mut idx: u32 = 0;
    loop {
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: dd is a valid DISPLAY_DEVICEW; PCWSTR points to NUL-terminated UTF-16.
        let ok = unsafe {
            EnumDisplayDevicesW(
                PCWSTR(adapter_w.as_ptr()),
                idx,
                &mut dd,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
        };
        if !ok.as_bool() {
            break;
        }
        let device_id = wstr_to_string(&dd.DeviceID);
        out.push(read_edid_for_pnp_path(&device_id));
        idx += 1;
    }
    out
}

/// Convert the device-interface path EnumDisplayDevices returns —
///   `\\?\DISPLAY#DELA0E5#5&abc&0&UID0_0#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}`
/// — into the registry path
///   `SYSTEM\CurrentControlSet\Enum\DISPLAY\DELA0E5\5&abc&0&UID0_0`
/// then read its `Device Parameters\EDID` blob and parse it.
fn read_edid_for_pnp_path(interface_path: &str) -> Option<edid::Edid> {
    let stripped = interface_path
        .strip_prefix(r"\\?\")
        .or_else(|| interface_path.strip_prefix(r"\\.\"))
        .unwrap_or(interface_path);
    // Drop the `#{guid}` interface-class suffix.
    let without_iface = match stripped.rfind('#') {
        Some(i) if stripped[i..].starts_with("#{") => &stripped[..i],
        _ => stripped,
    };
    let registry_relative = without_iface.replace('#', "\\");
    let full = format!(
        r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters",
        registry_relative
    );
    let bytes = read_registry_binary(HKEY_LOCAL_MACHINE, &full, "EDID")?;
    edid::parse(&bytes)
}

fn read_registry_binary(root: HKEY, subkey: &str, value_name: &str) -> Option<Vec<u8>> {
    let subkey_w = HSTRING::from(subkey);
    let value_w = HSTRING::from(value_name);
    let mut hkey: HKEY = HKEY::default();
    // SAFETY: subkey_w outlives the call; KEY_READ is a valid access.
    let kr = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
    };
    if kr.is_err() {
        return None;
    }
    let mut len: u32 = 0;
    let mut value_type: REG_VALUE_TYPE = REG_VALUE_TYPE::default();
    // First call to learn buffer size.
    // SAFETY: hkey valid until RegCloseKey; out pointers are stack locals.
    let _ = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_w.as_ptr()),
            None,
            Some(&mut value_type),
            None,
            Some(&mut len),
        )
    };
    if len == 0 {
        // SAFETY: hkey was opened above.
        unsafe { let _ = RegCloseKey(hkey); };
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: buf has `len` bytes.
    let kr = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_w.as_ptr()),
            None,
            Some(&mut value_type),
            Some(buf.as_mut_ptr()),
            Some(&mut len),
        )
    };
    // SAFETY: hkey is open.
    unsafe { let _ = RegCloseKey(hkey); };
    if kr.is_err() {
        return None;
    }
    buf.truncate(len as usize);
    Some(buf)
}

fn wstr_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------------------
// Internal panel via WMI (`root\WMI`)
// ---------------------------------------------------------------------------
//
// `WMIConnection` (and the underlying `IWbemServices`) is COM-apartment-bound
// and not `Send`. To expose a thread-safe handle we move the connection into
// a dedicated worker thread and route operations through a synchronous
// channel. This costs one OS thread per laptop panel (i.e. one) and avoids
// every Send/Sync hazard around COM.

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorBrightness {
    #[allow(dead_code)]
    instance_name: String,
    current_brightness: u8,
}

#[derive(Deserialize)]
#[serde(rename = "WmiMonitorBrightnessMethods")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorBrightnessMethods {
    #[serde(rename = "__Path")]
    path: String,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct WmiSetBrightnessIn {
    Timeout: u32,
    Brightness: u8,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct WmiSetBrightnessOut {
    #[allow(dead_code)]
    ReturnValue: u32,
}

enum WmiCmd {
    Read(mpsc::Sender<Result<u8>>),
    Write(u8, mpsc::Sender<Result<()>>),
    Shutdown,
}

struct WmiBacklight {
    info: MonitorInfo,
    tx: mpsc::Sender<WmiCmd>,
    /// We only set `joined = true` after sending Shutdown so the worker thread
    /// has a chance to drop COM cleanly.
    joined: Mutex<bool>,
}

impl WmiBacklight {
    fn open() -> Result<Option<Self>> {
        let (tx, rx) = mpsc::channel::<WmiCmd>();
        // First, probe synchronously: spawn a temporary thread, do one
        // SELECT, return whether an instance exists.
        let (probe_tx, probe_rx) = mpsc::channel::<Result<bool>>();
        let probe_tx_clone = probe_tx.clone();
        std::thread::Builder::new()
            .name("wmi-backlight".into())
            .spawn(move || {
                let com = match COMLibrary::new() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = probe_tx_clone
                            .send(Err(Error::Platform(format!("COMLibrary init: {e}"))));
                        return;
                    }
                };
                let conn = match WMIConnection::with_namespace_path(r"root\WMI", com) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = probe_tx_clone
                            .send(Err(Error::Platform(format!("WMI connect: {e}"))));
                        return;
                    }
                };
                let probe: std::result::Result<Vec<WmiMonitorBrightness>, _> =
                    conn.raw_query("SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness");
                match probe {
                    Ok(rows) if rows.is_empty() => {
                        let _ = probe_tx_clone.send(Ok(false));
                        return;
                    }
                    Err(e) => {
                        let _ = probe_tx_clone
                            .send(Err(Error::Platform(format!("WMI query: {e}"))));
                        return;
                    }
                    Ok(_) => {}
                }
                let _ = probe_tx_clone.send(Ok(true));
                wmi_worker(conn, rx);
            })
            .map_err(|e| Error::Platform(format!("spawn wmi-backlight thread: {e}")))?;

        match probe_rx.recv() {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return Ok(None),
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(Error::Platform("WMI worker died during probe".into())),
        }

        let info = MonitorInfo {
            id: MonitorId::new("win:internal:0"),
            name: "Internal Display".to_string(),
            kind: MonitorKind::Internal,
            manufacturer: None,
            model: None,
            capabilities: None,
        };
        Ok(Some(Self {
            info,
            tx,
            joined: Mutex::new(false),
        }))
    }
}

impl Drop for WmiBacklight {
    fn drop(&mut self) {
        let mut joined = self.joined.lock();
        if !*joined {
            *joined = true;
            let _ = self.tx.send(WmiCmd::Shutdown);
        }
    }
}

fn wmi_worker(conn: WMIConnection, rx: mpsc::Receiver<WmiCmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            WmiCmd::Shutdown => break,
            WmiCmd::Read(reply) => {
                let res = read_brightness(&conn);
                let _ = reply.send(res);
            }
            WmiCmd::Write(p, reply) => {
                let res = write_brightness(&conn, p);
                let _ = reply.send(res);
            }
        }
    }
}

fn read_brightness(conn: &WMIConnection) -> Result<u8> {
    let rows: Vec<WmiMonitorBrightness> = conn
        .raw_query("SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness")
        .map_err(|e| Error::Platform(format!("WMI re-query brightness: {e}")))?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| Error::NotFound("internal panel disappeared".into()))?;
    Ok(row.current_brightness)
}

fn write_brightness(conn: &WMIConnection, percent: u8) -> Result<()> {
    let methods: Vec<WmiMonitorBrightnessMethods> = conn
        .raw_query("SELECT __Path FROM WmiMonitorBrightnessMethods")
        .map_err(|e| Error::Platform(format!("WMI lookup methods instance: {e}")))?;
    let m = methods
        .into_iter()
        .next()
        .ok_or_else(|| Error::NotFound("WmiMonitorBrightnessMethods has no instance".into()))?;
    let in_params = WmiSetBrightnessIn {
        Timeout: 1,
        Brightness: percent.min(100),
    };
    let _: WmiSetBrightnessOut = conn
        .exec_instance_method::<WmiMonitorBrightnessMethods, _, _>(
            "WmiSetBrightness",
            &m.path,
            in_params,
        )
        .map_err(|e| Error::Platform(format!("WMI WmiSetBrightness: {e}")))?;
    Ok(())
}

impl MonitorHandle for WmiBacklight {
    fn info(&self) -> &MonitorInfo {
        &self.info
    }

    fn get_brightness_percent(&self) -> Result<f32> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(WmiCmd::Read(tx))
            .map_err(|_| Error::Platform("WMI worker disconnected".into()))?;
        let v = rx
            .recv()
            .map_err(|_| Error::Platform("WMI worker dropped reply".into()))??;
        Ok(v as f32)
    }

    fn set_brightness_percent(&self, percent: f32) -> Result<()> {
        let p = percent.clamp(0.0, 100.0).round() as u8;
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(WmiCmd::Write(p, tx))
            .map_err(|_| Error::Platform("WMI worker disconnected".into()))?;
        rx.recv()
            .map_err(|_| Error::Platform("WMI worker dropped reply".into()))?
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
        Err(Error::Unsupported("internal panel has no capability string"))
    }
}
