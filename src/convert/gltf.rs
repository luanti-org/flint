//! glTF, which models are read into and written from

use gltf_json::Root;

/// a glTF document and its one buffer, like a `.glb`
pub struct Gltf {
    pub root: Root,
    pub buffer: Vec<u8>,
}

impl Gltf {
    pub fn is_animated(&self) -> bool {
        !self.root.skins.is_empty() || self.root.animations.iter().any(|a| !a.channels.is_empty())
    }

    /// `.gltf`, with the buffer embedded as a data URI
    pub fn to_gltf(&self) -> Vec<u8> {
        let mut root = self.root.clone();
        if let Some(buffer) = root.buffers.first_mut() {
            buffer.uri = Some(format!("data:application/octet-stream;base64,{}", base64(&self.buffer)));
        }
        serde_json::to_vec(&root).expect("glTF is valid JSON")
    }

    /// binary `.glb`
    pub fn to_glb(&self) -> Vec<u8> {
        let mut json = serde_json::to_vec(&self.root).expect("glTF is valid JSON");
        json.resize(json.len().next_multiple_of(4), b' ');
        let mut bin = self.buffer.clone();
        bin.resize(bin.len().next_multiple_of(4), 0);

        let mut chunks = Vec::new();
        for (kind, data) in [(b"JSON", json), (b"BIN\0", bin)] {
            if !data.is_empty() {
                chunks.extend((data.len() as u32).to_le_bytes());
                chunks.extend(kind);
                chunks.extend(data);
            }
        }
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend((12 + chunks.len() as u32).to_le_bytes());
        glb.extend(chunks);
        glb
    }
}

pub(super) fn base64(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            // Blender requires padding
            out.push(if i <= chunk.len() { CHARS[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}
