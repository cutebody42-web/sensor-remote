fn main() {
    println!("cargo:rerun-if-changed=../../assets/sensor-logo.jpeg");
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Mechanical icon-format conversion only: preserve the complete supplied logo.
    let source = image::open("../../assets/sensor-logo.jpeg").expect("supplied SENSOR logo");
    let mut canvas = image::RgbaImage::from_pixel(256, 256, image::Rgba([255, 255, 255, 255]));
    let scaled = source
        .resize(256, 256, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    image::imageops::overlay(
        &mut canvas,
        &scaled,
        (256 - scaled.width()) as i64 / 2,
        (256 - scaled.height()) as i64 / 2,
    );
    let icon = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("sensor.ico");
    canvas
        .save_with_format(&icon, image::ImageFormat::Ico)
        .expect("encode Windows icon");
    winresource::WindowsResource::new()
        .set_icon(icon.to_str().unwrap())
        .set_manifest_file("app.manifest")
        .set("ProductName", "SENSOR Remote Access")
        .set("CompanyName", "SENSOR TECHNOLOGY")
        .set("FileDescription", "SENSOR Remote Access - Windows Desktop")
        .set(
            "LegalCopyright",
            "SENSOR TECHNOLOGY. Designed by ENG Mohamed Sayed.",
        )
        .compile()
        .expect("compile Windows resources");
}
