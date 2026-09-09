#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::env::args().any(|a| a == "--capture-my-visible-desktop") {
        return Err("Explicit local opt-in required: --capture-my-visible-desktop. This probe has no listener or remote access.".into());
    }
    use sensor_session::permissions::{Consent, Permissions};
    let os = sensor_windows::desktop::os_version()?;
    println!("LEGACY_CAPABILITY_PROBE os={os:?} application_support=UNVERIFIED");
    let mut consent = Consent::pending(Permissions::screen_sharing());
    consent.accept(Permissions::screen_sharing())?;
    let mut capture = sensor_windows::desktop::Capture::legacy(0,&consent)?;
    let frame = capture.next(&consent)?.ok_or("no GDI frame")?;
    let (width,height) = sensor_media::stream_size(frame.width,frame.height)?;
    let frame = sensor_media::rotate_scale_bgra(frame,0,width,height)?;
    let nv12 = sensor_media::bgra_to_nv12(&frame)?;
    let mut encoder = sensor_windows::codec::H264Encoder::new(width,height,15,4_000_000,false)?;
    let mut packets = encoder.encode(&nv12,0)?;
    packets.extend(encoder.drain()?);
    let mut decoder = sensor_windows::codec::H264Decoder::new(width,height,15)?;
    let mut frames = 0;
    for packet in packets { frames += decoder.decode(&packet)?.len(); }
    if frames == 0 { return Err("legacy codec produced no decoded frame".into()); }
    println!("LEGACY_CAPTURE_CODEC_PASS backend={} visible={}x{} decoded={} network=NOT_TESTED gui=NOT_INCLUDED win7_supported=NO",capture.backend_name(),width,height,frames);
    Ok(())
}
#[cfg(not(windows))]
fn main() { eprintln!("Windows-only capability probe"); }
