//! Domain primitives shared by SENSOR Remote Access components.
//!
//! This crate intentionally contains no networking, UI, or platform code.
//! Stable identifiers and validation rules live here so every component uses
//! the same representation.

use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

const MIN_DEVICE_ID: u32 = 100_000_000;
const DEVICE_ID_SPAN: u32 = 900_000_000;

/// A persistent nine-digit SENSOR device identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DeviceId(u32);

impl DeviceId {
    pub const fn new(value: u32) -> Option<Self> {
        if value >= MIN_DEVICE_ID && value < MIN_DEVICE_ID + DEVICE_ID_SPAN {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn generate(rng: &mut impl RngCore) -> Self {
        // Rejection sampling avoids introducing measurable modulo bias into
        // the identifier space while keeping generation dependency-light.
        let limit = u32::MAX - (u32::MAX % DEVICE_ID_SPAN);
        loop {
            let candidate = rng.next_u32();
            if candidate < limit {
                return Self(MIN_DEVICE_ID + (candidate % DEVICE_ID_SPAN));
            }
        }
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = format!("{:09}", self.0);
        write!(f, "{} {} {}", &digits[0..3], &digits[3..6], &digits[6..9])
    }
}

impl TryFrom<u32> for DeviceId {
    type Error = CoreError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(CoreError::InvalidDeviceId(value))
    }
}

/// A user-facing alias, optionally qualified by a namespace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceAlias(String);

impl DeviceAlias {
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 {
            return Err(CoreError::InvalidAlias);
        }
        if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(CoreError::InvalidAlias);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum CoreError {
    #[error("invalid SENSOR device id: {0}")]
    InvalidDeviceId(u32),
    #[error("invalid device alias")]
    InvalidAlias,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{Error, RngCore};

    struct FixedRng(u32);

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            self.0
        }

        fn next_u64(&mut self) -> u64 {
            self.next_u32() as u64
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(4) {
                chunk.copy_from_slice(&self.0.to_le_bytes()[..chunk.len()]);
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    #[test]
    fn device_id_is_grouped_for_humans() {
        let id = DeviceId::new(425_786_319).unwrap();
        assert_eq!(id.to_string(), "425 786 319");
    }

    #[test]
    fn generated_id_is_in_range() {
        let id = DeviceId::generate(&mut FixedRng(42));
        assert!(DeviceId::new(id.value()).is_some());
    }

    #[test]
    fn aliases_reject_whitespace() {
        assert!(DeviceAlias::new("support pc").is_err());
        assert!(DeviceAlias::new("support-pc").is_ok());
    }
}
