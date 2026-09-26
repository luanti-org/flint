//! `description.txt`: mod description before mod.conf

pub const FILE_NAME: &str = "description.txt";

pub fn parse(text: &str) -> String {
    super::normalize(text).trim().to_string()
}
