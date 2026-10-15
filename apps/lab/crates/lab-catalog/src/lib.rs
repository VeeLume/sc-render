//! The lab's own data: the DataCore's items as render scenes ([`Catalog`]),
//! Starlume inventory dumps resolved against them, and saved-character
//! colours (`.chf`). An app embedding sc-render brings its own equivalent.

mod catalog;
mod look;

pub use catalog::{Body, Catalog, ChildSpec, FALLBACK, InventoryNode, ItemSummary};
pub use look::{CharacterFile, CharacterLook, character_files, character_look};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Star Citizen install found: {0}")]
    Discovery(String),
    #[error(transparent)]
    Extract(#[from] sc_extract::Error),
    #[error("{0}")]
    Catalog(String),
}

pub type Result<T> = std::result::Result<T, Error>;
