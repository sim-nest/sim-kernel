use crate::{Error, Result, Symbol};

/// Build a deterministic ASCII catalog key from a namespace and path parts.
pub fn catalog_key(namespace: &str, parts: &[&str]) -> Symbol {
    let mut key = String::new();
    push_escaped(&mut key, namespace);
    for part in parts {
        key.push('/');
        push_escaped(&mut key, part);
    }
    Symbol::new(key)
}

/// Split a deterministic catalog key back into decoded path components.
pub fn split_catalog_key(symbol: &Symbol) -> Result<Vec<String>> {
    symbol
        .as_qualified_str()
        .split('/')
        .map(decode_part)
        .collect()
}

fn push_escaped(out: &mut String, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'/' => out.push_str("%2F"),
            0x00..=0x1f | 0x7f..=0xff => push_hex_escape(out, byte),
            _ => out.push(char::from(byte)),
        }
    }
}

fn push_hex_escape(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(char::from(HEX[usize::from(byte >> 4)]));
    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
}

fn decode_part(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hi = bytes
                    .get(index + 1)
                    .copied()
                    .and_then(hex_value)
                    .ok_or_else(malformed_escape)?;
                let lo = bytes
                    .get(index + 2)
                    .copied()
                    .and_then(hex_value)
                    .ok_or_else(malformed_escape)?;
                decoded.push((hi << 4) | lo);
                index += 3;
            }
            byte @ (0x00..=0x1f | 0x7f..=0xff) => {
                return Err(catalog_key_schema_error(format!(
                    "catalog key contains unescaped byte 0x{byte:02X}"
                )));
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| catalog_key_schema_error("catalog key escape is not valid UTF-8"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn malformed_escape() -> Error {
    catalog_key_schema_error("malformed catalog key escape")
}

fn catalog_key_schema_error(message: impl Into<String>) -> Error {
    Error::CatalogSchema {
        table: Symbol::new("catalog/key"),
        message: message.into(),
    }
}
