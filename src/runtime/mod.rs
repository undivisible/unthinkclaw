//! Runtime abstraction — execution environments.

pub mod traits;
pub mod native;
#[cfg(feature = "docker")]
pub mod docker;

pub use traits::RuntimeAdapter;
