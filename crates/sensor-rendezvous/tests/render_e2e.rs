use sensor_identity::DeviceIdentity;
use sensor_render::{accept, connect};
use sensor_session::ExpectedPeer;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn server() -> (Server, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sensor-rendezvous"))
        .env("PORT", "0")
        .env("RELAY_MAX_BITRATE", "300000")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = send.send(line);
    });
    let process = Server(child);
    let line = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("server bound readiness deadline");
    let address: std::net::SocketAddr = line
        .trim()
        .strip_prefix("SENSOR_LISTEN_ADDRESS=")
        .unwrap()
        .parse()
        .unwrap();
    let port = address.port();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if response.contains("\"status\":\"ok\"") {
                return (process, base);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("rendezvous server did not become healthy");
}

#[test]
fn two_render_clients_exchange_opaque_bytes_over_local_websocket() {
    let (_process, server) = server();
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
}

#[test]
fn congested_video_direction_does_not_delay_reverse_input() {
    let (_process, server) = server();
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
    let mut client = connect(&server, &client, expected, Duration::from_secs(10)).unwrap();
    let mut host = host_thread.join().unwrap();
    host.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut video = host.try_clone().unwrap();
    let send_video = thread::spawn(move || {
        let _ = video.write_all(&vec![7; 256 * 1024]);
    });
    // The relay's video budget is only 300 kbit/s; do not read that direction.
    // A serial limiter would block reverse input behind seconds of video.
    thread::sleep(Duration::from_millis(150));
    for value in 0..12u8 {
        let at = Instant::now();
        client.write_all(&[value]).unwrap();
        let mut received = [0];
        host.read_exact(&mut received)
            .expect("bounded reverse input during video congestion");
        assert_eq!(received, [value]);
        assert!(at.elapsed() < Duration::from_millis(500));
        thread::sleep(Duration::from_millis(20));
    }
    let _ = client.shutdown(std::net::Shutdown::Both);
    let _ = host.shutdown(std::net::Shutdown::Both);
    send_video.join().unwrap();
}
