use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Network {
    server: String,
}

/// Resolve only the selected source. A valid environment/profile override must
/// not be defeated by a malformed lower-priority package configuration.
pub fn resolve_network(
    environment: Option<String>,
    profile: &Path,
    package: &Path,
) -> Result<Option<String>, String> {
    if let Some(server) = environment {
        sensor_render::websocket_url(&server).map_err(|e| e.to_string())?;
        return Ok(Some(server));
    }
    if let Some(server) = load_network(profile)? {
        return Ok(Some(server));
    }
    load_network(package)
}

pub fn load_network(path: &Path) -> Result<Option<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("Network settings exceed size limit".into());
    }
    let settings: Network = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    sensor_render::websocket_url(&settings.server).map_err(|e| e.to_string())?;
    Ok(Some(settings.server))
}

pub fn save_network(path: &Path, server: &str) -> Result<(), String> {
    sensor_render::websocket_url(server).map_err(|e| e.to_string())?;
    let settings = Network {
        server: server.trim().to_owned(),
    };
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing settings directory")?)
            .map_err(|e| e.to_string())?;
    temp.write_all(&bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub name: String,
    pub id: String,
    pub key: String,
    pub address: String,
}

impl Contact {
    pub fn validate(&self) -> Result<(), String> {
        super::peer(&self.id, &self.key)?;
        if self.name.is_empty() || self.name.len() > 128 || self.name.chars().any(char::is_control)
        {
            return Err("Contact name must contain 1-128 bytes without control characters.".into());
        }
        if !self.address.is_empty() {
            self.address
                .parse::<std::net::SocketAddr>()
                .map_err(|_| "Use an IP address and port, or leave empty for Internet routing.")?;
        }
        Ok(())
    }
}

pub fn known_peer(
    id: &str,
    key: &str,
    contacts: &[Contact],
) -> Result<sensor_session::ExpectedPeer, String> {
    let parsed = super::peer(
        id,
        if key.trim().is_empty() {
            "0000000000000000000000000000000000000000000000000000000000000000"
        } else {
            key
        },
    )?;
    if let Some(contact) = contacts.iter().find(|contact| {
        super::peer(&contact.id, &contact.key)
            .is_ok_and(|known| known.device_id == parsed.device_id)
    }) {
        let known = super::peer(&contact.id, &contact.key)?;
        if parsed.public_key != [0; 32] && parsed.public_key != known.public_key {
            return Err(
                "This device's key differs from its saved identity. Connection blocked.".into(),
            );
        }
        return Ok(known);
    }
    Ok(parsed)
}

pub fn load(path: &Path) -> Result<Vec<Contact>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Contact store exceeds its size limit.".into());
    }
    let contacts: Vec<Contact> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if contacts.len() > 100 {
        return Err("Contact limit is 100.".into());
    }
    for contact in &contacts {
        contact.validate()?;
    }
    Ok(contacts)
}

pub fn save(path: &Path, contacts: &[Contact]) -> Result<(), String> {
    if contacts.len() > 100 {
        return Err("Contact limit is 100.".into());
    }
    for contact in contacts {
        contact.validate()?;
    }
    let bytes = serde_json::to_vec_pretty(contacts).map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Contact store exceeds its size limit.".into());
    }
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing contact directory")?)
            .map_err(|e| e.to_string())?;
    temp.write_all(&bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_only_connections_use_saved_key_and_refuse_replacement() {
        let contact = Contact {
            name: "Known".into(),
            id: "123456789".into(),
            key: "ab".repeat(32),
            address: String::new(),
        };
        assert!(contact.validate().is_ok());
        let contacts = vec![contact];
        assert_eq!(
            known_peer("123 456 789", "", &contacts).unwrap().public_key,
            [0xab; 32]
        );
        assert!(known_peer("123456789", &"cd".repeat(32), &contacts).is_err());
        assert_eq!(
            known_peer("987654321", "", &contacts).unwrap().public_key,
            [0; 32]
        );
        assert!(known_peer("invalid", "", &contacts).is_err());
    }
    #[test]
    fn network_precedence_is_lazy_and_invalid_selected_source_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile.json");
        let package = root.path().join("package.json");
        std::fs::write(&package, b"corrupt").unwrap();
        let server = "https://relay.example.com";
        assert_eq!(
            resolve_network(Some(server.into()), &profile, &package).unwrap(),
            Some(server.into())
        );
        save_network(&profile, server).unwrap();
        assert_eq!(
            resolve_network(None, &profile, &package).unwrap(),
            Some(server.into())
        );
        assert!(resolve_network(Some("http://example.com".into()), &profile, &package).is_err());
        std::fs::write(&profile, b"corrupt").unwrap();
        assert!(resolve_network(None, &profile, &package).is_err());
    }
    #[test]
    fn contacts_round_trip_and_corruption_never_resets_silently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contacts.json");
        let contact = Contact {
            name: "Lab".into(),
            id: "123456789".into(),
            key: "ab".repeat(32),
            address: "127.0.0.1:5909".into(),
        };
        save(&path, &[contact]).unwrap();
        assert_eq!(load(&path).unwrap()[0].id, "123456789");
        std::fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
    }
}
