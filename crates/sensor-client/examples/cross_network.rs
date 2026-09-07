//! Bounded synthetic-file fixture. Not shipped in the application package.
use sensor_client::{LocalInteraction, Mode};
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use sensor_transport::connection::SecureConnection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

const SIZE: usize = 8 * 1024 * 1024 + 17;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicEndpoint {
    server: String,
    device_id: u32,
    public_key: String,
}
fn payload() -> Vec<u8> {
    (0..SIZE).map(|n| (n % 251) as u8).collect()
}
struct SyntheticOnly;
impl LocalInteraction for SyntheticOnly {
    fn accept(&mut self, _: ExpectedPeer, mode: Mode) -> bool {
        matches!(mode, Mode::FileTransfer)
    }
    fn chat_reply(&mut self, _: &str) -> Option<String> {
        None
    }
}
fn host(server: &str, public_path: &Path) -> Result<(), Box<dyn Error>> {
    if !server.starts_with("https://") {
        return Err("public HTTPS endpoint required".into());
    }
    let identity = DeviceIdentity::generate();
    let root = tempfile::tempdir()?;
    let receiver = sensor_files::Receiver::open(root.path())?;
    let mut audit =
        sensor_audit::AuditLog::open(&root.path().join("audit.jsonl"), identity.keypair())?;
    let endpoint = PublicEndpoint {
        server: server.into(),
        device_id: identity.device_id().to_string().replace(' ', "").parse()?,
        public_key: sensor_render::encode_hex(&identity.keypair().public_key()),
    };
    let bytes = serde_json::to_vec_pretty(&endpoint)?;
    let started = Instant::now();
    let ready = || {
        if let Some(parent) = public_path.parent() {
            if let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) {
                use std::io::Write;
                if file.write_all(&bytes).is_ok() {
                    let _ = file.persist(public_path);
                }
            }
        }
    };
    let stream = sensor_render::accept_with_control(
        server,
        &identity,
        Duration::from_secs(90),
        &|| started.elapsed() > Duration::from_secs(600),
        &ready,
    )?;
    let connection = SecureConnection::accept_unpinned(stream, &identity, Duration::from_secs(90))?;
    sensor_client::serve(
        connection,
        identity.device_id(),
        &receiver,
        &mut audit,
        &mut SyntheticOnly,
    )?;
    let actual = fs::read(root.path().join("sensor-cross-network.bin"))?;
    if actual != payload() {
        return Err("received synthetic file mismatch".into());
    }
    println!(
        "CROSS_NETWORK_HOST_PASS platform={} bytes={} sha256={} audit_records={}",
        std::env::consts::OS,
        actual.len(),
        sensor_render::encode_hex(&Sha256::digest(&actual)),
        audit.head().records
    );
    Ok(())
}
fn client(public_path: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(public_path)?;
    if bytes.len() > 4096 {
        return Err("public endpoint file too large".into());
    }
    let endpoint: PublicEndpoint = serde_json::from_slice(&bytes)?;
    if !endpoint.server.starts_with("https://") {
        return Err("public HTTPS endpoint required".into());
    }
    let identity = DeviceIdentity::generate();
    let peer = ExpectedPeer {
        device_id: endpoint.device_id.try_into()?,
        public_key: sensor_render::decode_public_key(&endpoint.public_key)?,
    };
    let root = tempfile::tempdir()?;
    let path = root.path().join("sensor-cross-network.bin");
    let bytes = payload();
    fs::write(&path, &bytes)?;
    let started = Instant::now();
    let stream =
        sensor_render::connect(&endpoint.server, &identity, peer, Duration::from_secs(90))?;
    let mut connection =
        SecureConnection::initiate(stream, &identity, peer, Duration::from_secs(90))?;
    let manifest = sensor_client::send_file(&mut connection, &path, None, |_, _, _| {})?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    if manifest.size != SIZE as u64 || manifest.sha256 != digest {
        return Err("receiver acknowledgement mismatch".into());
    }
    println!(
        "CROSS_NETWORK_CLIENT_PASS platform={} bytes={} sha256={} duration_ms={}",
        std::env::consts::OS,
        manifest.size,
        sensor_render::encode_hex(&manifest.sha256),
        started.elapsed().as_millis()
    );
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, server, file] if mode == "host" => host(server, Path::new(file)),
        [mode, file] if mode == "client" => client(Path::new(file)),
        _ => Err(
            "Usage: cross_network host <https-server> <public-json> | client <public-json>".into(),
        ),
    }
}
