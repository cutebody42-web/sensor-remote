//! Local release operator tool. Private publisher key stays user-DPAPI protected,
//! outside the repository. This executable is not included in app packages.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use sensor_desktop::updates;
    use sensor_identity::IdentityFileStore;
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        path::Path,
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, key_path] if mode == "create-key" => {
            if Path::new(key_path).exists() { return Err("Refusing to replace an existing publisher key".into()); }
            std::fs::create_dir_all(Path::new(key_path).parent().ok_or("No key directory")?)?;
            let key=IdentityFileStore::new(key_path, sensor_windows::UserDpapi).load_or_create()?;
            println!("{}", sensor_desktop::hex(&key.keypair().public_key()));
        }
        [mode, key_path, installer, version, output] if mode == "sign" => {
            let key=IdentityFileStore::new(key_path, sensor_windows::UserDpapi).load()?;
            if key.keypair().public_key() != updates::publisher_key()? {return Err("Signing key is not the compiled publisher key".into());}
            let (installer_bytes,installer_sha256)=updates::digest(File::open(installer)?)?;
            let issued_unix=updates::now()?;
            let release=updates::Release {schema:1,product:"SENSOR Remote Access".into(),target:updates::TARGET.into(),version:updates::version(version)?,issued_unix,expires_unix:issued_unix+30*86400,installer_bytes,installer_sha256};
            let bytes=updates::sign(&release,key.keypair())?;
            let mut file=OpenOptions::new().write(true).create_new(true).open(output)?;
            file.write_all(&bytes)?; file.sync_all()?;
            println!("SIGNED_RELEASE version={version} bytes={installer_bytes} sha256={}",sensor_desktop::hex(&installer_sha256));
        }
        [mode, manifest, installer, installed, destination] if mode == "verify-stage" => {
            let (path,release)=updates::stage(Path::new(manifest),Path::new(installer),Path::new(destination),updates::version(installed)?)?;
            println!("VERIFIED_UPDATE version={:?} path={}",release.version,path.display());
        }
        _ => return Err("Usage: sign_release create-key <DPAPI-key> | sign <DPAPI-key> <installer> <version> <new-manifest> | verify-stage <manifest> <installer> <installed-version> <staging-directory>".into())
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Release signing requires the publisher's Windows DPAPI profile.");
}
