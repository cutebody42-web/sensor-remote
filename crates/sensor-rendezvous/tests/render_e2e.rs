use sensor_identity::DeviceIdentity;
use sensor_render::{accept, connect};
use sensor_session::ExpectedPeer;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

fn server() -> (Child, String) {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sensor-rendezvous"))
        .env("PORT", port.to_string())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if response.contains("\"status\":\"ok\"") {
                return (child, base);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("rendezvous server did not become healthy");
}

#[test]
fn two_render_clients_exchange_opaque_bytes_over_wss_profile() {
    let (mut process, server) = server();
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let expected = ExpectedPeer {
        device_id: host.device_id(),
        public_key: host.keypair().public_key(),
    };
    let host_server = server.clone();
    let host_thread =
        thread::spawn(move || accept(&host_server, &host, Duration::from_secs(10)).unwrap());
    thread::sleep(Duration::from_millis(100));
    let mut client_stream = connect(&server, &client, expected, Duration::from_secs(10)).unwrap();
    let mut host_stream = host_thread.join().unwrap();
    let sent = b"SENSOR encrypted endpoint bytes";
    client_stream.write_all(sent).unwrap();
    let mut received = vec![0; sent.len()];
    host_stream.read_exact(&mut received).unwrap();
    assert_eq!(received, sent);
    let reply = b"reply stays opaque to the relay";
    host_stream.write_all(reply).unwrap();
    let mut received = vec![0; reply.len()];
    client_stream.read_exact(&mut received).unwrap();
    assert_eq!(received, reply);
    drop(client_stream);
    drop(host_stream);
    let _ = process.kill();
    let _ = process.wait();
}
