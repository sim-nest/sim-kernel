//! The library and registry contracts: loading behavior against the kernel.
//!
//! The kernel defines the [`Lib`], [`Linker`], [`Registry`], [`ExportRecord`],
//! and loader contracts plus manifest and version metadata; the libraries
//! supply the behavior these registries make available.

mod boot;
mod boot_codec;
mod claims;
mod cx;
mod loaders;
mod model;
mod registry;
#[cfg(test)]
mod tests;
mod transaction;

pub use boot::{LibBootDependency, LibBootReceipt, LibSourceSpec, RegistryBootState};
pub use loaders::{CatalogSource, Lib, LibLoader, LibSource, LoadCx, LoaderRegistry};
pub use model::{
    AbiVersion, Dependency, Export, ExportKind, ExportRecord, ExportState, LibManifest, LibTarget,
    LoadedLib, RegisteredTest, Test, TestReport, Version,
};
pub use registry::Registry;
pub use transaction::{Linker, LoadTransaction};
