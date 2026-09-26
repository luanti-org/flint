//! parsers for luanti stuff

pub mod depends_txt;
pub mod description_txt;
pub mod mod_conf;
pub mod settings;

fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n")
}
