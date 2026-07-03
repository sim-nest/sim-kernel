//! The [`Datum`] contract: the content-addressable data form of the substrate.
//!
//! The kernel defines the datum shape and its content-hash algorithm
//! identity; libraries supply the stores that intern and resolve data.

use std::collections::BTreeSet;

use crate::{
    error::{Error, Result},
    expr::{Expr, NumberLiteral},
    id::Symbol,
    ref_id::ContentId,
};

/// Namespace of the kernel datum content-hash algorithm symbol.
pub const DATUM_CONTENT_ALGORITHM_NAMESPACE: &str = "core";
/// Name of the kernel datum content-hash algorithm symbol.
pub const DATUM_CONTENT_ALGORITHM_NAME: &str = "sha256-datum-v1";

/// Content-addressable data form of the substrate.
///
/// A `Datum` is the canonical, hashable projection of an [`Expr`]: the same
/// value always hashes to the same [`ContentId`], independent of how it was
/// written. The kernel fixes the datum shape and its content-hash algorithm;
/// libraries supply the [`DatumStore`](crate::datum_store::DatumStore)s that
/// intern and resolve data.
///
/// # Examples
///
/// ```
/// # use sim_kernel::Datum;
/// let datum = Datum::String("hello".to_owned());
/// let id = datum.content_id().unwrap();
/// // The same datum always hashes to the same content id.
/// assert_eq!(id, Datum::String("hello".to_owned()).content_id().unwrap());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Datum {
    /// The nil datum.
    Nil,
    /// A boolean datum.
    Bool(bool),
    /// A number literal in some domain.
    Number(NumberLiteral),
    /// A symbol datum.
    Symbol(Symbol),
    /// A UTF-8 string datum.
    String(String),
    /// A byte-string datum.
    Bytes(Vec<u8>),
    /// An ordered list datum.
    List(Vec<Datum>),
    /// An ordered vector datum.
    Vector(Vec<Datum>),
    /// A map datum as ordered key/value pairs; duplicate keys are rejected when
    /// hashing.
    Map(Vec<(Datum, Datum)>),
    /// A set datum; duplicate entries are rejected when hashing.
    Set(Vec<Datum>),
    /// A tagged node datum with named fields, the datum form of an
    /// [`Expr::Extension`].
    Node {
        /// Node tag symbol.
        tag: Symbol,
        /// Named field bindings in declaration order.
        fields: Vec<(Symbol, Datum)>,
    },
}

impl Datum {
    /// Returns the canonical byte encoding used to compute the content id.
    ///
    /// The encoding is deterministic: equal data produces equal bytes, with map
    /// and set entries ordered by their hashes so member order does not affect
    /// the result. Returns an error when the datum cannot be canonicalized
    /// (for example, a map with duplicate keys).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        write_bytes(&mut out, b"sim6:datum:v1");
        self.write_canonical(&mut out)?;
        Ok(out)
    }

    /// Returns the content id of this datum under the kernel datum algorithm.
    pub fn content_id(&self) -> Result<ContentId> {
        Ok(ContentId::from_bytes(
            datum_content_algorithm(),
            sha256(&self.canonical_bytes()?),
        ))
    }

    fn write_canonical(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Nil => write_tag(out, "sim6:datum:nil"),
            Self::Bool(value) => {
                write_tag(out, "sim6:datum:bool");
                out.push(u8::from(*value));
            }
            Self::Number(value) => {
                write_tag(out, "sim6:datum:number");
                write_symbol(out, &value.domain);
                write_string(out, &value.canonical);
            }
            Self::Symbol(symbol) => {
                write_tag(out, "sim6:datum:symbol");
                write_symbol(out, symbol);
            }
            Self::String(value) => {
                write_tag(out, "sim6:datum:string");
                write_string(out, value);
            }
            Self::Bytes(value) => {
                write_tag(out, "sim6:datum:bytes");
                write_bytes(out, value);
            }
            Self::List(items) => {
                write_tag(out, "sim6:datum:list");
                write_records(out, ordered_child_records(items)?)?;
            }
            Self::Vector(items) => {
                write_tag(out, "sim6:datum:vector");
                write_records(out, ordered_child_records(items)?)?;
            }
            Self::Map(entries) => {
                write_tag(out, "sim6:datum:map");
                write_records(out, map_records(entries)?)?;
            }
            Self::Set(items) => {
                write_tag(out, "sim6:datum:set");
                write_records(out, set_records(items)?)?;
            }
            Self::Node { tag, fields } => {
                write_tag(out, "sim6:datum:node");
                write_symbol(out, tag);
                write_records(out, node_field_records(fields)?)?;
            }
        }
        Ok(())
    }
}

/// Returns the kernel datum content-hash algorithm symbol (`core/sha256-datum-v1`).
pub fn datum_content_algorithm() -> Symbol {
    Symbol::qualified(
        DATUM_CONTENT_ALGORITHM_NAMESPACE,
        DATUM_CONTENT_ALGORITHM_NAME,
    )
}

impl TryFrom<Expr> for Datum {
    type Error = Error;

    fn try_from(expr: Expr) -> Result<Self> {
        match expr {
            Expr::Nil => Ok(Self::Nil),
            Expr::Bool(value) => Ok(Self::Bool(value)),
            Expr::Number(value) => Ok(Self::Number(value)),
            Expr::Symbol(value) => Ok(Self::Symbol(value)),
            Expr::String(value) => Ok(Self::String(value)),
            Expr::Bytes(value) => Ok(Self::Bytes(value)),
            Expr::List(items) => items
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>>>()
                .map(Self::List),
            Expr::Vector(items) => items
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>>>()
                .map(Self::Vector),
            Expr::Map(entries) => entries
                .into_iter()
                .map(|(key, value)| Ok((Self::try_from(key)?, Self::try_from(value)?)))
                .collect::<Result<Vec<_>>>()
                .map(Self::Map),
            Expr::Set(items) => items
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>>>()
                .map(Self::Set),
            Expr::Extension { tag, payload } => extension_to_node(tag, *payload),
            other => Err(Error::TypeMismatch {
                expected: "datum expression",
                found: expr_variant_name(&other),
            }),
        }
    }
}

impl From<Datum> for Expr {
    fn from(datum: Datum) -> Self {
        match datum {
            Datum::Nil => Self::Nil,
            Datum::Bool(value) => Self::Bool(value),
            Datum::Number(value) => Self::Number(value),
            Datum::Symbol(value) => Self::Symbol(value),
            Datum::String(value) => Self::String(value),
            Datum::Bytes(value) => Self::Bytes(value),
            Datum::List(items) => Self::List(items.into_iter().map(Self::from).collect()),
            Datum::Vector(items) => Self::Vector(items.into_iter().map(Self::from).collect()),
            Datum::Map(entries) => Self::Map(
                entries
                    .into_iter()
                    .map(|(key, value)| (Self::from(key), Self::from(value)))
                    .collect(),
            ),
            Datum::Set(items) => Self::Set(items.into_iter().map(Self::from).collect()),
            Datum::Node { tag, fields } => Self::Extension {
                tag,
                payload: Box::new(Self::Map(
                    fields
                        .into_iter()
                        .map(|(field, value)| (Self::Symbol(field), Self::from(value)))
                        .collect(),
                )),
            },
        }
    }
}

fn extension_to_node(tag: Symbol, payload: Expr) -> Result<Datum> {
    let Expr::Map(entries) = payload else {
        return Err(Error::TypeMismatch {
            expected: "datum node field map",
            found: expr_variant_name(&payload),
        });
    };

    let fields = entries
        .into_iter()
        .map(|(key, value)| {
            let Expr::Symbol(field) = key else {
                return Err(Error::TypeMismatch {
                    expected: "datum node field symbol",
                    found: expr_variant_name(&key),
                });
            };
            Ok((field, Datum::try_from(value)?))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Datum::Node { tag, fields })
}

fn expr_variant_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Nil => "nil expression",
        Expr::Bool(_) => "bool expression",
        Expr::Number(_) => "number expression",
        Expr::Symbol(_) => "symbol expression",
        Expr::Local(_) => "local expression",
        Expr::String(_) => "string expression",
        Expr::Bytes(_) => "bytes expression",
        Expr::List(_) => "list expression",
        Expr::Vector(_) => "vector expression",
        Expr::Map(_) => "map expression",
        Expr::Set(_) => "set expression",
        Expr::Call { .. } => "call expression",
        Expr::Infix { .. } => "infix expression",
        Expr::Prefix { .. } => "prefix expression",
        Expr::Postfix { .. } => "postfix expression",
        Expr::Block(_) => "block expression",
        Expr::Quote { .. } => "quote expression",
        Expr::Annotated { .. } => "annotated expression",
        Expr::Extension { .. } => "extension expression",
    }
}

fn ordered_child_records(items: &[Datum]) -> Result<Vec<Vec<u8>>> {
    items.iter().map(Datum::canonical_bytes).collect()
}

fn map_records(entries: &[(Datum, Datum)]) -> Result<Vec<Vec<u8>>> {
    let mut keys = BTreeSet::new();
    let mut records = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let key_bytes = key.canonical_bytes()?;
        if !keys.insert(key_bytes.clone()) {
            return Err(Error::Eval("duplicate datum map key".to_owned()));
        }
        let value_bytes = value.canonical_bytes()?;
        let mut record = Vec::new();
        write_tag(&mut record, "sim6:datum:map-entry");
        write_bytes(&mut record, &key_bytes);
        write_bytes(&mut record, &value_bytes);
        records.push(record);
    }
    sort_records(&mut records);
    Ok(records)
}

fn set_records(items: &[Datum]) -> Result<Vec<Vec<u8>>> {
    let mut seen = BTreeSet::new();
    let mut records = Vec::with_capacity(items.len());
    for item in items {
        let bytes = item.canonical_bytes()?;
        if !seen.insert(bytes.clone()) {
            return Err(Error::Eval("duplicate datum set entry".to_owned()));
        }
        records.push(bytes);
    }
    sort_records(&mut records);
    Ok(records)
}

fn node_field_records(fields: &[(Symbol, Datum)]) -> Result<Vec<Vec<u8>>> {
    let mut names = BTreeSet::new();
    let mut records = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        let mut name_bytes = Vec::new();
        write_symbol(&mut name_bytes, name);
        if !names.insert(name_bytes.clone()) {
            return Err(Error::Eval("duplicate datum node field".to_owned()));
        }
        let value_bytes = value.canonical_bytes()?;
        let mut record = Vec::new();
        write_tag(&mut record, "sim6:datum:node-field");
        write_bytes(&mut record, &name_bytes);
        write_bytes(&mut record, &value_bytes);
        records.push(record);
    }
    sort_records(&mut records);
    Ok(records)
}

fn sort_records(records: &mut [Vec<u8>]) {
    records.sort_by(|left, right| {
        sha256(left)
            .cmp(&sha256(right))
            .then_with(|| left.cmp(right))
    });
}

fn write_records(out: &mut Vec<u8>, records: Vec<Vec<u8>>) -> Result<()> {
    write_len(out, records.len())?;
    for record in records {
        write_bytes(out, &record);
    }
    Ok(())
}

fn write_tag(out: &mut Vec<u8>, tag: &str) {
    write_bytes(out, tag.as_bytes());
}

fn write_symbol(out: &mut Vec<u8>, symbol: &Symbol) {
    match &symbol.namespace {
        Some(namespace) => {
            out.push(1);
            write_string(out, namespace);
        }
        None => out.push(0),
    }
    write_string(out, &symbol.name);
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_bytes(out, value.as_bytes());
}

fn write_bytes(out: &mut Vec<u8>, value: &[u8]) {
    write_len(out, value.len()).expect("canonical datum record length exceeded u64");
    out.extend_from_slice(value);
}

fn write_len(out: &mut Vec<u8>, len: usize) -> Result<()> {
    let len = u64::try_from(len).map_err(|_| Error::Eval("datum record too large".to_owned()))?;
    out.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h = H0;
    let mut message = input.to_vec();
    let bit_len = u64::try_from(input.len())
        .unwrap_or(u64::MAX)
        .wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (word, bytes) in w.iter_mut().take(16).zip(chunk.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0_u8; 32];
    for (slot, word) in out.chunks_exact_mut(4).zip(h) {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
#[path = "datum_tests.rs"]
mod tests;
