//! DDC/CI wire-format encoding and decoding.
//!
//! Reference: VESA DDC/CI 1.1, MCCS Standard. The protocol on the I²C bus uses
//! address 0x37 (host destination) and 0x51 (display source). The "magic" XOR
//! seed for the host→display checksum is 0x50, the seed for the display→host
//! checksum is 0x6E (the host I²C address shifted left by one).

use crate::error::{Error, Result};

/// I²C 7-bit slave address used by DDC/CI displays.
pub const DDC_ADDR: u8 = 0x37;
/// Source byte placed in DDC/CI frames sent by the host.
pub const HOST_SRC: u8 = 0x51;
/// Source byte placed in DDC/CI frames sent by the display.
pub const DISPLAY_SRC: u8 = 0x6E;
/// XOR seed used by the display when computing reply checksum (host I²C addr << 1).
pub const REPLY_XOR_SEED: u8 = DDC_ADDR << 1;

/// Recommended minimum delay between consecutive DDC/CI frames (50 ms per spec).
pub const MIN_INTERVAL_MS: u64 = 50;
/// Recommended delay between Get-VCP request and the reply read (40 ms per spec).
pub const VCP_REQUEST_REPLY_DELAY_MS: u64 = 40;

const OPCODE_GET_VCP: u8 = 0x01;
const OPCODE_GET_VCP_REPLY: u8 = 0x02;
const OPCODE_SET_VCP: u8 = 0x03;
const OPCODE_CAP_REQUEST: u8 = 0xF3;
const OPCODE_CAP_REPLY: u8 = 0xE3;

/// Build a DDC/CI Set-VCP frame for the given feature.
///
/// Layout: `[SRC, LEN | 0x80, OP, CODE, HI, LO, XOR]`. The returned bytes are
/// the raw DDC/CI payload that follows the I²C address byte; drivers prepend
/// the address themselves.
pub fn encode_set_vcp(code: u8, value: u16) -> [u8; 7] {
    let mut buf = [0u8; 7];
    buf[0] = HOST_SRC;
    buf[1] = 0x80 | 4; // length = 4 (op + code + hi + lo)
    buf[2] = OPCODE_SET_VCP;
    buf[3] = code;
    buf[4] = (value >> 8) as u8;
    buf[5] = (value & 0xFF) as u8;
    buf[6] = host_checksum(&buf[..6]);
    buf
}

/// Build a DDC/CI Get-VCP request frame.
pub fn encode_get_vcp(code: u8) -> [u8; 5] {
    let mut buf = [0u8; 5];
    buf[0] = HOST_SRC;
    buf[1] = 0x80 | 2; // length = 2 (op + code)
    buf[2] = OPCODE_GET_VCP;
    buf[3] = code;
    buf[4] = host_checksum(&buf[..4]);
    buf
}

/// Build a DDC/CI Capabilities Request frame for the given offset.
pub fn encode_capabilities_request(offset: u16) -> [u8; 6] {
    let mut buf = [0u8; 6];
    buf[0] = HOST_SRC;
    buf[1] = 0x80 | 3;
    buf[2] = OPCODE_CAP_REQUEST;
    buf[3] = (offset >> 8) as u8;
    buf[4] = (offset & 0xFF) as u8;
    buf[5] = host_checksum(&buf[..5]);
    buf
}

/// Decoded Get-VCP reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcpReply {
    pub code: u8,
    pub vcp_type: VcpType,
    pub maximum: u16,
    pub current: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpType {
    SetParameter,
    Momentary,
    Other(u8),
}

/// Decode a Get-VCP reply. The slice should contain exactly the bytes the
/// display put on the bus (starting with the source address 0x6E). Any leading
/// 0x6E byte from raw I²C reads is tolerated; if your driver returns the
/// payload without the address, prepend nothing.
pub fn decode_get_vcp_reply(raw: &[u8]) -> Result<VcpReply> {
    // Standard reply: [DISPLAY_SRC, LEN|0x80, 0x02, RESULT, CODE, TYPE, MAX_HI, MAX_LO, CUR_HI, CUR_LO, CHK]
    if raw.len() < 11 {
        return Err(Error::Protocol(format!(
            "reply too short: {} bytes",
            raw.len()
        )));
    }
    if raw[0] != DISPLAY_SRC {
        return Err(Error::Protocol(format!(
            "unexpected source byte: 0x{:02X}",
            raw[0]
        )));
    }
    let len = (raw[1] & 0x7F) as usize;
    if len < 8 {
        return Err(Error::Protocol(format!("reply length too small: {len}")));
    }
    if raw.len() < 2 + len + 1 {
        return Err(Error::Protocol("truncated reply payload".into()));
    }
    if raw[2] != OPCODE_GET_VCP_REPLY {
        return Err(Error::Protocol(format!(
            "unexpected opcode: 0x{:02X}",
            raw[2]
        )));
    }
    if raw[3] != 0 {
        return Err(Error::Protocol(format!(
            "VCP not supported, result=0x{:02X}",
            raw[3]
        )));
    }
    if !verify_reply_checksum(&raw[..2 + len + 1]) {
        return Err(Error::Checksum);
    }
    let code = raw[4];
    let vcp_type = match raw[5] {
        0x00 => VcpType::SetParameter,
        0x01 => VcpType::Momentary,
        other => VcpType::Other(other),
    };
    let maximum = u16::from_be_bytes([raw[6], raw[7]]);
    let current = u16::from_be_bytes([raw[8], raw[9]]);
    Ok(VcpReply {
        code,
        vcp_type,
        maximum,
        current,
    })
}

/// Decoded Capability fragment reply.
#[derive(Debug, Clone)]
pub struct CapabilityFragment {
    pub offset: u16,
    pub data: Vec<u8>,
}

pub fn decode_capabilities_reply(raw: &[u8]) -> Result<CapabilityFragment> {
    // Reply: [DISPLAY_SRC, LEN|0x80, 0xE3, OFF_HI, OFF_LO, ...DATA, CHK]
    if raw.len() < 6 {
        return Err(Error::Protocol("cap reply too short".into()));
    }
    if raw[0] != DISPLAY_SRC {
        return Err(Error::Protocol("cap reply bad source".into()));
    }
    let len = (raw[1] & 0x7F) as usize;
    if raw.len() < 2 + len + 1 {
        return Err(Error::Protocol("cap reply truncated".into()));
    }
    if raw[2] != OPCODE_CAP_REPLY {
        return Err(Error::Protocol("cap reply bad opcode".into()));
    }
    if !verify_reply_checksum(&raw[..2 + len + 1]) {
        return Err(Error::Checksum);
    }
    let offset = u16::from_be_bytes([raw[3], raw[4]]);
    // payload is bytes 5..(2+len)
    let data = raw[5..2 + len].to_vec();
    Ok(CapabilityFragment { offset, data })
}

/// Compute the host→display checksum across `bytes`. Per DDC/CI 1.1 the seed
/// is the destination I²C address shifted left by one (0x37 << 1 = 0x6E) and
/// every byte of the frame including SRC and LEN is XOR'ed in.
fn host_checksum(bytes: &[u8]) -> u8 {
    let mut x = 0x6Eu8;
    for b in bytes {
        x ^= *b;
    }
    x
}

/// Verify the display→host checksum across `bytes` (the trailing byte must be
/// the supplied checksum). Same algorithm as `host_checksum` because the
/// destination address (host = 0x37) shifted left by one is also 0x6E.
fn verify_reply_checksum(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut x = REPLY_XOR_SEED;
    for b in &bytes[..bytes.len() - 1] {
        x ^= *b;
    }
    x == bytes[bytes.len() - 1]
}

/// Compute the checksum byte the display puts at the end of a reply (used by
/// tests and by drivers that need to forge frames for unit tests).
pub fn reply_checksum(bytes: &[u8]) -> u8 {
    let mut x = REPLY_XOR_SEED;
    for b in bytes {
        x ^= *b;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference vector: Set Luminance to 50 → "51 84 03 10 00 32 9A".
    #[test]
    fn set_vcp_frame_layout() {
        let f = encode_set_vcp(0x10, 50);
        assert_eq!(f, [0x51, 0x84, 0x03, 0x10, 0x00, 0x32, 0x9A]);
    }

    #[test]
    fn get_vcp_frame_layout() {
        let f = encode_get_vcp(0x10);
        // 0x6E ^ 0x51 ^ 0x82 ^ 0x01 ^ 0x10 = 0xAC
        assert_eq!(f, [0x51, 0x82, 0x01, 0x10, 0xAC]);
    }

    #[test]
    fn cap_request_frame_layout() {
        let f = encode_capabilities_request(0);
        // 0x6E ^ 0x51 ^ 0x83 ^ 0xF3 = 0x4F
        assert_eq!(f, [0x51, 0x83, 0xF3, 0x00, 0x00, 0x4F]);
    }

    #[test]
    fn decode_get_vcp_reply_roundtrip() {
        // Construct a synthetic reply: VCP 0x10, type 0x00, max=100, current=50
        let payload = [
            DISPLAY_SRC, // 0x6E
            0x88,        // 0x80 | 8
            0x02,        // op = get-vcp reply
            0x00,        // result = 0 (ok)
            0x10,        // vcp code
            0x00,        // type
            0x00, 0x64, // max = 100
            0x00, 0x32, // current = 50
        ];
        let mut buf = payload.to_vec();
        buf.push(reply_checksum(&payload));

        let r = decode_get_vcp_reply(&buf).unwrap();
        assert_eq!(r.code, 0x10);
        assert_eq!(r.vcp_type, VcpType::SetParameter);
        assert_eq!(r.maximum, 100);
        assert_eq!(r.current, 50);
    }

    #[test]
    fn decode_get_vcp_reply_rejects_bad_checksum() {
        let payload = [
            DISPLAY_SRC, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32,
        ];
        let mut buf = payload.to_vec();
        buf.push(0xFF); // bad
        let err = decode_get_vcp_reply(&buf).unwrap_err();
        matches!(err, Error::Checksum);
    }

    #[test]
    fn decode_get_vcp_reply_rejects_unsupported() {
        let payload = [
            DISPLAY_SRC, 0x88, 0x02, 0x01, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32,
        ];
        let mut buf = payload.to_vec();
        buf.push(reply_checksum(&payload));
        let err = decode_get_vcp_reply(&buf).unwrap_err();
        matches!(err, Error::Protocol(_));
    }

    #[test]
    fn decode_capabilities_fragment() {
        // Offset 0x0000, payload "abc"
        let payload = [
            DISPLAY_SRC,
            0x80 | 6, // 3 hdr bytes (op + off_hi + off_lo) + 3 data
            OPCODE_CAP_REPLY,
            0x00,
            0x00,
            b'a',
            b'b',
            b'c',
        ];
        let mut buf = payload.to_vec();
        buf.push(reply_checksum(&payload));

        let frag = decode_capabilities_reply(&buf).unwrap();
        assert_eq!(frag.offset, 0);
        assert_eq!(frag.data, b"abc");
    }
}
