//! macOS backend.
//!
//! Strategy:
//!
//! * **Internal panel** — `DisplayServices.framework` (private) via `dlopen`.
//!   `DisplayServicesSetBrightness` / `DisplayServicesGetBrightness` are what
//!   the OS Brightness slider uses; they survive every macOS release where
//!   `IOFramebuffer`-based shims (e.g. `CoreDisplay_Display_SetUserBrightness`)
//!   eventually break.
//!
//!   We add `CoreDisplay_Display_SetUserBrightness` from `CoreDisplay.framework`
//!   as a fallback to cover early macOS 11.x where DisplayServices' setter is
//!   absent on some hardware.
//!
//! * **External displays** — `IOAVService` from `CoreDisplay.framework`
//!   together with `IOServiceMatching("DCPAVServiceProxy")` (the same path
//!   used by `m1ddc`/MonitorControl). We send DDC/CI frames built by
//!   [`crate::ddc`] over `IOAVServiceWriteI2C`/`IOAVServiceReadI2C`.
//!
//! * **Display enumeration** — `CGGetActiveDisplayList`. `CGDisplayIsBuiltin`
//!   separates internal from external; for externals, we pair each
//!   `CGDirectDisplayID` with a `DCPAVServiceProxy` whose `Location` property
//!   equals `"External"`. The pairing order matches `CGGetActiveDisplayList`'s
//!   non-builtin order, which empirically tracks DCP enumeration on Apple
//!   Silicon.

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::sync::Arc;

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::{CGDisplay, CGDisplayIsBuiltin, CGGetActiveDisplayList};
use libloading::{Library, Symbol};
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
// IOKit / IOAVService raw bindings
// ---------------------------------------------------------------------------

type CGDirectDisplayID = u32;
#[allow(non_camel_case_types)]
type kern_return_t = i32;
#[allow(non_camel_case_types)]
type io_object_t = u32;
#[allow(non_camel_case_types)]
type io_iterator_t = io_object_t;
#[allow(non_camel_case_types)]
type io_service_t = io_object_t;
type IOOptionBits = u32;

const KERN_SUCCESS: kern_return_t = 0;
const IO_OBJECT_NULL: io_object_t = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFTypeRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CFTypeRef,
        existing: *mut io_iterator_t,
    ) -> kern_return_t;
    fn IOIteratorNext(iter: io_iterator_t) -> io_object_t;
    fn IOObjectRelease(obj: io_object_t) -> kern_return_t;
    fn IORegistryEntryCreateCFProperty(
        entry: io_service_t,
        key: CFStringRef,
        allocator: CFTypeRef,
        options: IOOptionBits,
    ) -> CFTypeRef;
    static kIOMainPortDefault: u32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
}

// IOAVService is exposed by `CoreDisplay.framework`. The headers are not
// public; the symbols are stable (used by `m1ddc`, MonitorControl, etc.).
#[link(name = "CoreDisplay", kind = "framework")]
extern "C" {
    fn IOAVServiceCreateWithService(allocator: CFTypeRef, service: io_service_t) -> CFTypeRef;
    fn IOAVServiceWriteI2C(
        service: CFTypeRef,
        chip_address: u32,
        offset: u32,
        data: *const u8,
        size: u32,
    ) -> kern_return_t;
    fn IOAVServiceReadI2C(
        service: CFTypeRef,
        chip_address: u32,
        offset: u32,
        data: *mut u8,
        size: u32,
    ) -> kern_return_t;
}

// ---------------------------------------------------------------------------
// DisplayServices / CoreDisplay private brightness APIs
// ---------------------------------------------------------------------------

/// Lazily-loaded handle bag for the two private frameworks.
struct PrivateFrameworks {
    _display_services: Library,
    ds_get: Symbol<'static, unsafe extern "C" fn(CGDirectDisplayID, *mut f32) -> i32>,
    ds_set: Symbol<'static, unsafe extern "C" fn(CGDirectDisplayID, f32) -> i32>,
    _core_display: Library,
    cd_set: Option<Symbol<'static, unsafe extern "C" fn(CGDirectDisplayID, f64) -> i32>>,
}

impl PrivateFrameworks {
    fn load() -> Result<&'static Self> {
        use once_cell::sync::OnceCell;
        static INIT: OnceCell<Result<PrivateFrameworks>> = OnceCell::new();
        let r = INIT.get_or_init(|| unsafe {
            let display_services = Library::new(
                "/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices",
            )
            .map_err(|e| Error::Platform(format!("dlopen DisplayServices: {e}")))?;
            // SAFETY: We rebind lifetimes via a leak. The Library is kept
            // alive by being moved into the OnceCell value below, so the
            // 'static is valid for the process.
            let ds_get_raw: Symbol<unsafe extern "C" fn(CGDirectDisplayID, *mut f32) -> i32> =
                display_services
                    .get(b"DisplayServicesGetBrightness\0")
                    .map_err(|e| Error::Platform(format!("DisplayServicesGetBrightness: {e}")))?;
            let ds_set_raw: Symbol<unsafe extern "C" fn(CGDirectDisplayID, f32) -> i32> =
                display_services
                    .get(b"DisplayServicesSetBrightness\0")
                    .map_err(|e| Error::Platform(format!("DisplayServicesSetBrightness: {e}")))?;

            let core_display =
                Library::new("/System/Library/Frameworks/CoreDisplay.framework/CoreDisplay")
                    .map_err(|e| Error::Platform(format!("dlopen CoreDisplay: {e}")))?;
            let cd_set_raw: Option<Symbol<unsafe extern "C" fn(CGDirectDisplayID, f64) -> i32>> =
                core_display
                    .get(b"CoreDisplay_Display_SetUserBrightness\0")
                    .ok();

            // Transmute lifetimes — they remain valid as long as the
            // corresponding `Library` is alive, which we keep next to the
            // symbols in the same struct.
            let ds_get: Symbol<'static, _> = std::mem::transmute(ds_get_raw);
            let ds_set: Symbol<'static, _> = std::mem::transmute(ds_set_raw);
            let cd_set: Option<Symbol<'static, _>> = cd_set_raw.map(|s| std::mem::transmute(s));

            Ok(PrivateFrameworks {
                _display_services: display_services,
                ds_get,
                ds_set,
                _core_display: core_display,
                cd_set,
            })
        });
        match r {
            Ok(fw) => Ok(fw),
            Err(e) => Err(Error::Platform(format!("private framework init: {e}"))),
        }
    }
}

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
    let display_ids = active_displays()?;
    let mut external_av = enumerate_external_av_services();

    let mut out: Vec<Monitor> = Vec::new();
    for id in display_ids {
        // SAFETY: id was returned by CGGetActiveDisplayList.
        let is_builtin = unsafe { CGDisplayIsBuiltin(id) } != 0;
        if is_builtin {
            out.push(Arc::new(InternalDisplay::new(id)));
        } else if let Some(av) = external_av.pop() {
            out.push(Arc::new(ExternalDisplay::new(id, av)));
        } else {
            log::warn!(
                "external display {id} has no matching DCPAVServiceProxy; \
                DDC/CI will be unavailable for it"
            );
        }
    }
    Ok(out)
}

fn active_displays() -> Result<Vec<CGDirectDisplayID>> {
    // First call to learn the count, then a sized call.
    let mut count: u32 = 0;
    // SAFETY: standard call; max=0, ids=null, only fills count.
    let kr = unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) };
    if kr != 0 {
        return Err(Error::Platform(format!("CGGetActiveDisplayList: {kr}")));
    }
    let mut ids: Vec<CGDirectDisplayID> = vec![0; count as usize];
    let mut count2: u32 = 0;
    // SAFETY: ids has `count` entries.
    let kr = unsafe { CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count2) };
    if kr != 0 {
        return Err(Error::Platform(format!("CGGetActiveDisplayList[2]: {kr}")));
    }
    ids.truncate(count2 as usize);
    Ok(ids)
}

/// All external DCPAVServiceProxy entries in registry order, wrapped as
/// retained `IOAVServiceRef` (CFType).
fn enumerate_external_av_services() -> Vec<IOAVServiceHandle> {
    let mut out = Vec::new();
    let class = CString::new("DCPAVServiceProxy").expect("static C string");
    let matching = unsafe { IOServiceMatching(class.as_ptr()) };
    if matching.is_null() {
        return out;
    }
    let mut iter: io_iterator_t = 0;
    // SAFETY: kIOMainPortDefault is the documented main mach port.
    let kr = unsafe { IOServiceGetMatchingServices(kIOMainPortDefault, matching, &mut iter) };
    if kr != KERN_SUCCESS || iter == IO_OBJECT_NULL {
        return out;
    }
    let location_key = CFString::from_static_string("Location");
    let external_value = CFString::from_static_string("External");
    loop {
        // SAFETY: iter is a valid iterator until we release it.
        let svc = unsafe { IOIteratorNext(iter) };
        if svc == IO_OBJECT_NULL {
            break;
        }
        // Property "Location" must equal "External" — internal panel proxies
        // exist on AS too and we want to skip them.
        // SAFETY: svc is a valid service object until we IOObjectRelease it.
        let prop = unsafe {
            IORegistryEntryCreateCFProperty(
                svc,
                location_key.as_concrete_TypeRef(),
                std::ptr::null(),
                0,
            )
        };
        let mut take = false;
        if !prop.is_null() {
            // SAFETY: external_value is a valid CFType; CFEqual compares CF objects.
            if unsafe { CFEqual(prop, external_value.as_concrete_TypeRef() as CFTypeRef) } != 0 {
                take = true;
            }
            unsafe { CFRelease(prop) };
        }
        if take {
            // SAFETY: svc is owned, IOAVServiceCreateWithService retains as needed.
            let av = unsafe { IOAVServiceCreateWithService(std::ptr::null(), svc) };
            if !av.is_null() {
                out.push(IOAVServiceHandle(av));
            }
        }
        // SAFETY: svc was returned by IOIteratorNext; release our reference.
        unsafe {
            IOObjectRelease(svc);
        }
    }
    // SAFETY: iter is owned.
    unsafe {
        IOObjectRelease(iter);
    }
    out
}

/// RAII wrapper around an `IOAVServiceRef`. Released on drop.
struct IOAVServiceHandle(CFTypeRef);

// SAFETY: IOAVService is documented (by reverse-engineered usage) to be safe
// across threads when access to the underlying I²C is serialized; we do that
// via a Mutex inside `ExternalDisplay`.
unsafe impl Send for IOAVServiceHandle {}
unsafe impl Sync for IOAVServiceHandle {}

impl Drop for IOAVServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Owned CF reference.
            unsafe { CFRelease(self.0) };
        }
    }
}

// ---------------------------------------------------------------------------
// Internal display
// ---------------------------------------------------------------------------

struct InternalDisplay {
    info: MonitorInfo,
    id: CGDirectDisplayID,
}

impl InternalDisplay {
    fn new(id: CGDirectDisplayID) -> Self {
        let info = MonitorInfo {
            id: MonitorId::new(format!("mac:internal:{id}")),
            name: format!("Built-in Display ({id})"),
            kind: MonitorKind::Internal,
            manufacturer: None,
            model: None,
            capabilities: None,
        };
        Self { info, id }
    }
}

impl MonitorHandle for InternalDisplay {
    fn info(&self) -> &MonitorInfo {
        &self.info
    }

    fn get_brightness_percent(&self) -> Result<f32> {
        let fw = PrivateFrameworks::load()?;
        let mut v: f32 = 0.0;
        // SAFETY: ds_get is a valid loaded symbol; v is a stack local.
        let kr = unsafe { (fw.ds_get)(self.id, &mut v) };
        if kr != 0 {
            return Err(Error::Platform(format!(
                "DisplayServicesGetBrightness: kr={kr}"
            )));
        }
        Ok((v.clamp(0.0, 1.0) * 100.0) as f32)
    }

    fn set_brightness_percent(&self, percent: f32) -> Result<()> {
        let fw = PrivateFrameworks::load()?;
        let v = percent.clamp(0.0, 100.0) / 100.0;
        // SAFETY: ds_set is a valid loaded symbol.
        let kr = unsafe { (fw.ds_set)(self.id, v) };
        if kr != 0 {
            // Fall back to CoreDisplay private API where available.
            if let Some(cd_set) = fw.cd_set.as_ref() {
                // SAFETY: cd_set takes f64; same DisplayID.
                let kr2 = unsafe { (cd_set)(self.id, v as f64) };
                if kr2 == 0 {
                    return Ok(());
                }
            }
            return Err(Error::Platform(format!(
                "DisplayServicesSetBrightness: kr={kr}"
            )));
        }
        Ok(())
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

// ---------------------------------------------------------------------------
// External display
// ---------------------------------------------------------------------------

struct ExternalDisplay {
    info: MonitorInfo,
    av: IOAVServiceHandle,
    /// Serializes I²C access — concurrent reads/writes confuse the display.
    lock: Mutex<()>,
}

impl ExternalDisplay {
    fn new(id: CGDirectDisplayID, av: IOAVServiceHandle) -> Self {
        let info = MonitorInfo {
            id: MonitorId::new(format!("mac:external:{id}")),
            name: format!("External Display ({id})"),
            kind: MonitorKind::External,
            manufacturer: None,
            model: None,
            capabilities: None,
        };
        Self {
            info,
            av,
            lock: Mutex::new(()),
        }
    }

    fn write_frame(&self, frame: &[u8]) -> Result<()> {
        // Per IOAVService convention, the chip address (DDC_ADDR<<1) and
        // offset are passed as parameters. The frame written corresponds to
        // the post-offset payload starting with LEN byte (the SRC address is
        // NOT included on this code path because the OS framework adds the
        // I²C envelope itself). The first byte of `frame` is therefore
        // expected to be the LEN byte, with subsequent bytes following.
        //
        // Empirically (m1ddc, MonitorControl) the convention is: chip=0x37<<1=0x6E,
        // offset = first byte of the DDC frame after SRC, data = remaining bytes
        // including the trailing checksum. We pass the entire DDC frame
        // *minus* the leading 0x51 SRC byte: the framework injects the
        // host source address.
        debug_assert!(frame.len() >= 2, "frame must include at least LEN byte");
        let chip = (DDC_ADDR as u32) << 1; // 0x6E
        let offset = frame[1] as u32; // the LEN byte serves as offset
                                      // SAFETY: av is non-null; data pointer is valid for `len` bytes.
        let kr = unsafe {
            IOAVServiceWriteI2C(
                self.av.0,
                chip,
                offset,
                frame[2..].as_ptr(),
                (frame.len() - 2) as u32,
            )
        };
        if kr != KERN_SUCCESS {
            return Err(Error::Platform(format!("IOAVServiceWriteI2C: kr={kr}")));
        }
        Ok(())
    }

    fn read_reply(&self, expected_len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; expected_len];
        let chip = (DDC_ADDR as u32) << 1;
        // SAFETY: buf is owned and writable for expected_len bytes.
        let kr = unsafe {
            IOAVServiceReadI2C(self.av.0, chip, 0, buf.as_mut_ptr(), expected_len as u32)
        };
        if kr != KERN_SUCCESS {
            return Err(Error::Platform(format!("IOAVServiceReadI2C: kr={kr}")));
        }
        Ok(buf)
    }
}

impl MonitorHandle for ExternalDisplay {
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
        let _g = self.lock.lock();
        let frame = ddc::encode_get_vcp(code);
        self.write_frame(&frame)?;
        std::thread::sleep(std::time::Duration::from_millis(VCP_REQUEST_REPLY_DELAY_MS));
        let raw = self.read_reply(11)?;
        let reply = decode_get_vcp_reply(&raw)?;
        std::thread::sleep(std::time::Duration::from_millis(MIN_INTERVAL_MS));
        Ok(VcpValue::new(reply.current, reply.maximum))
    }

    fn set_vcp(&self, code: u8, value: u16) -> Result<()> {
        let _g = self.lock.lock();
        let frame = ddc::encode_set_vcp(code, value);
        self.write_frame(&frame)?;
        std::thread::sleep(std::time::Duration::from_millis(MIN_INTERVAL_MS));
        Ok(())
    }

    fn capabilities(&self) -> Result<Capabilities> {
        let _g = self.lock.lock();
        let mut all = Vec::<u8>::new();
        let mut offset: u16 = 0;
        // 32 fragments * 32 bytes = 1024 should comfortably cover any cap string.
        for _ in 0..64 {
            let frame = ddc::encode_capabilities_request(offset);
            self.write_frame(&frame)?;
            std::thread::sleep(std::time::Duration::from_millis(VCP_REQUEST_REPLY_DELAY_MS));
            // Capability replies are at most ~38 bytes; read 64 to be safe.
            let raw = self.read_reply(64)?;
            let frag = decode_capabilities_reply(&raw)?;
            if frag.data.is_empty() {
                break;
            }
            offset += frag.data.len() as u16;
            all.extend_from_slice(&frag.data);
            std::thread::sleep(std::time::Duration::from_millis(MIN_INTERVAL_MS));
        }
        let s = String::from_utf8_lossy(&all).to_string();
        Ok(caps::parse(&s))
    }
}

// ---------------------------------------------------------------------------
// Bookkeeping
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _link_keepalive() {
    // Keep CGDisplay in scope so its symbols actually link.
    let _: Option<CGDisplay> = None;
    // Reference the libc/c_void use to silence imports on some toolchains.
    let _: *const c_void = std::ptr::null();
}
