//! `ask_user` question groups: the model's input, the user's answers, and the
//! text the model gets back. Shared by the agent, the TUI and the HTTP API.
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Single, Multi, Text }

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self { Kind::Single => "single", Kind::Multi => "multi", Kind::Text => "text" }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The option's title; answers refer to options by it.
    pub label: String,
    /// Optional explanation shown under the title.
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub kind: Kind,
    pub options: Vec<Choice>,
    /// Text: the prefilled answer. Single/multi: labels of preselected options.
    pub default: Vec<String>,
}

impl Question {
    fn has_option(&self, label: &str) -> bool {
        self.options.iter().any(|o| o.label == label)
    }
}

/// One entry per question, in order; empty means unanswered. Choice answers
/// may include text the user typed instead of an option.
pub type Answers = Vec<Vec<String>>;

/// Parses `questions`, or the single-question form `question` + `options`.
pub fn parse(input: &Value) -> Result<Vec<Question>, String> {
    let items = match &input["questions"] {
        Value::Array(items) => items.as_slice(),
        Value::Null if !input["question"].is_null() => std::slice::from_ref(input),
        _ => return Err("ask_user requires `questions`, a nonempty array of questions".into()),
    };
    if items.is_empty() {
        return Err("ask_user requires at least one question".into());
    }
    let mut questions: Vec<Question> = Vec::with_capacity(items.len());
    for (index, value) in items.iter().enumerate() {
        let question = item(index + 1, value)?;
        if questions.iter().any(|q| q.id == question.id) {
            return Err(format!("question ids must be unique; {:?} repeats", question.id));
        }
        questions.push(question);
    }
    Ok(questions)
}

fn item(n: usize, value: &Value) -> Result<Question, String> {
    let text = value["question"].as_str().map(str::trim).filter(|s| !s.is_empty())
        .ok_or_else(|| format!("question {n}: `question` must be a nonempty string"))?;
    let options = choices(&value["options"]).map_err(|e| format!("question {n}: {e}"))?;
    if let Some(repeat) = options.iter().enumerate().find_map(|(i, o)| options[..i].iter().any(|p| p.label == o.label).then_some(&o.label)) {
        return Err(format!("question {n}: option {repeat:?} repeats"));
    }
    let kind = match value["type"].as_str() {
        None if options.is_empty() => Kind::Text,
        None | Some("single") => Kind::Single,
        Some("multi") => Kind::Multi,
        Some("text") => Kind::Text,
        Some(other) => return Err(format!("question {n}: unknown type {other:?}; use single, multi or text")),
    };
    match kind {
        Kind::Text if !options.is_empty() => return Err(format!("question {n}: text questions take no options")),
        Kind::Single | Kind::Multi if options.is_empty() => return Err(format!("question {n}: {} questions need options", kind.as_str())),
        _ => {}
    }
    // Models often wrap a single default in an array; one element is unambiguous.
    let default = match (&value["default"], kind) {
        (Value::Null, _) => Vec::new(),
        (Value::String(s), Kind::Text) => if s.trim().is_empty() { Vec::new() } else { vec![s.clone()] },
        (Value::Array(items), Kind::Text) if items.len() == 1 && items[0].is_string() => {
            let s = items[0].as_str().unwrap_or_default();
            if s.trim().is_empty() { Vec::new() } else { vec![s.to_owned()] }
        }
        (Value::String(_), _) | (Value::Array(_), Kind::Multi) => strings(&value["default"])
            .ok_or_else(|| format!("question {n}: `default` must be nonempty strings"))?,
        (Value::Array(items), Kind::Single) if items.len() == 1 => strings(&value["default"])
            .ok_or_else(|| format!("question {n}: `default` must be a nonempty string"))?,
        _ => return Err(format!("question {n}: `default` must be a string{}", if kind == Kind::Multi { " or an array of options" } else { "" })),
    };
    if kind != Kind::Text && let Some(bad) = default.iter().find(|d| !options.iter().any(|o| &o.label == *d)) {
        return Err(format!("question {n}: default {bad:?} is not one of the options"));
    }
    let id = value["id"].as_str().map(str::trim).filter(|s| !s.is_empty()).map_or_else(|| n.to_string(), str::to_owned);
    Ok(Question { id, text: text.to_owned(), kind, options, default })
}

/// Options are strings or `{"label", "description"}` objects.
fn choices(value: &Value) -> Result<Vec<Choice>, String> {
    let items = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        _ => return Err("`options` must be an array".into()),
    };
    items.iter().enumerate().map(|(i, item)| {
        let bad = || format!("option {} must be a nonempty string or {{\"label\", \"description\"}}", i + 1);
        let nonempty = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        match item {
            Value::String(_) => nonempty(item).map(|label| Choice { label, description: None }).ok_or_else(bad),
            Value::Object(map) => {
                let label = map.get("label").and_then(nonempty).ok_or_else(bad)?;
                let description = match map.get("description") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
                    Some(_) => return Err(format!("option {}: `description` must be a string", i + 1)),
                };
                Ok(Choice { label, description })
            }
            _ => Err(bad()),
        }
    }).collect()
}

/// A string or an array of strings, trimmed; `null` is empty. `None` on any
/// other shape or an empty entry.
fn strings(value: &Value) -> Option<Vec<String>> {
    let one = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    match value {
        Value::Null => Some(Vec::new()),
        Value::String(_) => one(value).map(|s| vec![s]),
        Value::Array(items) => items.iter().map(one).collect(),
        _ => None,
    }
}

/// The event payload frontends render.
pub fn to_json(questions: &[Question]) -> Value {
    Value::Array(questions.iter().map(|q| json!({
        "id": q.id, "question": q.text, "type": q.kind.as_str(),
        "options": q.options.iter().map(|o| json!({"label": o.label, "description": o.description})).collect::<Vec<_>>(),
        "default": if q.kind == Kind::Multi { json!(q.default) } else { json!(q.default.first()) },
    })).collect())
}

/// Validates `{"<id>": "text" | ["choice", ...] | null}` from a frontend.
pub fn parse_answers(questions: &[Question], value: &Value) -> Result<Answers, String> {
    let map = value.as_object().ok_or("answers must be an object keyed by question id")?;
    if let Some(extra) = map.keys().find(|key| !questions.iter().any(|q| &q.id == *key)) {
        return Err(format!("answers has no question with id {extra:?}"));
    }
    questions.iter().map(|q| {
        let value = map.get(&q.id).unwrap_or(&Value::Null);
        if q.kind != Kind::Multi && value.is_array() {
            return Err(format!("the answer to {:?} must be a string", q.id));
        }
        let mut answer = strings(value).ok_or_else(|| format!("the answer to {:?} must be text or a list of choices", q.id))?;
        if q.kind == Kind::Text && let (Value::String(raw), Some(first)) = (value, answer.first_mut()) {
            // Keep the user's line breaks and indentation; only trim the ends.
            *first = raw.trim().to_owned();
        }
        let mut seen = std::collections::HashSet::new();
        answer.retain(|a| seen.insert(a.clone()));
        Ok(answer)
    }).collect()
}

/// The tool result. `None` means the user skipped the whole group.
pub fn result_text(questions: &[Question], answers: Option<&Answers>) -> String {
    let Some(answers) = answers.filter(|a| a.iter().any(|answer| !answer.is_empty())) else {
        return "The user skipped the question(s). Proceed with your best judgment and state the assumptions you made.".into();
    };
    let mut out = String::from("The user answered:");
    for (index, (q, answer)) in questions.iter().zip(answers).enumerate() {
        out.push_str(&format!("\n{}. {}\n   → ", index + 1, q.text));
        if answer.is_empty() {
            out.push_str("(no answer; use your judgment and state the assumption)");
            continue;
        }
        let shown: Vec<String> = answer.iter().map(|a| {
            if q.kind != Kind::Text && !q.has_option(a) { format!("{a} (typed by the user instead of an option)") } else { a.clone() }
        }).collect();
        out.push_str(&shown.join("; "));
    }
    out
}

/// One line per answered question for transcripts.
pub fn answer_lines(questions: &[Question], answers: &Answers) -> Vec<String> {
    questions.iter().zip(answers).map(|(q, a)| {
        format!("{} → {}", q.text, if a.is_empty() { "未回答".to_owned() } else { a.join("、") })
    }).collect()
}

/// Tool-card title: the first question, plus the count when there are more.
pub fn summary(input: &Value) -> String {
    match input["questions"].as_array() {
        Some(items) => {
            let first = items.first().and_then(|q| q["question"].as_str()).unwrap_or("");
            if items.len() > 1 { format!("{first}（共 {} 题）", items.len()) } else { first.to_owned() }
        }
        None => input["question"].as_str().unwrap_or("").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_input_to_user_answers_to_model_text() {
        let questions = parse(&json!({"questions": [
            {"id": "fmt", "question": "配置格式？", "options": ["JSON", "YAML"], "default": "YAML"},
            {"question": "启用哪些功能？", "type": "multi", "options": ["缓存", "日志", "指标"], "default": ["日志"]},
            {"id": "name", "question": "文件名？", "default": "app.yaml"},
        ]})).unwrap();
        assert_eq!(questions.iter().map(|q| (q.id.as_str(), q.kind)).collect::<Vec<_>>(), [("fmt", Kind::Single), ("2", Kind::Multi), ("name", Kind::Text)]);
        let event = to_json(&questions);
        assert_eq!((&event[0]["default"], &event[1]["default"], &event[2]["default"]), (&json!("YAML"), &json!(["日志"]), &json!("app.yaml")));

        let answers = parse_answers(&questions, &json!({"fmt": "TOML", "2": ["缓存", "日志", "缓存"], "name": null})).unwrap();
        assert_eq!(answers, vec![vec!["TOML".to_owned()], vec!["缓存".to_owned(), "日志".to_owned()], vec![]]);
        let text = result_text(&questions, Some(&answers));
        assert!(text.contains("配置格式？\n   → TOML (typed by the user instead of an option)"), "{text}");
        assert!(text.contains("启用哪些功能？\n   → 缓存; 日志"), "{text}");
        assert!(text.contains("文件名？\n   → (no answer"), "{text}");
        assert!(result_text(&questions, Some(&vec![vec![], vec![], vec![]])).starts_with("The user skipped"));

        assert!(parse_answers(&questions, &json!({"other": "x"})).unwrap_err().contains("\"other\""));
        assert!(parse_answers(&questions, &json!({"fmt": ["JSON"]})).is_err());
        // The single-question form still works.
        let legacy = parse(&json!({"question": "继续？", "options": ["是", "否"]})).unwrap();
        assert_eq!((legacy[0].kind, legacy[0].id.as_str()), (Kind::Single, "1"));
        // Exactly what a model sent: one-element arrays for single and text defaults.
        let wrapped = parse(&json!({"questions": [
            {"id": "f", "question": "框架？", "type": "single", "options": ["FastAPI", "Flask"], "default": ["FastAPI"]},
            {"id": "n", "question": "服务名？", "type": "text", "default": ["my-service"]},
        ]})).unwrap();
        assert_eq!((wrapped[0].default.as_slice(), wrapped[1].default.as_slice()), (&["FastAPI".to_owned()][..], &["my-service".to_owned()][..]));
        assert!(parse(&json!({"questions": [{"question": "a", "options": ["x", "y"], "default": ["x", "y"]}]})).is_err());
        // No cap on group size or option count.
        let many: Vec<Value> = (0..12).map(|i| json!({"question": format!("q{i}"), "options": (0..20).map(|o| format!("o{o}")).collect::<Vec<_>>()})).collect();
        assert_eq!(parse(&json!({"questions": many})).unwrap().len(), 12);
        // Options may carry a title and a note; answers use the title.
        let described = parse(&json!({"questions": [{"id": "db", "question": "数据库？", "type": "multi", "default": ["PostgreSQL"], "options": [
            {"label": "PostgreSQL", "description": "功能最全，推荐"},
            {"label": "SQLite", "description": "  "},
            "MySQL",
        ]}]})).unwrap();
        assert_eq!(described[0].options[0], Choice { label: "PostgreSQL".into(), description: Some("功能最全，推荐".into()) });
        assert_eq!((described[0].options[1].description.as_deref(), described[0].options[2].label.as_str()), (None, "MySQL"));
        assert_eq!(to_json(&described)[0]["options"][0], json!({"label": "PostgreSQL", "description": "功能最全，推荐"}));
        let picked = parse_answers(&described, &json!({"db": ["SQLite", "PostgreSQL"]})).unwrap();
        assert_eq!(result_text(&described, Some(&picked)).lines().last(), Some("   → SQLite; PostgreSQL"));
        for bad in [json!([{"description": "无标题"}]), json!([{"label": "x", "description": 1}]), json!([{"label": "x"}, "x"]), json!([3])] {
            assert!(parse(&json!({"questions": [{"question": "a", "options": bad}]})).is_err(), "{bad}");
        }
        for bad in [
            json!({"questions": []}),
            json!({"questions": [{"question": "a", "type": "multi"}]}),
            json!({"questions": [{"question": "a", "options": ["x"], "default": "y"}]}),
            json!({"questions": [{"id": "a", "question": "a"}, {"id": "a", "question": "b"}]}),
            json!({"questions": [{"question": "a", "type": "text", "options": ["x"]}]}),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }
}
