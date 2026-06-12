//! Entity storage and property staging for Murk simulations.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod overlay;
pub mod record;
pub mod snapshot;
pub mod staging;
pub mod store;

pub use overlay::EntityOverlayReader;
pub use record::EntityRecord;
pub use snapshot::EntitySnapshot;
pub use staging::PropertyStaging;
pub use store::{EntityStore, EntityStoreSnapshot};
