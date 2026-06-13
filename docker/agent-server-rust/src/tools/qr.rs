use base64::Engine;
use image::DynamicImage;

/// Result of QR decode.
pub struct QrDecodeResult {
    pub data: String,
    pub binary_data: Vec<u8>,
}

/// Decode QR code from base64-encoded PNG image.
pub fn decode_qr_from_base64(base64_str: &str) -> Option<QrDecodeResult> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_str)
        .ok()?;

    let img = image::load_from_memory(&bytes).ok()?;
    decode_qr_from_image(&img)
}

/// Decode QR code from an image.
fn decode_qr_from_image(img: &DynamicImage) -> Option<QrDecodeResult> {
    let luma = img.to_luma8();

    let mut prepared = rqrr::PreparedImage::prepare(luma);
    let grids = prepared.detect_grids();

    for grid in grids {
        if let Ok((_meta, content)) = grid.decode() {
            let binary_data = content.as_bytes().to_vec();
            return Some(QrDecodeResult {
                data: content,
                binary_data,
            });
        }
    }

    None
}

/// Convert QR data to a data URL for display (PNG image).
pub fn to_data_url(data: &str) -> Result<String, String> {
    use qrcode::QrCode;

    let code = QrCode::new(data.as_bytes()).map_err(|e| format!("QR encode error: {e}"))?;
    let image = code.render::<image::Luma<u8>>().build();

    let dynamic = DynamicImage::ImageLuma8(image);
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    dynamic
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode error: {e}"))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(format!("data:image/png;base64,{b64}"))
}
