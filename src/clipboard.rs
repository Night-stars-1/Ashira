use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ExtendedColorType, ImageEncoder, codecs::png::{CompressionType, FilterType, PngEncoder}};

pub enum Contents {
    Text(String),
    Image(ImageData),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub data: String,
}

pub fn read() -> Result<Contents, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("无法打开剪贴板：{error}"))?;
    match clipboard.get_image() {
        Ok(image) => image_data(image).map(Contents::Image),
        Err(arboard::Error::ContentNotAvailable) => match clipboard.get_text() {
            Ok(text) => Ok(Contents::Text(text)),
            Err(arboard::Error::ContentNotAvailable) => Err("剪贴板中没有可用的图片或文本".to_string()),
            Err(error) => Err(format!("读取剪贴板文本失败：{error}")),
        },
        Err(error) => Err(format!("读取剪贴板图片失败：{error}")),
    }
}

/// Terminals such as Orca paste an image's file path instead of clipboard pixels.
pub fn image_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let quoted = text.len() >= 2 && matches!(text.as_bytes()[0], b'\'' | b'"') && text.as_bytes().first() == text.as_bytes().last();
    let text = if quoted { &text[1..text.len() - 1] } else { text };
    if text.is_empty() || text.contains(['\n', '\r']) { return None; }
    let path = Path::new(text);
    if !quoted && text.contains(char::is_whitespace) && !path.is_absolute() { return None; }
    let extension = path.extension()?.to_str()?;
    ["png", "jpg", "jpeg", "webp", "gif"].iter().any(|ext| extension.eq_ignore_ascii_case(ext)).then(|| path.to_owned())
}

/// Decode and encode off the UI thread, using the same full-resolution payload as clipboard images.
pub fn read_file(path: &Path) -> Result<Contents, String> {
    let rgba = image::open(path).map_err(|error| format!("读取图片文件失败（{}）：{error}", path.display()))?.into_rgba8();
    let (width, height) = rgba.dimensions();
    image_data(arboard::ImageData { width: width as usize, height: height as usize, bytes: rgba.into_raw().into() })
        .map(Contents::Image)
}

fn image_data(image: arboard::ImageData<'_>) -> Result<ImageData, String> {
    let width = u32::try_from(image.width).map_err(|_| "图片宽度过大".to_string())?;
    let height = u32::try_from(image.height).map_err(|_| "图片高度过大".to_string())?;
    let expected = image.width.checked_mul(image.height).and_then(|size| size.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(image.bytes.len()) {
        return Err("图片尺寸或像素数据无效".to_string());
    }

    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(&image.bytes, width, height, ExtendedColorType::Rgba8)
        .map_err(|error| format!("编码图片失败：{error}"))?;

    Ok(ImageData { width, height, data: STANDARD.encode(png) })
}
