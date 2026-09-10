use image::{DynamicImage, ImageBuffer, Rgba};
use xcap::Monitor;

/// Kích thước cạnh dài tối đa (px) gửi lên AI vision model. Các provider (VD
/// OpenAI, Anthropic, Gemini) đều tự resize/tile ảnh input về quanh mốc
/// ~1568px cạnh dài trước khi tokenize — gửi ảnh lớn hơn mốc này không tăng
/// thêm chất lượng đọc hiểu (model tự thu nhỏ y hệt phía server), chỉ tốn
/// thêm băng thông upload + thời gian encode/decode 2 chiều một cách vô ích.
/// Downscale sẵn ở client giúp giảm payload đáng kể với vùng crop lớn (VD
/// chọn gần nửa màn hình 4K) mà không đổi chất lượng model nhận được.
const MAX_DIMENSION: u32 = 1568;

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
    let resized = downscale_if_needed(cropped);
    encode_png(&resized.to_rgba8())
}

/// Thu nhỏ ảnh nếu cạnh dài vượt `MAX_DIMENSION`, giữ nguyên tỉ lệ. Dùng
/// filter Lanczos3 (chất lượng cao nhất trong `image` crate, đổi lại chậm hơn
/// filter rẻ như Triangle/Nearest) — ưu tiên giữ chữ trong ảnh (VD code,
/// văn bản) còn đọc rõ nét sau khi thu nhỏ, vì use-case chính của app là
/// OCR/đọc nội dung ảnh, không phải ảnh chụp phong cảnh.
fn downscale_if_needed(img: DynamicImage) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let long_side = w.max(h);
    if long_side <= MAX_DIMENSION {
        return img;
    }

    let scale = MAX_DIMENSION as f64 / long_side as f64;
    let new_w = ((w as f64 * scale).round() as u32).max(1);
    let new_h = ((h as f64 * scale).round() as u32).max(1);

    let t0 = std::time::Instant::now();
    let resized = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
    eprintln!(
        "[snip-ai][timing] downscale ảnh {w}x{h} -> {new_w}x{new_h}: {:.0}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    resized
}

fn encode_png(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>, String> {
    let dynamic = DynamicImage::ImageRgba8(image.clone());
    let mut buf = std::io::Cursor::new(Vec::new());
    dynamic
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("Encode PNG lỗi: {e}"))?;
    Ok(buf.into_inner())
}
