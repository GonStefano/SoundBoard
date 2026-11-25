use std::env;

fn main() {
    println!("cargo:rerun-if-changed=soundboard.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("No se encontró el directorio del proyecto: {error}");
            std::process::exit(1);
        }
    };
    let icon_path = format!("{manifest_dir}/soundboard.ico");
    let mut resource = winres::WindowsResource::new();
    resource.set_icon(&icon_path);
    if env::var("TARGET")
        .map(|target| target == "x86_64-pc-windows-gnu")
        .unwrap_or(false)
    {
        resource.set_windres_path("x86_64-w64-mingw32-windres");
        resource.set_ar_path("x86_64-w64-mingw32-ar");
    }

    if let Err(error) = resource.compile() {
        eprintln!("No se pudo incrustar el icono de Soundboard: {error}");
        std::process::exit(1);
    }
}
