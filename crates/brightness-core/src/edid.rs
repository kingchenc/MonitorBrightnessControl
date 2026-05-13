//! Minimal EDID 1.x parser — just enough to extract manufacturer code and
//! model / monitor friendly name. Used to upgrade the boring "Generic PnP
//! Monitor" string returned by Windows' PHYSICAL_MONITOR API into the actual
//! monitor model the user sees in MultiMonitorTool / vendor utilities.

/// Decoded EDID fields useful to the application.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edid {
    /// 3-letter PNP manufacturer code (e.g. "DEL" for Dell, "GSM" for LG).
    pub manufacturer: String,
    /// Model name from descriptor block 0xFC if present, else empty.
    pub model_name: String,
    /// Serial-number string from descriptor block 0xFF if present.
    pub serial_string: String,
    /// 16-bit product code, little-endian.
    pub product_code: u16,
    /// Manufacture year (1990 + offset).
    pub year_of_manufacture: u16,
}

/// Parse an EDID 1.x block. Returns `None` if the header is wrong.
pub fn parse(bytes: &[u8]) -> Option<Edid> {
    if bytes.len() < 128 {
        return None;
    }
    // Required header.
    const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    if bytes[0..8] != HEADER {
        return None;
    }
    let mut out = Edid::default();

    // Manufacturer ID: bytes 8..10, big-endian, packed 5-bit chars (1='A'..26='Z').
    let mfg_word = u16::from_be_bytes([bytes[8], bytes[9]]);
    let c1 = ((mfg_word >> 10) & 0x1F) as u8;
    let c2 = ((mfg_word >> 5) & 0x1F) as u8;
    let c3 = (mfg_word & 0x1F) as u8;
    if (1..=26).contains(&c1) && (1..=26).contains(&c2) && (1..=26).contains(&c3) {
        out.manufacturer = format!(
            "{}{}{}",
            (b'A' + c1 - 1) as char,
            (b'A' + c2 - 1) as char,
            (b'A' + c3 - 1) as char
        );
    }

    out.product_code = u16::from_le_bytes([bytes[10], bytes[11]]);
    out.year_of_manufacture = 1990 + bytes[17] as u16;

    // Four 18-byte descriptor blocks at offsets 54, 72, 90, 108.
    for offset in [54usize, 72, 90, 108] {
        if offset + 18 > bytes.len() {
            break;
        }
        let block = &bytes[offset..offset + 18];
        // A "monitor descriptor" has bytes 0..4 = 00 00 00 <type>; the next
        // byte is 0; the trailing 13 bytes are the payload.
        if block[0] == 0 && block[1] == 0 && block[2] == 0 && block[4] == 0 {
            let descriptor_type = block[3];
            let payload = &block[5..18];
            let s = ascii_until_terminator(payload);
            match descriptor_type {
                0xFC => out.model_name = s,
                0xFF => out.serial_string = s,
                _ => {}
            }
        }
    }
    Some(out)
}

fn ascii_until_terminator(payload: &[u8]) -> String {
    // EDID descriptor strings are terminated by 0x0A (LF) and padded with
    // 0x20. We stop at the first LF or NUL.
    let mut s = String::new();
    for &b in payload {
        if b == 0x0A || b == 0x00 {
            break;
        }
        if b.is_ascii_graphic() || b == b' ' {
            s.push(b as char);
        }
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic EDID block: Dell, product 0xA0E5, model "DELL U2720Q".
    fn synth() -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        // Manufacturer "DEL" → D=4, E=5, L=12 → 0b00100_00101_01100 = 0x10AC
        e[8] = 0x10;
        e[9] = 0xAC;
        // Product code (LE) 0xA0E5
        e[10] = 0xE5;
        e[11] = 0xA0;
        // Year offset (e.g. year 2020 → 30)
        e[17] = 30;
        // Block 0xFC at offset 54: 00 00 00 FC 00 + 13-byte name
        let header = [0u8, 0, 0, 0xFC, 0];
        e[54..54 + 5].copy_from_slice(&header);
        let name = b"DELL U2720Q\n ";
        e[59..59 + name.len()].copy_from_slice(name);
        e
    }

    #[test]
    fn parses_synthetic_edid() {
        let p = parse(&synth()).expect("parse");
        assert_eq!(p.manufacturer, "DEL");
        assert_eq!(p.product_code, 0xA0E5);
        assert_eq!(p.year_of_manufacture, 2020);
        assert_eq!(p.model_name, "DELL U2720Q");
    }

    #[test]
    fn rejects_wrong_header() {
        let mut e = synth();
        e[0] = 0xFF;
        assert!(parse(&e).is_none());
    }

    #[test]
    fn tolerates_padded_payload() {
        let mut e = synth();
        // Replace name with one terminated by 0x0A and padded by 0x20.
        e[59..72].fill(0x20);
        e[59..63].copy_from_slice(b"ACME");
        e[63] = 0x0A;
        let p = parse(&e).unwrap();
        assert_eq!(p.model_name, "ACME");
    }
}
