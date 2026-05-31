//! VCP (Virtual Control Panel) codes per the VESA Monitor Control Command Set (MCCS).
//!
//! Only the codes the application actually uses are listed; the wire protocol
//! accepts any `u8` so unknown codes can still be probed via `set_vcp_raw`.

use std::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A high-level VCP feature with documented semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum VcpFeature {
    /// 0x10 — Luminance / brightness, 0..=100.
    Luminance,
    /// 0x12 — Contrast, 0..=100.
    Contrast,
    /// 0x14 — Select color preset (5=6500K, 6=7500K, 8=9300K, 11=user, ...).
    ColorPreset,
    /// 0x16 — Video gain (red).
    VideoGainRed,
    /// 0x18 — Video gain (green).
    VideoGainGreen,
    /// 0x1A — Video gain (blue).
    VideoGainBlue,
    /// 0x60 — Input source.
    InputSource,
    /// 0x62 — Audio: speaker volume.
    AudioVolume,
    /// 0x8D — Audio mute.
    AudioMute,
    /// 0xD6 — Power mode (1=on, 4=standby, 5=off).
    PowerMode,
    /// 0xDC — Display application/picture mode.
    PictureMode,
    /// 0xCC — OSD language.
    OsdLanguage,
}

impl VcpFeature {
    /// Wire-level VCP code per MCCS.
    pub const fn code(self) -> u8 {
        match self {
            Self::Luminance => 0x10,
            Self::Contrast => 0x12,
            Self::ColorPreset => 0x14,
            Self::VideoGainRed => 0x16,
            Self::VideoGainGreen => 0x18,
            Self::VideoGainBlue => 0x1A,
            Self::InputSource => 0x60,
            Self::AudioVolume => 0x62,
            Self::AudioMute => 0x8D,
            Self::PowerMode => 0xD6,
            Self::PictureMode => 0xDC,
            Self::OsdLanguage => 0xCC,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0x10 => Self::Luminance,
            0x12 => Self::Contrast,
            0x14 => Self::ColorPreset,
            0x16 => Self::VideoGainRed,
            0x18 => Self::VideoGainGreen,
            0x1A => Self::VideoGainBlue,
            0x60 => Self::InputSource,
            0x62 => Self::AudioVolume,
            0x8D => Self::AudioMute,
            0xD6 => Self::PowerMode,
            0xDC => Self::PictureMode,
            0xCC => Self::OsdLanguage,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Luminance => "luminance",
            Self::Contrast => "contrast",
            Self::ColorPreset => "color_preset",
            Self::VideoGainRed => "video_gain_red",
            Self::VideoGainGreen => "video_gain_green",
            Self::VideoGainBlue => "video_gain_blue",
            Self::InputSource => "input_source",
            Self::AudioVolume => "audio_volume",
            Self::AudioMute => "audio_mute",
            Self::PowerMode => "power_mode",
            Self::PictureMode => "picture_mode",
            Self::OsdLanguage => "osd_language",
        }
    }
}

impl fmt::Display for VcpFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:02X})", self.name(), self.code())
    }
}

/// Current and maximum value for a VCP feature, returned by a DDC/CI Get-VCP reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VcpValue {
    pub current: u16,
    pub maximum: u16,
}

impl VcpValue {
    pub fn new(current: u16, maximum: u16) -> Self {
        Self { current, maximum }
    }

    /// Current value as a 0..=100 percentage relative to `maximum`.
    pub fn percent(self) -> f32 {
        if self.maximum == 0 {
            0.0
        } else {
            (self.current as f32 / self.maximum as f32) * 100.0
        }
    }

    /// Convert a percent target (0..=100) into the absolute value to write,
    /// clamped to `maximum`. Returns 0 if `maximum` is 0 (degenerate monitor).
    pub fn percent_to_absolute(self, percent: f32) -> u16 {
        if self.maximum == 0 {
            return 0;
        }
        let clamped = percent.clamp(0.0, 100.0);
        let max = self.maximum as f32;
        (clamped / 100.0 * max).round().clamp(0.0, max) as u16
    }
}

/// Decide whether a verified VCP write actually "took", given the value read
/// immediately *before* the write (`prev`, `None` if it could not be read), the
/// value read back immediately *after* (`after`), and what we asked for
/// (`target`).
///
/// * Accepts when the read-back equals the target exactly.
/// * Accepts when the read-back moved away from `prev` — some displays quantize
///   to their own internal step grid and land near, but not exactly on, the
///   requested value.
/// * Accepts when there was no prior reading to compare against (best effort —
///   never loop forever on a display we cannot read).
/// * Rejects only when the value is unchanged from `prev` **and** still off
///   target. That is the classic "first DDC/CI write after the panel went idle
///   was silently dropped" case — the caller should back off briefly and retry.
pub fn write_accepted(prev: Option<u16>, after: u16, target: u16) -> bool {
    if after == target {
        return true;
    }
    match prev {
        Some(p) => after != p,
        None => true,
    }
}

/// Standard ColorPreset (0x14) values per MCCS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ColorPreset {
    SRgb,
    Native,
    K4000,
    K5000,
    K6500,
    K7500,
    K8200,
    K9300,
    K10000,
    K11500,
    UserDefined,
    Other(u16),
}

impl ColorPreset {
    pub const fn raw(self) -> u16 {
        match self {
            Self::SRgb => 0x01,
            Self::Native => 0x02,
            Self::K4000 => 0x03,
            Self::K5000 => 0x04,
            Self::K6500 => 0x05,
            Self::K7500 => 0x06,
            Self::K8200 => 0x07,
            Self::K9300 => 0x08,
            Self::K10000 => 0x09,
            Self::K11500 => 0x0A,
            Self::UserDefined => 0x0B,
            Self::Other(v) => v,
        }
    }

    pub const fn from_raw(v: u16) -> Self {
        match v {
            0x01 => Self::SRgb,
            0x02 => Self::Native,
            0x03 => Self::K4000,
            0x04 => Self::K5000,
            0x05 => Self::K6500,
            0x06 => Self::K7500,
            0x07 => Self::K8200,
            0x08 => Self::K9300,
            0x09 => Self::K10000,
            0x0A => Self::K11500,
            0x0B => Self::UserDefined,
            other => Self::Other(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcp_codes_roundtrip() {
        for f in [
            VcpFeature::Luminance,
            VcpFeature::Contrast,
            VcpFeature::ColorPreset,
            VcpFeature::InputSource,
            VcpFeature::AudioVolume,
            VcpFeature::AudioMute,
            VcpFeature::PowerMode,
            VcpFeature::PictureMode,
            VcpFeature::OsdLanguage,
            VcpFeature::VideoGainRed,
            VcpFeature::VideoGainGreen,
            VcpFeature::VideoGainBlue,
        ] {
            assert_eq!(VcpFeature::from_code(f.code()), Some(f));
        }
    }

    #[test]
    fn percent_math() {
        let v = VcpValue::new(50, 100);
        assert!((v.percent() - 50.0).abs() < 0.01);
        assert_eq!(v.percent_to_absolute(75.0), 75);
        assert_eq!(v.percent_to_absolute(150.0), 100);
        assert_eq!(v.percent_to_absolute(-5.0), 0);

        let v = VcpValue::new(0, 200);
        assert_eq!(v.percent_to_absolute(50.0), 100);
        assert_eq!(v.percent_to_absolute(100.0), 200);
    }

    #[test]
    fn percent_zero_maximum_safe() {
        let v = VcpValue::new(0, 0);
        assert_eq!(v.percent(), 0.0);
        assert_eq!(v.percent_to_absolute(50.0), 0);
    }

    #[test]
    fn write_accepted_logic() {
        // Exact landing → accepted regardless of prior value.
        assert!(write_accepted(Some(20), 50, 50));
        assert!(write_accepted(None, 50, 50));
        // Quantized: moved away from prev but not exactly on target → accepted.
        assert!(write_accepted(Some(20), 49, 50));
        // Dropped: unchanged from prev and still off target → rejected (retry).
        assert!(!write_accepted(Some(20), 20, 50));
        // No prior reading available → best-effort accept (never loop forever).
        assert!(write_accepted(None, 20, 50));
        // Enumerated codes (e.g. color preset): old value sticks → rejected.
        assert!(!write_accepted(Some(4), 4, 5));
        // Enumerated code accepted exactly.
        assert!(write_accepted(Some(4), 5, 5));
    }

    #[test]
    fn color_preset_roundtrip() {
        for p in [
            ColorPreset::SRgb,
            ColorPreset::Native,
            ColorPreset::K4000,
            ColorPreset::K5000,
            ColorPreset::K6500,
            ColorPreset::K7500,
            ColorPreset::K9300,
            ColorPreset::K10000,
            ColorPreset::UserDefined,
        ] {
            assert_eq!(ColorPreset::from_raw(p.raw()), p);
        }
        assert_eq!(ColorPreset::from_raw(0x42), ColorPreset::Other(0x42));
    }
}
