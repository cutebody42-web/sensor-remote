//! Endpoint consent state. Every privileged operation must call require().
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Permission {
    Audio,
    Input,
    ClipboardText,
    ClipboardFiles,
    FileManager,
    Printing,
    Restart,
    Lock,
    SecureAttention,
    SystemInformation,
    Whiteboard,
    TcpTunnel,
    Vpn,
    Privacy,
    Recording,
    BlockLocalInput,
    RemotePointer,
    ViewDesktop,
    Chat,
}

const VALID_BITS: u64 = (1 << 19) - 1;

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct Permissions(u64);

impl TryFrom<u64> for Permissions {
    type Error = PermissionError;
    fn try_from(bits: u64) -> Result<Self, Self::Error> {
        if bits & !VALID_BITS != 0 {
            return Err(PermissionError::UnknownPermission);
        }
        Ok(Self(bits))
    }
}

impl Permissions {
    pub const NONE: Self = Self(0);
    pub const FULL_ACCESS: Self = Self(VALID_BITS);
    pub fn of(values: &[Permission]) -> Self {
        Self(values.iter().fold(0, |bits, p| bits | (1 << (*p as u8))))
    }
    pub fn allows(self, permission: Permission) -> bool {
        self.0 & (1 << permission as u8) != 0
    }
    pub fn is_subset_of(self, maximum: Self) -> bool {
        self.0 & !maximum.0 == 0
    }
    pub fn file_transfer() -> Self {
        Self::of(&[Permission::FileManager])
    }
    pub fn screen_sharing() -> Self {
        Self::of(&[
            Permission::ViewDesktop,
            Permission::RemotePointer,
            Permission::Chat,
        ])
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PermissionError {
    #[error("session has not been accepted")]
    NotAccepted,
    #[error("operation is not granted by the endpoint")]
    Denied,
    #[error("permission profile contains unknown bits")]
    UnknownPermission,
    #[error("invalid session transition")]
    InvalidTransition,
    #[error("permission change exceeds the requested profile")]
    Escalation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsentState {
    Pending,
    Active,
    Rejected,
    Closed,
}

pub struct Consent {
    requested: Permissions,
    granted: Permissions,
    state: ConsentState,
}

impl Consent {
    pub fn pending(requested: Permissions) -> Self {
        Self {
            requested,
            granted: Permissions::NONE,
            state: ConsentState::Pending,
        }
    }
    pub fn accept(&mut self, granted: Permissions) -> Result<(), PermissionError> {
        if self.state != ConsentState::Pending {
            return Err(PermissionError::InvalidTransition);
        }
        if !granted.is_subset_of(self.requested) {
            return Err(PermissionError::Escalation);
        }
        self.granted = granted;
        self.state = ConsentState::Active;
        Ok(())
    }
    pub fn reject(&mut self) -> Result<(), PermissionError> {
        if self.state != ConsentState::Pending {
            return Err(PermissionError::InvalidTransition);
        }
        self.state = ConsentState::Rejected;
        Ok(())
    }
    /// A local endpoint decision; never expose as a peer-issued grant operation.
    pub fn change_permissions(&mut self, granted: Permissions) -> Result<(), PermissionError> {
        if self.state != ConsentState::Active {
            return Err(PermissionError::InvalidTransition);
        }
        if !granted.is_subset_of(self.requested) {
            return Err(PermissionError::Escalation);
        }
        self.granted = granted;
        Ok(())
    }
    pub fn require(&self, permission: Permission) -> Result<(), PermissionError> {
        if self.state != ConsentState::Active {
            return Err(PermissionError::NotAccepted);
        }
        if !self.granted.allows(permission) {
            return Err(PermissionError::Denied);
        }
        Ok(())
    }
    pub fn state(&self) -> ConsentState {
        self.state
    }
    pub fn granted(&self) -> Permissions {
        self.granted
    }
    pub fn close(&mut self) {
        self.granted = Permissions::NONE;
        self.state = ConsentState::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_requires_consent_and_grant_and_stops_after_revocation() {
        let requested = Permissions::of(&[Permission::Input, Permission::Chat]);
        let mut consent = Consent::pending(requested);
        assert_eq!(
            consent.require(Permission::Input),
            Err(PermissionError::NotAccepted)
        );
        consent.accept(requested).unwrap();
        consent.require(Permission::Input).unwrap();
        consent
            .change_permissions(Permissions::of(&[Permission::Chat]))
            .unwrap();
        assert_eq!(
            consent.require(Permission::Input),
            Err(PermissionError::Denied)
        );
        consent.close();
        assert!(consent.require(Permission::Chat).is_err());
        assert!(consent.accept(requested).is_err());
    }
    #[test]
    fn file_only_session_cannot_escalate_to_desktop_control() {
        let mut consent = Consent::pending(Permissions::file_transfer());
        assert_eq!(
            consent.accept(Permissions::FULL_ACCESS),
            Err(PermissionError::Escalation)
        );
        consent.accept(Permissions::file_transfer()).unwrap();
        assert!(consent.require(Permission::Input).is_err());
        assert!(consent.require(Permission::ViewDesktop).is_err());
        consent.require(Permission::FileManager).unwrap();
    }
    #[test]
    fn rejection_is_terminal() {
        let mut consent = Consent::pending(Permissions::FULL_ACCESS);
        consent.reject().unwrap();
        assert!(consent.accept(Permissions::FULL_ACCESS).is_err());
        assert!(consent.require(Permission::FileManager).is_err());
    }
}
