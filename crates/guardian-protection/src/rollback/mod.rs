pub mod strategy_trait;
pub mod vault;
pub mod file_copy;
pub mod metadata_only;

pub use strategy_trait::{RollbackStrategy, SnapshotId};
pub use vault::VaultManager;
pub use file_copy::FileCopyStrategy;
pub use metadata_only::MetadataOnlyStrategy;
