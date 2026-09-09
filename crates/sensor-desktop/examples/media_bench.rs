//! Opt-in local real desktop capture/encode/decode benchmark. No input or networking.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use sensor_session::permissions::{Consent, Permissions};
    use sensor_windows::{codec, desktop};
    use std::time::{Duration, Instant};
    let args: Vec<_> = std::env::args().collect();
    let software = args.iter().any(|arg| arg == "--software");
    let synthetic = args.iter().any(|arg| arg == "--synthetic");
    let mut consent = Consent::pending(Permissions::screen_sharing());
    consent.accept(Permissions::screen_sharing())?;
    let display = desktop::displays(&consent)?.remove(0);
    let mut capture = if synthetic {
        None
    } else {
        Some(desktop::Capture::new(0, &consent)?)
    };
    let (width, height) = sensor_media::stream_size(display.width, display.height)?;
    let mut encoder = codec::H264Encoder::new(width, height, 60, 12_000_000, !software)?;
    let mut decoder = codec::H264Decoder::new(width, height, 60)?;
    let mut captured = 0u64;
    let mut encoded = 0u64;
    let mut decoded = 0u64;
    let mut bytes = 0u64;
    let mut capture_us = 0u128;
    let mut prepare_us = 0u128;
    let mut encode_us = 0u128;
    let mut decode_us = 0u128;
    let started = Instant::now();
    println!("MEDIA_BENCH_START source={}x{} encoded={}x{} target_fps=60 hardware={} encoder={} synthetic={} actual_desktop={}", display.width, display.height, width,height,encoder.hardware,encoder.name,synthetic,!synthetic);
    let cpu_start = codec::process_cpu_seconds()?;
    let mut frame_due = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if Instant::now() < frame_due {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        frame_due = Instant::now() + Duration::from_secs_f64(1.0 / 60.0);
        let at = Instant::now();
        let frame = if synthetic {
            Some(sensor_media::BgraFrame {
                width,
                height,
                bytes: [30, (captured % 128) as u8 + 50, 200, 255]
                    .repeat((width * height) as usize),
            })
        } else {
            capture.as_mut().expect("real capture").next(&consent)?
        };
        capture_us += at.elapsed().as_micros();
        let mut packets = encoder.available()?;
        if let Some(frame) = frame {
            captured += 1;
            let at = Instant::now();
            let frame = sensor_media::rotate_scale_bgra(
                frame,
                capture.as_ref().map_or(0, |c| c.rotation),
                width,
                height,
            )?;
            let nv12 = sensor_media::bgra_to_nv12(&frame)?;
            prepare_us += at.elapsed().as_micros();
            let at = Instant::now();
            packets.extend(encoder.encode(&nv12, started.elapsed().as_micros() as i64 * 10)?);
            encode_us += at.elapsed().as_micros();
        }
        for packet in packets {
            encoded += 1;
            bytes += packet.bytes.len() as u64;
            let at = Instant::now();
            let frames = decoder.decode(&packet)?;
            decode_us += at.elapsed().as_micros();
            for frame in &frames {
                if (frame.width, frame.height) != (width, height) {
                    return Err("decoded visible dimensions mismatch".into());
                }
            }
            decoded += frames.len() as u64;
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let cpus = std::thread::available_parallelism()?.get() as f64;
    let cpu = (codec::process_cpu_seconds()? - cpu_start) / seconds / cpus * 100.;
    println!("MEDIA_BENCH_RESULT seconds={seconds:.3} captured={captured} encoded={encoded} decoded={decoded} encode_fps={:.2} decode_fps={:.2} bitrate_mbps={:.3} cpu_percent={cpu:.2} capture_ms_per_frame={:.3} prepare_ms_per_frame={:.3} encode_ms_per_frame={:.3} decode_ms_per_frame={:.3} display_fps=not_measured gpu_utilization=not_measured",encoded as f64/seconds,decoded as f64/seconds, bytes as f64*8./seconds/1e6,capture_us as f64/captured.max(1) as f64/1000.,prepare_us as f64/captured.max(1) as f64/1000.,encode_us as f64/captured.max(1) as f64/1000.,decode_us as f64/encoded.max(1) as f64/1000.);
    if decoded == 0 {
        return Err("no real decoded output".into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows native benchmark only");
}
