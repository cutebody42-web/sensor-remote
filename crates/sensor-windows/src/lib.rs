//! Windows API implementations. Privileged service functionality is separate.
#[cfg(windows)]
mod dpapi;
#[cfg(windows)]
pub use dpapi::UserDpapi;
#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod codec;
#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod startup;
