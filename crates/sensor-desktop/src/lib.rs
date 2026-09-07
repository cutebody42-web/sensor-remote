//! Native desktop's testable worker boundary; no browser, HTML, or web server.
pub mod settings;
pub mod updates;
pub mod worker;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let value = value.trim();
    if !value.is_ascii() || value.len() != N * 2 {
        return Err(format!("Expected {} hexadecimal characters.", N * 2));
    }
    let mut result = [0; N];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "Invalid hexadecimal value.".to_owned())?;
    }
    Ok(result)
}

pub fn peer(id: &str, key: &str) -> Result<sensor_session::ExpectedPeer, String> {
    let id: String = id.chars().filter(|c| !c.is_whitespace()).collect();
    Ok(sensor_session::ExpectedPeer {
        device_id: sensor_core::DeviceId::try_from(
            id.parse::<u32>()
                .map_err(|_| "Enter the nine-digit device ID.")?,
        )
        .map_err(|e| e.to_string())?,
        public_key: parse_hex(key)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peer_input_is_strict_and_allows_display_spaces() {
        assert!(peer("123 456 789", &"ab".repeat(32)).is_ok());
        assert!(peer("999", &"ab".repeat(32)).is_err());
        assert!(peer("123456789", &"zz".repeat(32)).is_err());
        assert!(peer("123456789", &"é".repeat(32)).is_err());
    }
}
