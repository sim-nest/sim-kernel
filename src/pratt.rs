//! Pratt-parser contracts: operator tables for [`shape`](crate::shape)-driven
//! syntax.
//!
//! The kernel defines the operator-table, fixity, and token contracts that
//! parameterize precedence parsing; concrete grammars and codecs that drive
//! them live in library crates.

mod object;
#[cfg(test)]
mod tests;
mod types;

pub use object::{PrattTableObject, pratt_table_value};
pub use types::{Fixity, PrattOperator, PrattResult, PrattTable, Token, parse_symbol};
