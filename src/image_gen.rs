//! `generate_image`: pictures from the `modelRoles.image` model's OpenAI-compatible
//! `images/generations` endpoint or, without one, from the session's Responses model
//! running the provider's built-in `image_generation` tool in a request of its own.
//! Files go to the workspace; interfaces show them through `GET /v1/sessions/{id}/raw`.

use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::api::{Client, Usage};

/// Generation regularly takes a minute or more for large or high-quality images.
const TIMEOUT: Duration = Duration::from_secs(300);
pub const DIR: &str = "generated-images";
const MAX_IMAGES: u64 = 4;

pub fn definition() -> Value {
    json!({
        "name": "generate_image",
        "description": "Generate images from a text prompt and save them in the workspace. Returns one `saved: <path>` line per image; the user sees the images in the conversation, so do not describe them back unless asked.",
        "input_schema": {
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "What to draw: subject, style, composition, text to render." },
                "size": { "type": "string", "description": "Optional, e.g. 1024x1024, 1536x1024, 1024x1536. Omit to use the model default." },
                "n": { "type": "integer", "minimum": 1, "maximum": MAX_IMAGES, "description": "Number of images, default 1." },
                "path": { "type": "string", "description": "Optional workspace-relative file path for a single image, e.g. assets/logo.png. Default: generated-images/<time>.png" }
            },
            "required": ["prompt"]
        }
    })
}

/// What one generation request produced.
pub struct Generated {
    pub saved: Vec<String>,
    pub notes: Vec<String>,
    pub usage: Option<Usage>,
}

/// How `generate_image` reaches an image model.
pub enum Source {
    /// `modelRoles.image`: its OpenAI-compatible `images/generations` endpoint.
    Images,
    /// No image role: the chat model itself, on Responses, through a separate request that
    /// forces the provider's built-in `image_generation` tool. Chat requests never carry
    /// that tool, so providers that reject it only fail this call.
    Hosted,
}

pub async fn generate(client: &Client, source: Source, input: &Value, cwd: &Path, cancel: &CancellationToken) -> Result<Generated, String> {
    let prompt = input["prompt"].as_str().map(str::trim).filter(|p| !p.is_empty()).ok_or("prompt is required")?;
    let n = input["n"].as_u64().unwrap_or(1).clamp(1, MAX_IMAGES);
    let path = input["path"].as_str().map(str::trim).filter(|p| !p.is_empty());
    if path.is_some() && n > 1 { return Err("path only applies to a single image (n = 1)".into()); }
    let size = input["size"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let (images, usage) = match source {
        Source::Images => images_endpoint(client, prompt, n, size, cancel).await?,
        Source::Hosted => hosted(client, prompt, n, size, cancel).await?,
    };
    let stamp = stamp();
    let mut saved = Vec::new();
    let mut notes = Vec::new();
    for (i, (bytes, revised)) in images.iter().enumerate() {
        let relative = match path {
            Some(path) => path.replace('\\', "/"),
            None => default_name(&stamp, i, images.len(), bytes),
        };
        saved.push(save(cwd, &relative, bytes)?);
        if let Some(revised) = revised.as_deref().filter(|r| !r.is_empty() && *r != prompt) {
            notes.push(format!("revised prompt: {revised}"));
        }
    }
    Ok(Generated { saved, notes, usage })
}

/// Picture bytes with the prompt the model actually drew from, when it reports one.
type Pictures = Vec<(Vec<u8>, Option<String>)>;

/// Token usage as both image endpoints report it; cached input is billed as a cache read.
fn usage(u: &Value) -> Option<Usage> {
    if !u.is_object() { return None; }
    let get = |v: &Value| v.as_u64().unwrap_or(0);
    let cached = get(&u["input_tokens_details"]["cached_tokens"]);
    Some(Usage { input: get(&u["input_tokens"]).saturating_sub(cached), output: get(&u["output_tokens"]), cache_read: cached, cache_write: 0 })
}

async fn images_endpoint(client: &Client, prompt: &str, n: u64, size: Option<&str>, cancel: &CancellationToken) -> Result<(Pictures, Option<Usage>), String> {
    let mut body = json!({ "model": client.model_id(), "prompt": prompt, "n": n });
    if let Some(size) = size { body["size"] = json!(size); }
    let reply = client.post_json("images/generations", &body, TIMEOUT, cancel).await?;
    let data = reply["data"].as_array().filter(|d| !d.is_empty()).ok_or("the image model returned no images")?;
    let mut pictures = Vec::new();
    for (i, item) in data.iter().enumerate() {
        let bytes = match (item["b64_json"].as_str(), item["url"].as_str()) {
            (Some(b64), _) => STANDARD.decode(b64.trim()).map_err(|e| format!("image {}: bad base64: {e}", i + 1))?,
            (None, Some(url)) => client.download(url, TIMEOUT, cancel).await?,
            _ => return Err(format!("image {}: neither b64_json nor url in the reply", i + 1)),
        };
        pictures.push((bytes, item["revised_prompt"].as_str().map(str::to_owned)));
    }
    Ok((pictures, usage(&reply["usage"])))
}

/// One Responses request per picture (the tool draws one per call), forced to use the
/// built-in `image_generation`; no history, no other tools.
async fn hosted(client: &Client, prompt: &str, n: u64, size: Option<&str>, cancel: &CancellationToken) -> Result<(Pictures, Option<Usage>), String> {
    let mut tool = json!({ "type": "image_generation" });
    if let Some(size) = size { tool["size"] = json!(size); }
    let body = json!({
        "model": client.model_id(),
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }],
        "tools": [tool],
        "tool_choice": { "type": "image_generation" },
        "store": false,
    });
    let mut pictures = Vec::new();
    let mut total: Option<Usage> = None;
    for _ in 0..n {
        let reply = client.post_json("responses", &body, TIMEOUT, cancel).await.map_err(|error| format!("{} cannot generate images here: {error}", client.name()))?;
        let call = reply["output"].as_array().into_iter().flatten()
            .find(|item| item["type"] == "image_generation_call" && item["result"].as_str().is_some_and(|r| !r.is_empty()))
            .ok_or_else(|| format!("{} returned no image: the provider does not run the built-in image tool for this model; set an image model (modelRoles.image) instead", client.name()))?;
        let bytes = STANDARD.decode(call["result"].as_str().unwrap_or_default().trim()).map_err(|e| format!("generated image: bad base64: {e}"))?;
        pictures.push((bytes, call["revised_prompt"].as_str().map(str::to_owned)));
        if let Some(u) = usage(&reply["usage"]) {
            let sum = total.get_or_insert_default();
            sum.input += u.input;
            sum.output += u.output;
            sum.cache_read += u.cache_read;
        }
    }
    Ok((pictures, total))
}

/// The tool result: one `saved:` line per image (interfaces parse these), then notes.
pub fn report(model: &str, generated: &Generated) -> String {
    let mut lines: Vec<String> = generated.saved.iter().map(|path| format!("saved: {path}")).collect();
    lines.push(format!("model: {model}"));
    lines.extend(generated.notes.iter().cloned());
    lines.join("\n")
}

fn default_name(stamp: &str, index: usize, count: usize, bytes: &[u8]) -> String {
    let suffix = if count > 1 { format!("-{}", index + 1) } else { String::new() };
    format!("{DIR}/{stamp}{suffix}.{}", extension(bytes))
}

/// Writes `bytes` to `relative` inside `cwd`, creating folders, never outside the
/// workspace and never over an existing file (a counter is appended instead).
fn save(cwd: &Path, relative: &str, bytes: &[u8]) -> Result<String, String> {
    let relative = Path::new(relative.trim_start_matches('/'));
    if relative.as_os_str().is_empty() || relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("{}: not a path inside the workspace", relative.display()));
    }
    let root = cwd.canonicalize().map_err(|e| format!("{}: {e}", cwd.display()))?;
    let target = root.join(relative);
    let parent = target.parent().ok_or("invalid path")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    if !parent.canonicalize().map_err(|e| e.to_string())?.starts_with(&root) {
        return Err(format!("{}: escapes the workspace", relative.display()));
    }
    let target = free_name(target);
    std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(target.strip_prefix(&root).unwrap_or(&target).to_string_lossy().replace('\\', "/"))
}

fn free_name(path: PathBuf) -> PathBuf {
    if !path.exists() { return path; }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| path.with_file_name(format!("{stem}-{n}{ext}"))).find(|p| !p.exists()).expect("a free name")
}

fn extension(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [b'G', b'I', b'F', b'8', ..] => "gif",
        _ => "png",
    }
}

fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    crate::session::timestamp(secs)
}

/// Content type for an image path the `/raw` route may serve; `None` for anything else.
pub fn content_type(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_inside_the_workspace_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("ashira-image-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
        assert_eq!(save(&dir, "assets/logo.png", &png).unwrap(), "assets/logo.png");
        assert_eq!(save(&dir, "assets/logo.png", &png).unwrap(), "assets/logo-2.png");
        assert!(save(&dir, "../outside.png", &png).is_err());
        assert!(save(&dir, "a/../../outside.png", &png).is_err());
        assert_eq!(extension(&[0xFF, 0xD8, 0xFF, 0]), "jpg");
        assert_eq!(content_type("x/Photo.JPEG"), Some("image/jpeg"));
        assert_eq!(content_type("notes.txt"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
