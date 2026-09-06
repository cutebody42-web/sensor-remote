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
        self.address
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "Use an IP address and port.")?;
        Ok(())
    }
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
