#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod win32;

#[cfg(windows)]
pub use render::*;
#[cfg(windows)]
pub use win32::*;

#[cfg(not(windows))]
pub mod linux;

#[cfg(not(windows))]
pub use linux::*;
