//! SENSOR's self-hosted, provisioned relay endpoint.
//!
//! This binary is deliberately small and explicit. It does not provide a
//! public directory or accept arbitrary destinations: an operator provisions
//! exactly two endpoint public keys and the relay forwards their already
//! encrypted byte streams.

use rand_core::{OsRng, RngCore};
use sensor_crypto::IdentityKeypair;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    time::Duration,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let value = value.trim();
    if value.len() != N * 2 || !value.is_ascii() {
        return Err(format!("expected {} hexadecimal characters", N * 2));
    }
    let mut bytes = [0_u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid hexadecimal value".to_owned())?;
    }
    Ok(bytes)
}

fn seed_path(config: &Path) -> PathBuf {
    config.join("relay-seed.bin")
}

fn load_or_create(config: &Path) -> Result<IdentityKeypair, Box<dyn std::error::Error>> {
    fs::create_dir_all(config)?;
    let path = seed_path(config);
    let seed = match fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        Ok(_) => return Err("relay-seed.bin must contain exactly 32 bytes".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut generated = [0_u8; 32];
            OsRng.fill_bytes(&mut generated);
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            file.write_all(&generated)?;
            file.sync_all()?;
            generated.to_vec()
        }
        Err(error) => return Err(error.into()),
    };
    let seed: [u8; 32] = seed.try_into().map_err(|_| "invalid relay seed")?;
    Ok(IdentityKeypair::from_seed(seed))
}

fn usage() {
    println!(
        "SENSOR Relay {}\n\n\
Commands:\n  identity <config-dir>\n  serve <config-dir> <bind-ip:port> <left-public-key> <right-public-key> [pair-timeout-seconds] [idle-timeout-seconds]\n\n\
The relay-seed.bin file is created once and must be protected by the server operator.\n\
Each endpoint must configure this relay address and public key, and both endpoint public keys must be provisioned here.",
        env!("CARGO_PKG_VERSION")
    );
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") => {
            usage();
            Ok(())
        }
        Some("--version") if args.len() == 1 => {
            println!("SENSOR Relay {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("identity") if args.len() == 2 => {
            let identity = load_or_create(Path::new(&args[1]))?;
            println!(
                "Relay public key: {}\nKey file: {}",
                hex(&identity.public_key()),
                seed_path(Path::new(&args[1])).display()
            );
            Ok(())
        }
        Some("serve") if (5..=7).contains(&args.len()) => {
            let config = Path::new(&args[1]);
            let bind: SocketAddr = args[2].parse()?;
            let left = parse_hex::<32>(&args[3])?;
            let right = parse_hex::<32>(&args[4])?;
            if left == right {
                return Err("the two provisioned endpoint keys must differ".into());
            }
            let pairing_timeout = args
                .get(5)
                .map(|value| value.parse::<u64>())
                .transpose()?
                .unwrap_or(120)
                .clamp(5, 3600);
            let idle_timeout = args
                .get(6)
                .map(|value| value.parse::<u64>())
                .transpose()?
                .unwrap_or(300)
                .clamp(5, 3600);
            let identity = load_or_create(config)?;
            let listener = TcpListener::bind(bind)?;
            println!(
                "SENSOR Relay listening on {}\nRelay public key: {}\nProvisioned endpoints: {} and {}",
                listener.local_addr()?,
                hex(&identity.public_key()),
                hex(&left),
                hex(&right)
            );
            sensor_relay::serve_forever(
                listener,
                &identity,
                [left, right],
                Duration::from_secs(pairing_timeout),
                Duration::from_secs(idle_timeout),
            )?;
            Ok(())
        }
        _ => {
            usage();
            Err("unknown command or invalid arguments".into())
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("SENSOR Relay: {error}");
        std::process::exit(1);
    }
}
