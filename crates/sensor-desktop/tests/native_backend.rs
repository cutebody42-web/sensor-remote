#[cfg(windows)]
#[test]
fn windows_build_has_an_actual_d3d12_backend() {
    assert!(wgpu::Instance::enabled_backend_features().contains(wgpu::Backends::DX12));
}

#[test]
fn embedded_branding_is_the_exact_owner_supplied_logo() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../assets/sensor-logo.jpeg");
    assert_eq!(
        sensor_desktop::hex(&Sha256::digest(bytes)),
        "7cd4b3ad7791f7347c0c0a09073bf74d33eebfe8337a0bd9b520f947e8a3ca81"
    );
}
