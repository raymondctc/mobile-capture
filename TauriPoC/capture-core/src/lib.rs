//! Isolated Android log capture for the Tauri proof of concept.
mod evidence;
mod files;
pub mod model;
mod owner;
mod process;
mod timeline;
pub mod worker;
pub use owner::CaptureOwner;
