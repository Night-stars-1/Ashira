//! Settings → 模型: read and edit `models.yml` providers and the `config.yml`
//! model roles. Only the fields the form owns are changed; everything else on a
//! provider (`modelOverrides`, `headers`, `compat`, …) is kept as written. The
//! file is re-serialized, so comments are not preserved.
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{config, session};

const APIS: [&str; 3] = ["anthropic-messages", "openai-completions", "openai-responses"];

/// A YAML mapping with key order kept (`serde_json::Map` would sort it).
type Mapping = IndexMap<String, Value>;

/// `models.yml`: providers in file order, unknown root keys kept.
#[derive(Deserialize, Serialize, Default)]
struct ModelsFile {
    #[serde(default)]
    providers: IndexMap<String, Mapping>,
    #[serde(flatten)]
    rest: Mapping,
}

fn models_path() -> PathBuf {
    let home = session::home();
    let yaml = home.join("models.yaml");
    if !home.join("models.yml").is_file() && yaml.is_file() { yaml } else { home.join("models.yml") }
}

fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Ok(_) => Ok(T::default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let yaml = serde_saphyr::to_string(value).map_err(|e| format!("无法生成 {}：{e}", path.display()))?;
    config::write_settings_atomic(path, yaml.as_bytes())
}

/// What the page sees. A literal key is never sent back, only that one is set.
pub fn read() -> Result<Value, String> {
    read_at(&models_path(), &config::settings_path())
}

fn read_at(models: &Path, settings: &Path) -> Result<Value, String> {
    let file: ModelsFile = load(models)?;
    let rows: Vec<Value> = file.providers.iter().map(|(id, p)| {
        let text = |key: &str| p.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let models: Vec<&str> = p.get("models").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|m| m.get("id").and_then(Value::as_str)).collect();
        json!({
            "id": id,
            "api": text("api"),
            "baseUrl": text("baseUrl"),
            "key": key_view(p),
            "discovery": p.get("discovery").and_then(|d| d.get("type")).and_then(Value::as_str).is_some(),
            "models": models,
        })
    }).collect();
    let settings: Mapping = load(settings)?;
    let role = |name: &str| settings.get("modelRoles").and_then(|r| r.get(name)).and_then(Value::as_str).unwrap_or("").to_owned();
    let roles: serde_json::Map<String, Value> = ROLES.iter().map(|&name| (name.to_owned(), json!(role(name)))).collect();
    Ok(json!({"apis": APIS, "providers": rows, "roles": roles}))
}

/// `{kind, value}`: an env var name, a `!command`, a stored literal (value hidden), none, or keyless.
fn key_view(p: &Mapping) -> Value {
    if p.get("auth").and_then(Value::as_str) == Some("none") { return json!({"kind": "none"}); }
    let Some(raw) = p.get("apiKey").and_then(Value::as_str).filter(|k| !k.is_empty()) else { return json!({"kind": "missing"}) };
    if let Some(command) = raw.strip_prefix('!') { return json!({"kind": "command", "value": command.trim()}); }
    // Same rule as the loader: a set env var wins. Only an UPPER_CASE name is
    // shown when unset, so a literal key that happens to be alphanumeric stays hidden.
    if is_env_name(raw) && std::env::var(raw).is_ok_and(|v| !v.is_empty()) { return json!({"kind": "env", "value": raw}); }
    if is_env_name(raw) && raw.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') { return json!({"kind": "env", "value": raw, "unset": true}); }
    json!({"kind": "literal"})
}

fn is_env_name(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !text.starts_with(|c: char| c.is_ascii_digit())
}

/// Create (`previous` empty) or replace one provider.
pub fn save_provider(previous: &str, form: &Value) -> Result<(), String> {
    save_provider_at(&models_path(), &config::settings_path(), previous, form)
}

/// The provider as the form would save it, with discovery on, for listing its
/// models before saving. An empty key field uses the stored key.
pub fn probe_target(previous: &str, form: &Value) -> Result<Value, String> {
    let file: ModelsFile = load(&models_path())?;
    let mut provider = connection(&file, previous, form)?;
    if provider.get("discovery").and_then(|d| d.get("type")).is_none() {
        provider.insert("discovery".into(), json!({"type": "openai-models-list"}));
    }
    Ok(Value::Object(provider.into_iter().collect()))
}

/// Stored provider (or a new one) with the form's api, Base URL and key applied.
fn connection(file: &ModelsFile, previous: &str, form: &Value) -> Result<Mapping, String> {
    let api = form["api"].as_str().unwrap_or("");
    if config::Api::parse(api).is_none() { return Err(format!("不支持的接口类型：{api}")); }
    let base_url = form["baseUrl"].as_str().map(str::trim).unwrap_or("");
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) { return Err("Base URL 需要以 http:// 或 https:// 开头".into()); }
    // Start from the stored provider so fields the form doesn't own survive.
    let mut provider = if previous.is_empty() { Mapping::new() } else {
        file.providers.get(previous).cloned().ok_or_else(|| format!("找不到服务商 {previous}"))?
    };
    provider.insert("api".into(), json!(api));
    provider.insert("baseUrl".into(), json!(base_url.trim_end_matches('/')));
    match form["key"]["kind"].as_str() {
        Some("none") => { provider.shift_remove("apiKey"); provider.insert("auth".into(), json!("none")); }
        Some(kind @ ("env" | "literal" | "command")) => {
            provider.shift_remove("auth");
            let value = form["key"]["value"].as_str().map(str::trim).unwrap_or("");
            if value.is_empty() {
                // Empty means "keep the stored key", which must exist.
                if provider.get("apiKey").and_then(Value::as_str).is_none() { return Err("请填写 API Key".into()); }
            } else {
                let stored = match kind {
                    "command" => format!("!{value}"),
                    "env" if !is_env_name(value) => return Err("环境变量名只能包含字母、数字和 _".into()),
                    _ => value.to_owned(),
                };
                provider.insert("apiKey".into(), json!(stored));
            }
        }
        _ => return Err("请选择 API Key 的方式".into()),
    }
    Ok(provider)
}

fn save_provider_at(models: &Path, settings: &Path, previous: &str, form: &Value) -> Result<(), String> {
    let id = form["id"].as_str().map(str::trim).unwrap_or("");
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("服务商 ID 只能包含字母、数字、-、_ 和 .".into());
    }
    let models_list: Vec<String> = form["models"].as_array().into_iter().flatten()
        .filter_map(|m| m.as_str().map(str::trim)).filter(|m| !m.is_empty()).map(str::to_owned).collect();
    let discovery = form["discovery"].as_bool().unwrap_or(false);
    if models_list.is_empty() && !discovery { return Err("至少填一个模型 ID，或开启自动获取模型列表".into()); }

    let mut file: ModelsFile = load(models)?;
    if id != previous && file.providers.contains_key(id) { return Err(format!("服务商 {id} 已存在")); }
    let mut provider = connection(&file, previous, form)?;
    if discovery {
        let kept = provider.get("discovery").filter(|d| d.get("type").is_some()).cloned();
        provider.insert("discovery".into(), kept.unwrap_or_else(|| json!({"type": "openai-models-list"})));
    } else {
        provider.shift_remove("discovery");
    }
    // Rebuild `models`, keeping each listed model's existing extra fields.
    let old: Vec<Value> = provider.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
    let rebuilt: Vec<Value> = models_list.iter().map(|m| old.iter().find(|o| o["id"] == m.as_str()).cloned().unwrap_or_else(|| json!({"id": m}))).collect();
    if rebuilt.is_empty() { provider.shift_remove("models"); } else { provider.insert("models".into(), Value::Array(rebuilt)); }

    // An edit or rename keeps the provider's place in the file; a new one goes last.
    match file.providers.get_index_of(previous).filter(|_| !previous.is_empty()) {
        Some(index) => {
            file.providers.shift_remove_index(index);
            file.providers.shift_insert(index, id.to_owned(), provider);
        }
        None => { file.providers.insert(id.to_owned(), provider); }
    }
    write(models, &file)?;
    if !previous.is_empty() && previous != id { rename_roles(settings, previous, id)?; }
    Ok(())
}

pub fn remove_provider(id: &str) -> Result<(), String> {
    remove_provider_at(&models_path(), id)
}

fn remove_provider_at(models: &Path, id: &str) -> Result<(), String> {
    let mut file: ModelsFile = load(models)?;
    if file.providers.shift_remove(id).is_none() { return Err(format!("找不到服务商 {id}")); }
    write(models, &file)
}

/// Roles the settings form edits: default model, subagents, the background jobs, images.
pub const ROLES: [&str; 6] = ["default", "task", "compact", "title", "memory", "image"];

/// Sets each role in `roles` (`name -> selector`); an empty selector clears it (default
/// falls back to the first usable model, the others to the session's model). Roles
/// missing from `roles` and any other keys are left as they are.
pub fn save_roles(roles: &Value) -> Result<(), String> {
    save_roles_at(&config::settings_path(), roles)
}

fn save_roles_at(settings: &Path, given: &Value) -> Result<(), String> {
    let mut value: Mapping = load(settings)?;
    let roles = value.entry("modelRoles".into()).or_insert_with(|| json!({}));
    let roles = roles.as_object_mut().ok_or("modelRoles 必须是映射")?;
    for name in ROLES {
        let Some(selector) = given.get(name) else { continue };
        let selector = selector.as_str().ok_or_else(|| format!("{name} 必须是字符串"))?.trim();
        if selector.is_empty() { roles.remove(name); } else { roles.insert(name.into(), json!(selector)); }
    }
    write(settings, &value)
}

/// A renamed provider keeps working as the default/task model.
fn rename_roles(settings: &Path, from: &str, to: &str) -> Result<(), String> {
    let mut value: Mapping = load(settings)?;
    let Some(roles) = value.get_mut("modelRoles").and_then(Value::as_object_mut) else { return Ok(()) };
    let mut changed = false;
    for selector in roles.values_mut() {
        if let Some(rest) = selector.as_str().and_then(|s| s.strip_prefix(&format!("{from}/"))) {
            *selector = json!(format!("{to}/{rest}"));
            changed = true;
        }
    }
    if changed { write(settings, &value) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_edits_keep_unowned_fields_order_and_hide_keys() {
        let dir = std::env::temp_dir().join(format!("ashira-models-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let (models, settings) = (dir.join("models.yml"), dir.join("config.yml"));
        std::fs::write(&models, "providers:\n  zeta:\n    api: openai-responses\n    baseUrl: https://a.example/v1\n    apiKey: sk-secret-literal\n    authHeader: true\n    discovery:\n      type: openai-models-list\n    modelOverrides:\n      gpt-x:\n        cost: {input: 1, output: 2}\n    models:\n      - id: gpt-x\n        contextWindow: 9000\n  alpha:\n    api: anthropic-messages\n    baseUrl: https://b.example\n    apiKey: '!get-key'\n    models:\n      - id: claude-y\n").unwrap();
        std::fs::write(&settings, "modelRoles:\n  default: zeta/gpt-x\n  task: alpha/claude-y\nmemory:\n  auto: true\n").unwrap();
        let ids = |path: &Path| load::<ModelsFile>(path).unwrap().providers.keys().cloned().collect::<Vec<_>>();

        let view = read_at(&models, &settings).unwrap();
        assert_eq!(view["providers"][0]["key"], json!({"kind": "literal"}), "literal keys never leave the server");
        assert!(!view.to_string().contains("sk-secret-literal"));
        assert_eq!(view["providers"][1]["key"], json!({"kind": "command", "value": "get-key"}));
        assert_eq!(view["roles"]["default"].as_str(), Some("zeta/gpt-x"));
        assert_eq!(view["roles"]["title"].as_str(), Some(""), "every role is listed, unset ones empty");

        // Rename + edit without retyping the key: key, overrides, per-model fields and position survive.
        let form = json!({"id": "renamed", "api": "openai-completions", "baseUrl": "https://a2.example/v1/", "key": {"kind": "literal", "value": ""}, "discovery": false, "models": ["gpt-x", "gpt-new"]});
        save_provider_at(&models, &settings, "zeta", &form).unwrap();
        assert_eq!(ids(&models), ["renamed", "alpha"], "a rename stays in place instead of re-sorting");
        let file: ModelsFile = load(&models).unwrap();
        let p = &file.providers["renamed"];
        assert_eq!((p["apiKey"].as_str(), p["authHeader"].as_bool(), p["baseUrl"].as_str()), (Some("sk-secret-literal"), Some(true), Some("https://a2.example/v1")));
        assert_eq!(p["modelOverrides"]["gpt-x"]["cost"]["output"], 2);
        assert_eq!(p["models"], json!([{"id": "gpt-x", "contextWindow": 9000}, {"id": "gpt-new"}]));
        assert!(p.get("discovery").is_none());
        let roles: Mapping = load(&settings).unwrap();
        assert_eq!((roles["modelRoles"]["default"].as_str(), roles["memory"]["auto"].as_bool()), (Some("renamed/gpt-x"), Some(true)));

        // Validation, a new provider, roles and removal.
        let bad = json!({"id": "x y", "api": "openai-responses", "baseUrl": "https://c", "key": {"kind": "env", "value": "K"}, "discovery": true, "models": []});
        assert!(save_provider_at(&models, &settings, "", &bad).is_err());
        let dup = json!({"id": "alpha", "api": "openai-responses", "baseUrl": "https://c", "key": {"kind": "env", "value": "K"}, "discovery": true, "models": []});
        assert!(save_provider_at(&models, &settings, "", &dup).unwrap_err().contains("已存在"));
        let fresh = json!({"id": "third", "api": "openai-responses", "baseUrl": "https://c.example/v1", "key": {"kind": "env", "value": "THIRD_KEY"}, "discovery": true, "models": []});
        save_provider_at(&models, &settings, "", &fresh).unwrap();
        assert_eq!(ids(&models), ["renamed", "alpha", "third"]);
        assert_eq!(load::<ModelsFile>(&models).unwrap().providers["third"]["discovery"]["type"], "openai-models-list");
        save_roles_at(&settings, &json!({"default": "third/m", "task": "", "title": "third/small"})).unwrap();
        let roles: Mapping = load(&settings).unwrap();
        assert_eq!((roles["modelRoles"]["default"].as_str(), roles["modelRoles"].get("task"), roles["modelRoles"]["title"].as_str()), (Some("third/m"), None, Some("third/small")));
        // Roles left out of a save keep their value.
        save_roles_at(&settings, &json!({"default": "third/m"})).unwrap();
        assert_eq!(load::<Mapping>(&settings).unwrap()["modelRoles"]["title"].as_str(), Some("third/small"));
        remove_provider_at(&models, "alpha").unwrap();
        assert_eq!(ids(&models), ["renamed", "third"]);
        // The registry still loads what the form wrote.
        assert!(config::read_settings(&models).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
