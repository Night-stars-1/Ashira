use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
// Nerd Font file-text glyph; shared by the card title and input token.
const FILE_ICON: &str = "\u{f15c}";
pub(crate) const IMAGE_ICON: &str = "\u{f03e}";

#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub id: u64,
    pub label: String,
    pub data: crate::clipboard::ImageData,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    pub id: u64,
    pub text: String,
    pub lines: usize,
    pub characters: usize,
    pub label: String,
    /// Bounded, sanitized leading lines; never rescan a large paste while drawing.
    pub preview: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    PastedBackslash,
    Char(char),
    Block(Arc<Block>),
    Image(Arc<Image>),
    Pending(u64),
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Composer(pub Vec<Atom>);

impl Composer {
    pub fn pending(&mut self, cursor: &mut usize) -> u64 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.0.insert(*cursor, Atom::Pending(id));
        *cursor += 1;
        id
    }

    pub fn resolve(&mut self, id: u64, contents: Option<crate::clipboard::Contents>, cursor: &mut usize) -> bool {
        let Some(index) = self.0.iter().position(|atom| *atom == Atom::Pending(id)) else { return false };
        self.0.remove(index);
        let after = *cursor > index;
        if after { *cursor -= 1; }
        let mut end = index;
        match contents {
            Some(crate::clipboard::Contents::Text(text)) => self.paste(&mut end, text),
            Some(crate::clipboard::Contents::Image(data)) => {
                self.0.insert(index, Atom::Image(Arc::new(Image { id, label: format!("{IMAGE_ICON} #{id}"), data })));
                end += 1;
            }
            None => {}
        }
        if after { *cursor += end - index; }
        true
    }

    pub fn has_pending(&self) -> bool { self.0.iter().any(|a| matches!(a, Atom::Pending(_))) }

    pub fn message(&self) -> serde_json::Value {
        assert!(!self.has_pending());
        let mut content = Vec::new();
        let mut text = String::new();
        for atom in &self.0 {
            match atom {
                Atom::Char(c) => text.push(*c),
                Atom::PastedBackslash => text.push('\\'),
                Atom::Block(block) => text.push_str(&block.text),
                Atom::Image(image) => {
                    if !text.is_empty() { content.push(serde_json::json!({"type":"text", "text":std::mem::take(&mut text)})); }
                    content.push(serde_json::json!({"type":"image", "source":{"type":"base64", "media_type":"image/png", "data":image.data.data}}));
                }
                Atom::Pending(_) => unreachable!(),
            }
        }
        if !text.is_empty() { content.push(serde_json::json!({"type":"text", "text":text})); }
        serde_json::json!({"role":"user", "content":content})
    }

    pub fn paste(&mut self, cursor: &mut usize, text: String) {
        let text = if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text };
        let (lines, characters) = text.chars().fold((1, 0), |(lines, count), c| (lines + usize::from(c == '\n'), count + 1));
        if lines > 4 || characters > 400 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let preview = text.split('\n').take(4).map(|line| {
                let mut chars = line.chars();
                let prefix: String = chars.by_ref().take(128).collect();
                let mut clean = crate::render::sanitize(&prefix);
                if chars.next().is_some() { clean.push('…'); }
                clean
            }).collect();
            self.0.insert(*cursor, Atom::Block(Arc::new(Block { id, text, lines, characters, label: format!("{FILE_ICON} #{id}"), preview })));
            *cursor += 1;
        } else {
            let chars: Vec<_> = text.chars().map(|c| if c == '\\' { Atom::PastedBackslash } else { Atom::Char(c) }).collect();
            let n = chars.len();
            self.0.splice(*cursor..*cursor, chars);
            *cursor += n;
        }
    }

    pub fn expand(&mut self, cursor: &mut usize) -> bool {
        let index = if matches!(self.0.get(*cursor), Some(Atom::Block(_))) {
            Some(*cursor)
        } else if *cursor > 0 && matches!(self.0.get(*cursor - 1), Some(Atom::Block(_))) {
            Some(*cursor - 1)
        } else {
            None
        };
        if let Some(i) = index
            && let Atom::Block(block) = self.0.remove(i)
        {
            let chars: Vec<_> = block.text.chars().map(|c| if c == '\\' { Atom::PastedBackslash } else { Atom::Char(c) }).collect();
            let n = chars.len();
            self.0.splice(i..i, chars);
            *cursor = i + n;
            return true;
        }
        false
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for atom in &self.0 {
            match atom {
                Atom::Char(c) => out.push(*c),
                Atom::PastedBackslash => out.push('\\'),
                Atom::Block(b) => out.push_str(&b.text),
                Atom::Image(_) | Atom::Pending(_) => {},
            }
        }
        out
    }

    pub fn display(&self) -> String {
        let mut out = String::new();
        for (i, atom) in self.0.iter().enumerate() {
            let label = match atom {
                Atom::Char(c) => { out.push(*c); continue; }
                Atom::PastedBackslash => { out.push('\\'); continue; }
                Atom::Block(b) => b.label.as_str(),
                Atom::Image(image) => image.label.as_str(),
                Atom::Pending(_) => "粘贴中…",
            };
            out.push_str(label);
            // Same gap as the input box, unless whitespace already follows.
            if !matches!(self.0.get(i + 1), Some(Atom::Char(c)) if c.is_whitespace()) { out.push(' '); }
        }
        out
    }

    pub fn has_attachment(&self) -> bool {
        self.0.iter().any(|a| matches!(a, Atom::Block(_) | Atom::Image(_) | Atom::Pending(_)))
    }

    pub fn is_blank(&self) -> bool {
        self.0.iter().all(|atom| match atom {
            Atom::Char(c) => c.is_whitespace(),
            Atom::PastedBackslash => false,
            Atom::Block(block) => block.text.trim().is_empty(),
            Atom::Image(_) | Atom::Pending(_) => false,
        })
    }

    pub fn command(&self) -> Option<String> {
        (self.0.first() == Some(&Atom::Char('/')) && !self.has_attachment() && !self.0.iter().any(|atom| matches!(atom, Atom::Char('\n' | '\r')))).then(|| self.text())
    }
}

impl From<String> for Composer {
    fn from(text: String) -> Self {
        Self(text.chars().map(Atom::Char).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_paste_draft_flow() {
        let mut draft = Composer::default();
        let mut cursor = 0;
        let paste = "中文\t一\n\n二\n三\n四\n".to_string();
        draft.paste(&mut cursor, paste.clone());
        let marker = draft.display();
        draft.0.extend(" suffix".chars().map(Atom::Char));
        let history = draft.clone();
        assert_eq!(draft.text(), format!("{paste} suffix"));
        draft.expand(&mut cursor);
        draft.0.insert(cursor, Atom::Char('!'));
        assert_eq!(draft.text(), format!("{paste}! suffix"));
        assert_eq!(history.text(), format!("{paste} suffix"));
        let mut recalled = history.clone();
        recalled.0.remove(0);
        assert_eq!(recalled.text(), " suffix");
        let literal = Composer::from(marker.clone());
        assert_eq!(literal.text(), marker);
        assert!(!literal.has_attachment());
    }
}
