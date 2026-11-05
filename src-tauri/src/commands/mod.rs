//! IPC command surface. Every command validates its input and routes privileged
//! work through the Rust core; the webview never touches credentials or files.

pub mod agent;
pub mod clusters;
pub mod helm;
pub mod resources;
pub mod streams;
pub mod system;
