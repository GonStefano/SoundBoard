pub fn rgba_icon(size: u32) -> Result<(Vec<u8>, u32, u32), String> {
    if size == 0 {
        return Err("El tamaño del icono debe ser mayor que cero.".to_string());
    }

    let image = image::load_from_memory(include_bytes!("../soundboard.png"))
        .map_err(|error| format!("No se pudo decodificar soundboard.png: {error}"))?
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .into_rgba8();

    Ok((image.into_raw(), size, size))
}
