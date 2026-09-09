use image::{DynamicImage, ImageBuffer, Rgba};
use xcap::Monitor;

/// Chụp toàn bộ monitor "chính" (primary). Bản MVP chỉ hỗ trợ 1 màn hình,
/// giống giới hạn đã ghi nhận ở bản Python — mở rộng đa màn hình là việc của
/// bản sau (cần chụp monitor đang chứa con trỏ chuột thay vì luôn lấy primary).
pub fn capture_primary_monitor() -> Result<(Vec<u8>, i32, i32, u32, u32), String> {
    let monitors = Monitor::all().map_err(|e| format!("Không liệt kê được màn hình: {e}"))?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary())
        .or_else(|| monitors.first())
        .ok_or_else(|| "Không tìm thấy màn hình nào".to_string())?;

    let x = monitor.x();
    let y = monitor.y();

    let image = monitor
        .capture_image()
        .map_err(|e| format!("Chụp màn hình thất bại: {e}"))?;

    let width = image.width();
    let height = image.height();

    let png_bytes = encode_png(&image)?;
    Ok((png_bytes, x, y, width, height))
}

/// Crop ảnh PNG gốc (bytes) theo bbox (toạ độ TƯƠNG ĐỐI trong ảnh, không phải
/// toạ độ màn hình), trả về PNG bytes của phần đã crop.
///
/// QUAN TRỌNG: `x,y,w,h` do frontend tự tính (nhân toạ độ CSS px với
/// devicePixelRatio) nên có thể lệch 1-2px làm vượt biên ảnh gốc (VD kéo chọn
/// sát mép màn hình rồi làm tròn số lên). `DynamicImage::crop_imm` KHÔNG tự
/// clamp — vùng vượt biên có thể gây panic khi encode (`to_rgba8()` đọc pixel
/// ngoài bounds). Phải tự clamp trước khi crop để không bao giờ panic.
pub fn crop_png(png_bytes: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(png_bytes).map_err(|e| format!("Ảnh gốc lỗi: {e}"))?;
    let (img_w, img_h) = (img.width(), img.height());

    let x = x.min(img_w.saturating_sub(1));
    let y = y.min(img_h.saturating_sub(1));
    let w = w.max(1).min(img_w.saturating_sub(x)).max(1);
    let h = h.max(1).min(img_h.saturating_sub(y)).max(1);

    let cropped = img.crop_imm(x, y, w, h);
    encode_png(&cropped.to_rgba8())
}

fn encode_png(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>, String> {
    let dynamic = DynamicImage::ImageRgba8(image.clone());
    let mut buf = std::io::Cursor::new(Vec::new());
    dynamic
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("Encode PNG lỗi: {e}"))?;
    Ok(buf.into_inner())
}
