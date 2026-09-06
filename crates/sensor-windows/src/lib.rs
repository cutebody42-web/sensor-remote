//! Windows API implementations. Privileged service functionality is separate.
#[cfg(windows)]
mod dpapi;
#[cfg(windows)]
pub use dpapi::UserDpapi;
