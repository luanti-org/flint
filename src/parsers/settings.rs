//! Settings format, used by multiple files

use std::collections::HashMap;

pub struct Settings {
    entries: HashMap<String, String>,
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let text = super::normalize(text);
        let mut entries = HashMap::new();
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let mut value = value.trim().to_string();
            if value == "\"\"\"" {
                value = lines
                    .by_ref()
                    .take_while(|l| l.trim() != "\"\"\"")
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            entries.insert(key.trim().to_string(), value);
        }
        Settings { entries }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

pub fn format_entry(key: &str, value: &str) -> String {
    if value.contains('\n') {
        format!("{key} = \"\"\"\n{value}\n\"\"\"\n")
    } else {
        format!("{key} = {value}\n")
    }
}
