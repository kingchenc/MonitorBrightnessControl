//! Parser for the MCCS Capability String returned by Capabilities-Request.
//!
//! Format (informal): `(prot(monitor)type(LCD)model(ABC)cmds(01 02 03 0C E3 F3)
//! vcp(02 04 05 06 08 0B 0C 10 12 14(01 05 06 08 0B 0C) 60(01 03 11 12) ...))`

use std::collections::BTreeMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Capabilities {
    pub model: Option<String>,
    pub display_type: Option<String>,
    pub protocol: Option<String>,
    /// Map of VCP code → permitted values (empty Vec means "any" per spec).
    pub vcp: BTreeMap<u8, Vec<u16>>,
    /// Raw capability string, useful for diagnostics.
    pub raw: String,
}

impl Capabilities {
    pub fn supports(&self, code: u8) -> bool {
        self.vcp.contains_key(&code)
    }
}

pub fn parse(raw: &str) -> Capabilities {
    let mut caps = Capabilities {
        raw: raw.to_string(),
        ..Default::default()
    };
    // Strip outer parens if present.
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed);

    for (key, body) in iter_top_level_groups(inner) {
        match key {
            "model" => caps.model = Some(body.to_string()),
            "type" => caps.display_type = Some(body.to_string()),
            "prot" | "protocol" => caps.protocol = Some(body.to_string()),
            "vcp" => parse_vcp(body, &mut caps.vcp),
            _ => {}
        }
    }
    caps
}

/// Walk the top-level `key(body)` groups of the capability string.
fn iter_top_level_groups(s: &str) -> Vec<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Read key until `(` or whitespace.
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_end = i;
        if i >= bytes.len() || bytes[i] != b'(' {
            // No body — skip.
            continue;
        }
        // Match parentheses.
        let body_start = i + 1;
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let body_end = if depth == 0 { i - 1 } else { i };
        out.push((&s[key_start..key_end], &s[body_start..body_end]));
    }
    out
}

fn parse_vcp(body: &str, out: &mut BTreeMap<u8, Vec<u16>>) {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Parse a hex byte (1 or 2 chars).
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
            i += 1;
        }
        if start == i {
            i += 1; // skip unrecognized char
            continue;
        }
        let code = match u8::from_str_radix(&body[start..i], 16) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Optional value list `(1 2 3)`.
        let mut values = Vec::new();
        if i < bytes.len() && bytes[i] == b'(' {
            let body_start = i + 1;
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            let body_end = if depth == 0 { i - 1 } else { i };
            let inner = &body[body_start..body_end];
            for tok in inner.split_ascii_whitespace() {
                if let Ok(v) = u16::from_str_radix(tok, 16) {
                    values.push(v);
                }
            }
        }
        out.insert(code, values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let s = "(prot(monitor)type(LCD)model(EX)vcp(10 12 14(05 06 08) 60(01 11)))";
        let c = parse(s);
        assert_eq!(c.model.as_deref(), Some("EX"));
        assert_eq!(c.display_type.as_deref(), Some("LCD"));
        assert_eq!(c.protocol.as_deref(), Some("monitor"));
        assert!(c.supports(0x10));
        assert!(c.supports(0x12));
        assert_eq!(c.vcp.get(&0x14), Some(&vec![0x05, 0x06, 0x08]));
        assert_eq!(c.vcp.get(&0x60), Some(&vec![0x01, 0x11]));
        assert!(!c.supports(0xFF));
    }

    #[test]
    fn parse_no_outer_parens() {
        let s = "vcp(10 12)";
        let c = parse(s);
        assert!(c.supports(0x10));
        assert!(c.supports(0x12));
    }

    #[test]
    fn parse_truncated_caps_does_not_panic() {
        let s = "(vcp(10 12";
        let _ = parse(s);
    }
}
