//! SENSOR command-line endpoint. No listener starts implicitly.
fn main() {
    if let Err(error) = run() {
        eprintln!("SENSOR Remote Access: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("This endpoint build requires Windows DPAPI".into())
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use sensor_audit::{verify_file, AuditLog};
    use sensor_client::{request, send_file, LocalInteraction, Message, Mode, MAX_CHAT_BYTES};
    use sensor_core::DeviceId;
    use sensor_files::{Receiver, TransferId};
    use sensor_identity::IdentityFileStore;
    use sensor_session::ExpectedPeer;
    use sensor_transport::connection::{SecureConnection, DEFAULT_TIMEOUT};
    use sensor_windows::UserDpapi;
    use std::{
        io::{self, BufRead, Read, Write},
        net::{SocketAddr, TcpListener},
        path::Path,
        time::Duration,
    };

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn parse_hex<const N: usize>(text: &str) -> Result<[u8; N], Box<dyn std::error::Error>> {
        if text.len() != N * 2 || !text.is_ascii() {
            return Err("invalid hexadecimal key or transfer ID".into());
        }
        let mut bytes = [0; N];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
        }
        Ok(bytes)
    }
    fn input() -> Option<String> {
        let mut bytes = Vec::new();
        let count = io::stdin()
            .lock()
            .take((MAX_CHAT_BYTES + 2) as u64)
            .read_until(b'\n', &mut bytes)
            .ok()?;
        if count == 0 || bytes.len() > MAX_CHAT_BYTES + 1 {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        Some(text.trim_end_matches(['\r', '\n']).to_owned())
    }
    struct Console;
    impl LocalInteraction for Console {
        fn accept(&mut self, peer: ExpectedPeer, mode: Mode) -> bool {
            println!("Incoming SENSOR session\nDevice: {}\nVerified public key: {}\nRequested mode: {mode:?}\nType ACCEPT to grant this session; anything else rejects.", peer.device_id, hex(&peer.public_key));
            input().as_deref() == Some("ACCEPT")
        }
        fn chat_reply(&mut self, text: &str) -> Option<String> {
            println!("Remote: {}", text.escape_debug());
            print!("You (/quit to disconnect): ");
            let _ = io::stdout().flush();
            input().filter(|text| text != "/quit")
        }
        fn transfer_progress(&mut self, received: u64, total: u64) {
            print!("\rReceived {received}/{total} bytes");
            let _ = io::stdout().flush();
        }
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "help" {
        println!("SENSOR Remote Access {}\nSENSOR TECHNOLOGY\nDesigned by ENG Mohamed Sayed\n\nCommands:\n  identity <config-dir>\n  host <config-dir> <listen-ip:port> <peer-id> <peer-public-key> <receive-dir>\n  send <config-dir> <host-ip:port> <peer-id> <peer-public-key> <file> [resume-id]\n  chat <config-dir> <host-ip:port> <peer-id> <peer-public-key>\n  audit <config-dir>\n  --version\n\nPublic keys must be verified with the other device owner.\nHost serves one explicitly accepted session. Files are limited to 1 GiB per offer.\nTransport: direct TCP with endpoint encryption; no rendezvous or relay configured.", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args[0] == "--version" && args.len() == 1 {
        println!("SENSOR Remote Access {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let valid = match args[0].as_str() {
        "identity" | "audit" => args.len() == 2,
        "host" => args.len() == 6,
        "send" => args.len() == 6 || args.len() == 7,
        "chat" => args.len() == 5,
        _ => false,
    };
    if !valid {
        return Err("Unknown command or invalid arguments; use --help".into());
    }
    let config = Path::new(&args[1]);
    let identity =
        IdentityFileStore::new(config.join("identity.bin"), UserDpapi).load_or_create()?;
    if args[0] == "identity" {
        println!(
            "Device ID: {}\nPublic key: {}\nStorage: Windows user DPAPI\nNetwork: offline",
            identity.device_id(),
            hex(&identity.keypair().public_key())
        );
        return Ok(());
    }
    if args[0] == "audit" {
        let head = verify_file(
            &config.join("audit.jsonl"),
            &identity.keypair().public_key(),
        )?;
        println!(
            "Verified records: {}\nChain head: {}",
            head.records,
            hex(&head.hash)
        );
        return Ok(());
    }
    let address: SocketAddr = args[2].parse()?;
    let peer = ExpectedPeer {
        device_id: DeviceId::try_from(args[3].parse::<u32>()?)?,
        public_key: parse_hex(&args[4])?,
    };
    if args[0] == "host" {
        let receiver = Receiver::open(Path::new(&args[5]))?;
        let mut log = AuditLog::open(&config.join("audit.jsonl"), identity.keypair())?;
        let listener = TcpListener::bind(address)?;
        println!("SENSOR listening on {}\nLocal Device ID: {}\nOnly configured peer {} can authenticate. Waiting for a connection...", listener.local_addr()?, identity.device_id(), peer.device_id);
        let (stream, _) = listener.accept()?;
        let mut connection = SecureConnection::accept(stream, &identity, peer, DEFAULT_TIMEOUT)?;
        connection.set_timeout(Duration::from_secs(120))?;
        sensor_client::serve(
            connection,
            identity.device_id(),
            &receiver,
            &mut log,
            &mut Console,
        )?;
        println!("\nSession closed. Audit records: {}", log.head().records);
        return Ok(());
    }
    let mut connection = SecureConnection::connect(address, &identity, peer, DEFAULT_TIMEOUT)?;
    connection.set_timeout(Duration::from_secs(120))?;
    println!(
        "Verified peer {}. Direct TCP; ChaCha20-Poly1305. Waiting for local acceptance...",
        peer.device_id
    );
    if args[0] == "send" {
        let resume = args
            .get(6)
            .map(|value| parse_hex(value).map(TransferId))
            .transpose()?;
        let mut announced = false;
        let completed = send_file(
            &mut connection,
            Path::new(&args[5]),
            resume,
            |id, offset, total| {
                if !announced {
                    println!("Transfer ID (keep to resume): {}", hex(&id.0));
                    announced = true;
                }
                print!("\rSent {offset}/{total} bytes");
                let _ = io::stdout().flush();
            },
        )?;
        println!(
            "\nRemote endpoint verified and saved {} bytes (SHA-256 {}).",
            completed.size,
            hex(&completed.sha256)
        );
    } else {
        request(&mut connection, Mode::Chat)?;
        loop {
            print!("You (/quit to disconnect): ");
            io::stdout().flush()?;
            let Some(text) = input().filter(|text| text != "/quit") else {
                connection.send(&Message::Close)?;
                break;
            };
            connection.send(&Message::Chat(text))?;
            match connection.receive()? {
                Message::Chat(reply) => println!("Remote: {}", reply.escape_debug()),
                Message::Close => break,
                _ => return Err("unexpected chat response".into()),
            }
        }
    }
    Ok(())
}
