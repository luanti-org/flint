//! b3d to glTF, ported from [modlib](https://github.com/appgurueu/modlib/blob/master/b3d.lua)'s `to_gltf`
//!
//! unlike modlib, the result loads in Luanti like the b3d did, so mods using it only need to change the file name:
//! * Luanti reads b3d coordinates as they are, and mirrors glTF ones along Z, so that's the axis flipped here, not X
//! * Luanti uses glTF keyframe times as frame numbers, and b3d frame `n` as `n - 1`, so times are `frame - 1`, not seconds
//! * Luanti gives a mesh buffer the `textures` entry of its glTF texture index, and b3d triangle sets the entry of their
//!   position in the file, so each triangle set gets the texture of that index

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::parsers::b3d::{B3d, Brush, Mesh, Node};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const FLOAT: u32 = 5126;

/// Luanti stores vertex indices as 16 bit, and doesn't allow the max value
const MAX_VERTICES: usize = u16::MAX as usize - 1;

pub struct Gltf {
    json: Value,
    buffer: Vec<u8>,
}

impl Gltf {
    /// `.gltf`, with the buffer embedded as a data URI
    pub fn to_gltf(&self) -> Vec<u8> {
        let mut json = self.json.clone();
        if let Some(buffer) = json.pointer_mut("/buffers/0") {
            buffer["uri"] = format!("data:application/octet-stream;base64,{}", base64(&self.buffer)).into();
        }
        serde_json::to_vec(&json).expect("glTF is valid JSON")
    }

    /// binary `.glb`
    pub fn to_glb(&self) -> Vec<u8> {
        let mut json = serde_json::to_vec(&self.json).expect("glTF is valid JSON");
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

/// returns the glTF and warnings about what couldn't be converted
pub fn convert(b3d: &B3d) -> Result<(Gltf, Vec<String>), String> {
    let root = b3d.node.as_ref().ok_or("the model is empty")?;
    let mut c = Converter::new(b3d);
    let root = c.add_node(root, None, IDENTITY)?;
    if c.non_finite {
        return Err("the model has numbers that are infinite or NaN".to_string());
    }

    let mut gltf = Map::new();
    gltf.insert("asset".into(), json!({"version": "2.0", "generator": "flint convert"}));
    gltf.insert("scene".into(), 0.into());
    gltf.insert("scenes".into(), json!([{"nodes": [root]}]));
    gltf.insert("nodes".into(), c.nodes.into_iter().map(|n| n.expect("every node is filled in")).collect());
    let materials = c.materials.into_iter().map(|m| m.expect("every triangle set has a material")).collect();
    let animations = match c.channels.is_empty() {
        true => Vec::new(),
        // b3d only has a single animation
        false => vec![json!({"channels": c.channels, "samplers": c.samplers})],
    };
    for (key, list) in [
        ("meshes", c.meshes),
        ("materials", materials),
        ("textures", c.textures),
        ("skins", c.skins),
        ("animations", animations),
        ("accessors", c.accessors),
        ("bufferViews", c.views),
    ] {
        // glTF doesn't allow empty lists
        if !list.is_empty() {
            gltf.insert(key.into(), list.into());
        }
    }
    if !c.buffer.is_empty() {
        gltf.insert("buffers".into(), json!([{"byteLength": c.buffer.len()}]));
    }
    Ok((Gltf { json: gltf.into(), buffer: c.buffer }, c.warnings))
}

struct Converter<'a> {
    b3d: &'a B3d,
    warnings: Vec<String>,
    non_finite: bool,
    buffer: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    /// textures and materials are indexed by triangle set
    textures: Vec<Value>,
    materials: Vec<Option<Value>>,
    meshes: Vec<Value>,
    nodes: Vec<Option<Value>>,
    skins: Vec<Value>,
    samplers: Vec<Value>,
    channels: Vec<Value>,
}

/// a mesh's bones
#[derive(Default)]
struct Skin {
    vertices: usize,
    /// node ids
    joints: Vec<usize>,
    inverse_binds: Vec<Mat>,
    /// `weights[vertex][joint]`
    weights: BTreeMap<usize, BTreeMap<usize, f32>>,
}

impl<'a> Converter<'a> {
    fn new(b3d: &'a B3d) -> Self {
        Converter {
            b3d,
            warnings: Vec::new(),
            non_finite: false,
            buffer: Vec::new(),
            views: Vec::new(),
            accessors: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            skins: Vec::new(),
            samplers: Vec::new(),
            channels: Vec::new(),
        }
    }

    fn warn(&mut self, warning: String) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// adds an accessor of `count` elements, with `data` in its own buffer view
    fn accessor(&mut self, kind: &str, component: u32, target: Option<u32>, count: usize, data: Data) -> usize {
        self.non_finite |= data.non_finite;
        // keep every view aligned to 4 bytes, as accessors must be aligned to their component size
        self.buffer.resize(self.buffer.len().next_multiple_of(4), 0);
        let mut view = json!({"buffer": 0, "byteOffset": self.buffer.len(), "byteLength": data.bytes.len()});
        if let Some(target) = target {
            view["target"] = target.into();
        }
        self.buffer.extend(data.bytes);
        self.views.push(view);

        let mut accessor = json!({"bufferView": self.views.len() - 1, "componentType": component, "type": kind, "count": count});
        if let Some((min, max)) = data.bounds {
            accessor["min"] = min.into_iter().map(number).collect();
            accessor["max"] = max.into_iter().map(number).collect();
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// adds `node` and its children, returning its id. `skin` is the mesh it's in, and `bind` its transform relative to it
    fn add_node(&mut self, node: &Node, skin: Option<&mut Skin>, bind: Mat) -> Result<usize, String> {
        let id = self.nodes.len();
        // filled in after the children, which come after it
        self.nodes.push(None);
        self.add_keys(node, id);

        let (translation, rotation) = (vector(node.position), rotation(node.rotation));
        let local = trs(translation, rotation, node.scale);
        let mut own_skin = node.mesh.as_ref().map(|mesh| Skin { vertices: mesh.vertices.list.len(), ..Skin::default() });
        let (mut skin, bind) = match own_skin.as_mut() {
            Some(own) => (Some(own), IDENTITY),
            None => (skin, mul(&bind, &local)),
        };

        if let Some(weights) = &node.bone {
            let Some(skin) = skin.as_deref_mut() else {
                return Err(format!("bone `{}` isn't in a mesh", node.name));
            };
            let joint = skin.joints.len();
            skin.joints.push(id);
            skin.inverse_binds.push(inverse(&bind).ok_or_else(|| format!("bone `{}` is scaled to zero", node.name))?);
            for &(vertex, weight) in weights {
                if vertex >= skin.vertices {
                    return Err(format!("bone `{}` weights vertex {vertex}, but the mesh has {}", node.name, skin.vertices));
                }
                // Luanti adds up duplicate weights
                *skin.weights.entry(vertex).or_default().entry(joint).or_default() += weight;
            }
        }

        let mut children = Vec::new();
        for child in &node.children {
            children.push(self.add_node(child, skin.as_deref_mut(), bind)?);
        }

        let mut gltf_node = json!({"name": node.name});
        let mut neutral = None;
        if let Some(mesh) = node.mesh.as_ref().filter(|mesh| mesh.triangle_sets.iter().all(|set| set.triangles.is_empty())) {
            // glTF doesn't allow empty meshes, but the triangle sets still take up texture slots
            for set in &mesh.triangle_sets {
                self.add_material(set.buffer, set.brush_id.or(mesh.brush_id));
            }
        } else if let (Some(mesh), Some(skin)) = (&node.mesh, own_skin.as_mut()) {
            // Vertices without bones get a neutral bone, see https://github.com/KhronosGroup/glTF/issues/2269.
            // It takes the mesh node's place in the tree, with the mesh node as its child: as a sibling there would be no
            // common skeleton root, and as a child the mesh node's transform would move it.
            // Its inverse bind matrix is the mesh node's transform, which skinned vertices are relative to.
            let neutral_bone = (self.nodes.len(), local);
            gltf_node["mesh"] = self.add_mesh(mesh, skin, neutral_bone).into();
            if !skin.weights.is_empty() {
                neutral = skin.joints.contains(&neutral_bone.0).then_some(neutral_bone.0);
                gltf_node["skin"] = self.add_skin(skin, neutral).into();
            }
        }
        if !children.is_empty() {
            gltf_node["children"] = children.into();
        }
        gltf_node["translation"] = translation.into_iter().map(number).collect();
        gltf_node["rotation"] = rotation.into_iter().map(number).collect();
        gltf_node["scale"] = node.scale.into_iter().map(number).collect();
        self.nodes[id] = Some(gltf_node);

        if let Some(neutral) = neutral {
            self.nodes.push(Some(json!({"name": "neutral_bone", "children": [id]})));
            return Ok(neutral);
        }
        Ok(id)
    }

    /// adds an animation channel for each of position, rotation and scale that `node` has keys for
    fn add_keys(&mut self, node: &Node, id: usize) {
        if let Some(key) = node.keys.iter().find(|key| key.frame < 1) {
            self.warn(format!("bone `{}` has keyframe {}, Luanti uses 1 for frames below that", node.name, key.frame));
        }
        let keys = || node.keys.iter().map(|key| (key.frame.max(1), key));
        let translations = keys().filter_map(|(frame, key)| Some((frame, vector(key.position?)))).collect();
        let rotations = keys().filter_map(|(frame, key)| Some((frame, rotation(key.rotation?)))).collect();
        let scales = keys().filter_map(|(frame, key)| Some((frame, key.scale?))).collect();
        self.add_channel(node, id, "translation", "VEC3", translations);
        self.add_channel(node, id, "rotation", "VEC4", rotations);
        self.add_channel(node, id, "scale", "VEC3", scales);
    }

    fn add_channel<const N: usize>(&mut self, node: &Node, id: usize, path: &str, kind: &str, mut keys: Vec<(i32, [f32; N])>) {
        if keys.is_empty() {
            return;
        }
        keys.sort_by_key(|&(frame, _)| frame);
        let before = keys.len();
        keys.dedup_by_key(|&mut (frame, _)| frame);
        if keys.len() != before {
            self.warn(format!("bone `{}` has more than one {path} for a frame, only the first is used", node.name));
        }

        // Luanti's glTF frames are b3d frames minus one
        let times = Data::floats(keys.iter().map(|&(frame, _)| [(frame - 1) as f32])).with_bounds();
        let input = self.accessor("SCALAR", FLOAT, None, keys.len(), times);
        let output = self.accessor(kind, FLOAT, None, keys.len(), Data::floats(keys.iter().map(|&(_, v)| v)));
        // linear interpolation is the default, like b3d
        self.samplers.push(json!({"input": input, "output": output}));
        self.channels.push(json!({"sampler": self.samplers.len() - 1, "target": {"node": id, "path": path}}));
    }

    /// `neutral_bone` is the node id and inverse bind matrix of the bone for vertices without one, added to `skin` if needed
    fn add_mesh(&mut self, mesh: &Mesh, skin: &mut Skin, neutral_bone: (usize, Mat)) -> usize {
        let vertices = &mesh.vertices;
        let count = vertices.list.len();
        if count > MAX_VERTICES {
            self.warn(format!("a mesh has {count} vertices, Luanti can only load up to {MAX_VERTICES}"));
        }
        let mut attributes = Map::new();
        let positions = Data::floats(vertices.list.iter().map(|v| vector(v.pos))).with_bounds();
        attributes.insert("POSITION".into(), self.accessor("VEC3", FLOAT, Some(ARRAY_BUFFER), count, positions).into());
        if vertices.has_normals {
            // some b3d models don't have their normals normalized
            let normals = Data::floats(vertices.list.iter().map(|v| vector(normalize(v.normal))));
            attributes.insert("NORMAL".into(), self.accessor("VEC3", FLOAT, Some(ARRAY_BUFFER), count, normals).into());
        }
        if vertices.has_colors {
            let colors = Data::floats(vertices.list.iter().map(|v| v.color));
            attributes.insert("COLOR_0".into(), self.accessor("VEC4", FLOAT, Some(ARRAY_BUFFER), count, colors).into());
        }
        let size = vertices.tex_coord_set_size;
        if size < 2 && vertices.tex_coord_sets > 0 {
            self.warn("texture coordinates have less than 2 components, so they're left out".to_string());
        } else {
            for set in 0..vertices.tex_coord_sets {
                let uvs = Data::floats(vertices.list.iter().map(|v| [v.tex_coords[set * size], v.tex_coords[set * size + 1]]));
                let accessor = self.accessor("VEC2", FLOAT, Some(ARRAY_BUFFER), count, uvs);
                attributes.insert(format!("TEXCOORD_{set}"), accessor.into());
            }
        }
        if !skin.weights.is_empty() {
            self.add_weights(&mut attributes, skin, neutral_bone);
        }

        let mut primitives = Vec::new();
        for set in &mesh.triangle_sets {
            let material = self.add_material(set.buffer, set.brush_id.or(mesh.brush_id));
            if set.triangles.is_empty() {
                continue;
            }
            let mut indices = Data::default();
            for tri in &set.triangles {
                // Luanti reverses the order of glTF indices, flipping the winding order back after mirroring
                for &index in tri.iter().rev() {
                    indices.bytes.extend((index as u32).to_le_bytes());
                }
            }
            let indices = self.accessor("SCALAR", UNSIGNED_INT, Some(ELEMENT_ARRAY_BUFFER), set.triangles.len() * 3, indices);
            primitives.push(json!({"attributes": attributes, "indices": indices, "material": material}));
        }
        self.meshes.push(json!({"primitives": primitives}));
        self.meshes.len() - 1
    }

    /// adds `JOINTS_n` and `WEIGHTS_n` attributes, for each set of 4 bones influencing a vertex
    fn add_weights(&mut self, attributes: &mut Map<String, Value>, skin: &mut Skin, (neutral_id, neutral_bind): (usize, Mat)) {
        let count = skin.vertices;
        // normalized (joint, weight)s of each vertex
        let mut influences: Vec<Vec<(usize, f32)>> = (0..count)
            .map(|vertex| {
                let weights = skin.weights.get(&vertex).into_iter().flatten();
                let total: f32 = weights.clone().map(|(_, weight)| weight).sum();
                weights.map(|(&joint, &weight)| (joint, weight / total)).collect()
            })
            .collect();
        if influences.iter().any(Vec::is_empty) {
            let joint = skin.joints.len();
            skin.joints.push(neutral_id);
            skin.inverse_binds.push(neutral_bind);
            for influence in influences.iter_mut().filter(|i| i.is_empty()) {
                influence.push((joint, 1.0));
            }
        }

        let sets = influences.iter().map(Vec::len).max().unwrap_or(0).div_ceil(4);
        for set in 0..sets {
            // unused slots are joint 0 with weight 0, as glTF requires
            let slots = |influence: &Vec<(usize, f32)>| [0, 1, 2, 3].map(|n| influence.get(set * 4 + n).copied().unwrap_or((0, 0.0)));
            let mut joints = Data::default();
            for influence in &influences {
                for (joint, _) in slots(influence) {
                    joints.bytes.extend((joint as u16).to_le_bytes());
                }
            }
            let weights = Data::floats(influences.iter().map(|influence| slots(influence).map(|(_, weight)| weight)));
            let joints = self.accessor("VEC4", UNSIGNED_SHORT, Some(ARRAY_BUFFER), count, joints);
            let weights = self.accessor("VEC4", FLOAT, Some(ARRAY_BUFFER), count, weights);
            attributes.insert(format!("JOINTS_{set}"), joints.into());
            attributes.insert(format!("WEIGHTS_{set}"), weights.into());
        }
    }

    /// adds the material and texture of the `buffer`th triangle set in the file, both at that index
    fn add_material(&mut self, buffer: usize, brush_id: Option<usize>) -> usize {
        // Luanti ignores brushes and textures that don't exist
        let brush = brush_id.and_then(|id| self.b3d.brushes.get(id));
        let texture = brush.and_then(|b| b.texture_ids.first().copied().flatten()).and_then(|id| self.b3d.textures.get(id));
        if let Some(texture) = texture.filter(|t| t.scale != [1.0, 1.0]) {
            self.warn(format!("texture `{}` is scaled, which isn't converted", texture.file));
        }

        if self.materials.len() <= buffer {
            self.materials.resize(buffer + 1, None);
            self.textures.resize(buffer + 1, json!({}));
        }
        // textures only have a name, as Luanti gets them from the mod
        if let Some(name) = texture.map(|t| &t.file).or(brush.map(|b| &b.name)).filter(|n| !n.is_empty()) {
            self.textures[buffer] = json!({"name": name});
        }
        let mut material = json!({});
        let mut pbr = json!({"baseColorTexture": {"index": buffer}, "metallicFactor": 0});
        if let Some(Brush { name, color, shininess, .. }) = brush {
            material["name"] = name.as_str().into();
            pbr["baseColorFactor"] = color.map(|c| c.clamp(0.0, 1.0)).into_iter().map(number).collect();
            pbr["metallicFactor"] = number(shininess.clamp(0.0, 1.0));
        }
        material["pbrMetallicRoughness"] = pbr;
        self.materials[buffer] = Some(material);
        buffer
    }

    fn add_skin(&mut self, skin: &Skin, neutral: Option<usize>) -> usize {
        // glTF matrices are column-major
        let matrices = skin.inverse_binds.iter().map(|m| std::array::from_fn::<f32, 16, _>(|i| m[i % 4][i / 4] as f32));
        let inverse_binds = self.accessor("MAT4", FLOAT, None, skin.inverse_binds.len(), Data::floats(matrices));
        let mut json = json!({"inverseBindMatrices": inverse_binds, "joints": skin.joints});
        if let Some(neutral) = neutral {
            json["skeleton"] = neutral.into();
        }
        self.skins.push(json);
        self.skins.len() - 1
    }
}

#[derive(Default)]
struct Data {
    bytes: Vec<u8>,
    non_finite: bool,
    /// min and max of each component
    bounds: Option<(Vec<f32>, Vec<f32>)>,
    computed_bounds: Option<(Vec<f32>, Vec<f32>)>,
}

impl Data {
    fn floats<const N: usize>(values: impl IntoIterator<Item = [f32; N]>) -> Self {
        let mut data = Data::default();
        let (mut min, mut max) = ([f32::INFINITY; N], [f32::NEG_INFINITY; N]);
        for value in values {
            for (i, v) in value.into_iter().enumerate() {
                data.non_finite |= !v.is_finite();
                data.bytes.extend(v.to_le_bytes());
                min[i] = min[i].min(v);
                max[i] = max[i].max(v);
            }
        }
        data.computed_bounds = Some((min.to_vec(), max.to_vec()));
        data
    }

    /// includes the bounds in the accessor, which glTF requires for positions and keyframe times
    fn with_bounds(mut self) -> Self {
        self.bounds = self.computed_bounds.take();
        self
    }
}

/// b3d to glTF coordinates, as Luanti mirrors glTF along Z
fn vector([x, y, z]: [f32; 3]) -> [f32; 3] {
    [x, y, -z]
}

/// b3d to glTF rotation, normalized as glTF requires
fn rotation([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    let len = (x * x + y * y + z * z + w * w).sqrt();
    if len == 0.0 || !len.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [x / len, y / len, -z / len, w / len]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = v.iter().map(|c| c * c).sum::<f32>().sqrt();
    if len == 0.0 { [0.0, 1.0, 0.0] } else { v.map(|c| c / len) }
}

/// an f32 as the shortest decimal that reads back the same, e.g. `0.1` rather than `0.10000000149011612`
fn number(v: f32) -> Value {
    v.to_string().parse::<f64>().map_or(Value::Null, Value::from)
}

fn base64(data: &[u8]) -> String {
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

/// row-major 4x4 matrix
type Mat = [[f64; 4]; 4];

const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// translation * rotation * scale, as glTF composes node transforms
fn trs(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Mat {
    let [x, y, z, w] = r.map(f64::from);
    let rotation = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = rotation[i][j] * f64::from(s[j]);
        }
        m[i][3] = f64::from(t[i]);
    }
    m
}

fn mul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Gauss-Jordan elimination, `None` if it isn't invertible
fn inverse(m: &Mat) -> Option<Mat> {
    let (mut m, mut inv) = (*m, IDENTITY);
    for col in 0..4 {
        let pivot = (col..4).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        inv.swap(col, pivot);
        let factor = m[col][col];
        for j in 0..4 {
            m[col][j] /= factor;
            inv[col][j] /= factor;
        }
        for row in (0..4).filter(|&row| row != col) {
            let factor = m[row][col];
            for j in 0..4 {
                m[row][j] -= factor * m[col][j];
                inv[row][j] -= factor * inv[col][j];
            }
        }
    }
    Some(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::b3d::parse;
    use crate::parsers::b3d::tests::{Writer, b3d};

    const NO_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    fn convert_b3d(data: &[u8]) -> (Gltf, Vec<String>) {
        convert(&parse(data).unwrap()).unwrap()
    }

    /// a MESH chunk of vertices with normals and one set of UVs, and triangle sets with their brush
    fn mesh(w: &mut Writer, brush: i32, positions: &[[f32; 3]], sets: &[(i32, &[[i32; 3]])]) {
        w.chunk("MESH", |w| {
            w.int(brush);
            w.chunk("VRTS", |w| {
                w.int(1).int(1).int(2);
                for (i, pos) in positions.iter().enumerate() {
                    w.floats(pos).floats(&[0.0, 2.0, 0.0]).floats(&[i as f32 / 10.0, 0.5]);
                }
            });
            for (brush, tris) in sets {
                w.chunk("TRIS", |w| {
                    w.int(*brush);
                    for tri in *tris {
                        w.int(tri[0]).int(tri[1]).int(tri[2]);
                    }
                });
            }
        });
    }

    fn get<'a>(gltf: &'a Gltf, pointer: &str) -> &'a Value {
        gltf.json.pointer(pointer).unwrap_or_else(|| panic!("no {pointer} in {}", gltf.json))
    }

    /// an accessor's values as f64s, whatever their component type
    fn read(gltf: &Gltf, accessor: &Value) -> Vec<f64> {
        let accessor = &gltf.json["accessors"][accessor.as_u64().unwrap() as usize];
        let view = &gltf.json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        let start = view["byteOffset"].as_u64().unwrap() as usize;
        let bytes = &gltf.buffer[start..start + view["byteLength"].as_u64().unwrap() as usize];
        let values: Vec<f64> = match accessor["componentType"].as_u64().unwrap() as u32 {
            FLOAT => bytes.chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap()).into()).collect(),
            UNSIGNED_INT => bytes.chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()).into()).collect(),
            UNSIGNED_SHORT => bytes.chunks(2).map(|b| u16::from_le_bytes(b.try_into().unwrap()).into()).collect(),
            other => panic!("unexpected component type {other}"),
        };
        let size = match accessor["type"].as_str().unwrap() {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            other => panic!("unexpected type {other}"),
        };
        assert_eq!(values.len(), accessor["count"].as_u64().unwrap() as usize * size, "count matches the data");
        values
    }

    fn floats(value: &Value) -> Vec<f64> {
        value.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
    }

    fn assert_near(a: &[f64], b: &[f64]) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5), "{a:?} vs {b:?}");
    }

    fn node_matrix(node: &Value) -> Mat {
        let get = |key, default: &[f32]| node.get(key).map_or(default.to_vec(), |v| floats(v).iter().map(|&f| f as f32).collect());
        let (t, r, s) = (get("translation", &[0.0; 3]), get("rotation", &NO_ROTATION), get("scale", &[1.0; 3]));
        trs(t.try_into().unwrap(), r.try_into().unwrap(), s.try_into().unwrap())
    }

    /// node transforms in the scene, combined with their parents'
    fn global_matrices(gltf: &Gltf) -> Vec<Mat> {
        fn visit(nodes: &[Value], globals: &mut [Mat], id: usize, parent: Mat) {
            globals[id] = mul(&parent, &node_matrix(&nodes[id]));
            for child in nodes[id].get("children").and_then(Value::as_array).into_iter().flatten() {
                visit(nodes, globals, child.as_u64().unwrap() as usize, globals[id]);
            }
        }
        let nodes = gltf.json["nodes"].as_array().unwrap();
        let mut globals = vec![IDENTITY; nodes.len()];
        for root in gltf.json["scenes"][0]["nodes"].as_array().unwrap() {
            visit(nodes, &mut globals, root.as_u64().unwrap() as usize, IDENTITY);
        }
        globals
    }

    fn apply(m: &Mat, [x, y, z]: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| m[i][0] * x + m[i][1] * y + m[i][2] * z + m[i][3])
    }

    fn static_model() -> Vec<u8> {
        b3d(|w| {
            w.chunk("TEXS", |w| {
                w.string("model_skin.png").int(1).int(2).floats(&[0.0, 0.0, 1.0, 1.0, 0.0]);
            });
            w.chunk("BRUS", |w| {
                w.int(1);
                w.string("textured").floats(&[1.0, 0.5, 0.25, 1.0, 0.25]).int(1).int(0).int(0);
                w.string("plain").floats(&[1.0, 1.0, 1.0, 1.0, 2.0]).int(1).int(0).int(-1);
            });
            w.chunk("NODE", |w| {
                // rotated 90 degrees around Y
                let half = std::f32::consts::FRAC_1_SQRT_2;
                w.node_header("quad", [1.0, 2.0, 3.0], [0.0, half, 0.0, half]);
                let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 2.0], [0.0, 0.0, 2.0]];
                mesh(w, 1, &corners, &[(0, &[[0, 1, 2]]), (-1, &[[0, 2, 3]])]);
            });
        })
    }

    #[test]
    fn static_model_has_no_skin_or_animation() {
        let (gltf, warnings) = convert_b3d(&static_model());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(gltf.json.get("skins").is_none() && gltf.json.get("animations").is_none());
        assert_eq!(gltf.json["scenes"], json!([{"nodes": [0]}]));
        assert_eq!(gltf.json["nodes"].as_array().unwrap().len(), 1);
        let node = get(&gltf, "/nodes/0");
        assert_eq!(node["name"], "quad");
        assert_eq!(node["mesh"], 0);
        assert!(node.get("skin").is_none() && node.get("children").is_none());
    }

    #[test]
    fn coordinates_are_mirrored_along_z() {
        let (gltf, _) = convert_b3d(&static_model());
        let node = get(&gltf, "/nodes/0");
        assert_eq!(floats(&node["translation"]), [1.0, 2.0, -3.0]);
        let half = f64::from(std::f32::consts::FRAC_1_SQRT_2);
        // Luanti reads glTF rotations as (x, y, -z, w), so this is read back as the b3d rotation
        assert_near(&floats(&node["rotation"]), &[0.0, half, 0.0, half]);

        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        assert_eq!(read(&gltf, &attributes["POSITION"]), [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, -2.0, 0.0, 0.0, -2.0]);
        let position = &gltf.json["accessors"][attributes["POSITION"].as_u64().unwrap() as usize];
        assert_eq!(floats(&position["min"]), [0.0, 0.0, -2.0]);
        assert_eq!(floats(&position["max"]), [1.0, 0.0, 0.0]);
        // normals are normalized too
        assert_eq!(read(&gltf, &attributes["NORMAL"])[..3], [0.0, 1.0, 0.0]);
        assert_eq!(read(&gltf, &attributes["TEXCOORD_0"])[..4], [0.0, 0.5, 0.1f32 as f64, 0.5]);
        assert!(attributes.get("JOINTS_0").is_none());
    }

    #[test]
    fn rotations_are_mirrored_along_z() {
        assert_eq!(rotation([0.1, 0.2, 0.3, 0.9]).map(|f| (f * 1e4).round()), [1026.0, 2052.0, -3078.0, 9234.0]);
        assert_eq!(rotation([0.0; 4]), NO_ROTATION);
    }

    #[test]
    fn winding_order_is_reversed() {
        // Luanti reverses glTF indices when loading, getting back the b3d's order
        let (gltf, _) = convert_b3d(&static_model());
        assert_eq!(read(&gltf, get(&gltf, "/meshes/0/primitives/0/indices")), [2.0, 1.0, 0.0]);
        assert_eq!(read(&gltf, get(&gltf, "/meshes/0/primitives/1/indices")), [3.0, 2.0, 0.0]);
    }

    #[test]
    fn each_triangle_set_has_the_texture_of_its_position() {
        let (gltf, _) = convert_b3d(&static_model());
        assert_eq!(get(&gltf, "/meshes/0/primitives/0/material"), 0);
        assert_eq!(get(&gltf, "/meshes/0/primitives/1/material"), 1);
        // named after the brush's texture, or the brush
        assert_eq!(gltf.json["textures"], json!([{"name": "model_skin.png"}, {"name": "plain"}]));
        assert_eq!(
            gltf.json["materials"][0],
            json!({"name": "textured", "pbrMetallicRoughness": {
                "baseColorTexture": {"index": 0}, "metallicFactor": 0.25, "baseColorFactor": [1.0, 0.5, 0.25, 1.0]
            }})
        );
        // the triangle set has no brush, so it's the mesh's, with shininess clamped to glTF's range
        assert_eq!(get(&gltf, "/materials/1/name"), "plain");
        assert_eq!(get(&gltf, "/materials/1/pbrMetallicRoughness/baseColorTexture/index"), 1);
        assert_eq!(get(&gltf, "/materials/1/pbrMetallicRoughness/metallicFactor"), 1.0);
    }

    #[test]
    fn texture_slots_follow_triangle_sets_across_meshes() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("first", [0.0; 3], NO_ROTATION);
                mesh(w, -1, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]])]);
                w.chunk("NODE", |w| {
                    w.node_header("second", [0.0; 3], NO_ROTATION);
                    mesh(w, -1, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]]), (-1, &[[2, 1, 0]])]);
                });
            });
        });
        let (gltf, _) = convert_b3d(&data);
        assert_eq!(get(&gltf, "/meshes/0/primitives/0/material"), 1);
        assert_eq!(get(&gltf, "/meshes/1/primitives/0/material"), 0);
        assert_eq!(get(&gltf, "/meshes/0/primitives/1/material"), 2);
        assert_eq!(gltf.json["textures"], json!([{}, {}, {}]));
        assert_eq!(get(&gltf, "/materials/2/pbrMetallicRoughness"), &json!({"baseColorTexture": {"index": 2}, "metallicFactor": 0}));
    }

    #[test]
    fn empty_meshes_are_left_out_but_keep_their_texture_slots() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("empty", [0.0; 3], NO_ROTATION);
                mesh(w, -1, &[], &[(-1, &[])]);
                w.chunk("NODE", |w| {
                    w.node_header("partly", [0.0; 3], NO_ROTATION);
                    mesh(w, -1, &[[0.0; 3]; 3], &[(-1, &[]), (-1, &[[0, 1, 2]])]);
                });
            });
        });
        let (gltf, _) = convert_b3d(&data);
        assert!(get(&gltf, "/nodes/0").get("mesh").is_none());
        assert_eq!(gltf.json["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(get(&gltf, "/meshes/0/primitives").as_array().unwrap().len(), 1);
        assert_eq!(get(&gltf, "/meshes/0/primitives/0/material"), 2);
        assert_eq!(gltf.json["materials"].as_array().unwrap().len(), 3);
    }

    const ARM_ROTATION: [f32; 4] = [0.0, 0.0, 0.38268343, 0.9238795];

    /// a mesh with a root bone, an arm bone in it, and keyframes
    fn animated_model() -> Vec<u8> {
        b3d(|w| {
            w.chunk("NODE", |w| {
                // the mesh node's transform applies to vertices without bones too
                w.node_header("Armature", [0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0]);
                let positions = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.5], [2.0, 0.0, 0.0], [3.0, 3.0, 3.0]];
                mesh(w, -1, &positions, &[(-1, &[[0, 1, 2], [2, 3, 4]])]);
                w.chunk("ANIM", |w| {
                    w.int(0).int(20).floats(&[30.0]);
                });
                w.chunk("NODE", |w| {
                    w.node_header("root", [0.0, 1.0, 0.0], NO_ROTATION);
                    w.chunk("BONE", |w| {
                        w.int(0).floats(&[1.0]).int(1).floats(&[2.0]).int(2).floats(&[0.25]);
                    });
                    w.chunk("NODE", |w| {
                        w.node_header("arm", [1.0, 0.0, 0.0], ARM_ROTATION);
                        w.chunk("BONE", |w| {
                            w.int(2).floats(&[0.75]).int(3).floats(&[1.0]);
                        });
                        w.chunk("KEYS", |w| {
                            w.int(1 | 4);
                            w.int(1).floats(&[1.0, 0.0, 0.0]).floats(&[1.0, 0.0, 0.0, 0.0]);
                            w.int(11).floats(&[1.0, 0.5, 0.0]).floats(&[0.9238795, 0.0, 0.0, 0.38268343]);
                        });
                        w.chunk("KEYS", |w| {
                            w.int(2).int(6).floats(&[2.0, 2.0, 2.0]);
                        });
                    });
                });
            });
        })
    }

    #[test]
    fn bones_become_a_skin_with_a_neutral_bone() {
        let (gltf, warnings) = convert_b3d(&animated_model());
        assert!(warnings.is_empty(), "{warnings:?}");
        let names: Vec<&str> = gltf.json["nodes"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Armature", "root", "arm", "neutral_bone"]);
        // vertex 4 has no bones, so a neutral bone takes the mesh's place as the root
        assert_eq!(gltf.json["scenes"], json!([{"nodes": [3]}]));
        assert_eq!(get(&gltf, "/nodes/3/children"), &json!([0]));
        assert_eq!(get(&gltf, "/nodes/0/children"), &json!([1]));
        assert_eq!(get(&gltf, "/nodes/0/skin"), 0);
        assert_eq!(get(&gltf, "/skins/0/joints"), &json!([1, 2, 3]));
        assert_eq!(get(&gltf, "/skins/0/skeleton"), 3);

        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        #[rustfmt::skip]
        assert_eq!(read(&gltf, &attributes["JOINTS_0"]), [
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            2.0, 0.0, 0.0, 0.0,
        ]);
        // normalized
        #[rustfmt::skip]
        assert_eq!(read(&gltf, &attributes["WEIGHTS_0"]), [
            1.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            0.25, 0.75, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
        ]);
        assert!(attributes.get("JOINTS_1").is_none());
    }

    #[test]
    fn skinned_vertices_are_where_the_b3d_puts_them() {
        let (gltf, _) = convert_b3d(&animated_model());
        let globals = global_matrices(&gltf);
        let joints: Vec<usize> = floats(get(&gltf, "/skins/0/joints")).iter().map(|&j| j as usize).collect();
        let inverse_binds = read(&gltf, get(&gltf, "/skins/0/inverseBindMatrices"));
        let joint_matrix = |joint: usize| -> Mat {
            let ibm: Mat = std::array::from_fn(|i| std::array::from_fn(|j| inverse_binds[joint * 16 + j * 4 + i]));
            mul(&globals[joints[joint]], &ibm)
        };

        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        let positions = read(&gltf, &attributes["POSITION"]);
        let (joint_ids, weights) = (read(&gltf, &attributes["JOINTS_0"]), read(&gltf, &attributes["WEIGHTS_0"]));
        // Luanti puts b3d vertices at the mesh node's global transform
        let mesh_node = trs(vector([0.0, 0.0, 5.0]), rotation([0.0, 1.0, 0.0, 0.0]), [1.0; 3]);
        for vertex in 0..positions.len() / 3 {
            let pos: [f64; 3] = positions[vertex * 3..vertex * 3 + 3].try_into().unwrap();
            let mut skinned = [0.0; 3];
            for slot in vertex * 4..vertex * 4 + 4 {
                let moved = apply(&joint_matrix(joint_ids[slot] as usize), pos);
                for axis in 0..3 {
                    skinned[axis] += weights[slot] * moved[axis];
                }
            }
            assert_near(&skinned, &apply(&mesh_node, pos));
        }
    }

    #[test]
    fn keyframes_become_one_animation() {
        let (gltf, _) = convert_b3d(&animated_model());
        let animations = gltf.json["animations"].as_array().unwrap();
        assert_eq!(animations.len(), 1);
        let channels = animations[0]["channels"].as_array().unwrap();
        let paths: Vec<_> = channels.iter().map(|c| (c["target"]["node"].as_u64().unwrap(), c["target"]["path"].as_str().unwrap())).collect();
        assert_eq!(paths, [(2, "translation"), (2, "rotation"), (2, "scale")]);

        let sampler = |path: &str| {
            let channel = channels.iter().find(|c| c["target"]["path"] == path).unwrap();
            &animations[0]["samplers"][channel["sampler"].as_u64().unwrap() as usize]
        };
        // Luanti's glTF frame numbers are b3d's minus one, rather than seconds
        assert_eq!(read(&gltf, &sampler("translation")["input"]), [0.0, 10.0]);
        assert_eq!(read(&gltf, &sampler("scale")["input"]), [5.0]);
        let input = &gltf.json["accessors"][sampler("rotation")["input"].as_u64().unwrap() as usize];
        assert_eq!((&input["min"], &input["max"]), (&json!([0.0]), &json!([10.0])));
        assert_eq!(read(&gltf, &sampler("translation")["output"]), [1.0, 0.0, 0.0, 1.0, 0.5, 0.0]);
        let rotations = read(&gltf, &sampler("rotation")["output"]);
        assert_near(&rotations, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -0.38268343, 0.9238795]);
        assert_eq!(read(&gltf, &sampler("scale")["output"]), [2.0, 2.0, 2.0]);
    }

    #[test]
    fn keyframe_warnings() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("spinner", [0.0; 3], NO_ROTATION);
                w.chunk("KEYS", |w| {
                    w.int(1).int(0).floats(&[1.0, 0.0, 0.0]).int(3).floats(&[2.0, 0.0, 0.0]);
                    w.int(3).floats(&[3.0, 0.0, 0.0]);
                });
            });
        });
        let (gltf, warnings) = convert_b3d(&data);
        assert_eq!(
            warnings,
            [
                "bone `spinner` has keyframe 0, Luanti uses 1 for frames below that",
                "bone `spinner` has more than one translation for a frame, only the first is used",
            ]
        );
        let sampler = get(&gltf, "/animations/0/samplers/0");
        assert_eq!(read(&gltf, &sampler["input"]), [0.0, 2.0]);
        assert_eq!(read(&gltf, &sampler["output"]), [1.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        // nothing to put in a buffer besides the animation
        assert!(gltf.json.get("meshes").is_none());
    }

    #[test]
    fn bones_without_weights_are_plain_nodes() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]])]);
                w.chunk("NODE", |w| {
                    w.node_header("bone", [0.0; 3], NO_ROTATION);
                    w.chunk("BONE", |w| {
                        w.int(0).floats(&[0.0]);
                    });
                });
            });
        });
        let (gltf, _) = convert_b3d(&data);
        assert!(gltf.json.get("skins").is_none());
        assert_eq!(gltf.json["scenes"], json!([{"nodes": [0]}]));
    }

    #[test]
    fn errors() {
        let error = |data: Vec<u8>| convert(&parse(&data).unwrap()).err().unwrap();
        assert_eq!(error(b3d(|_| {})), "the model is empty");
        let loose_bone = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("bone", [0.0; 3], NO_ROTATION);
                w.chunk("BONE", |_| {});
            });
        });
        assert_eq!(error(loose_bone), "bone `bone` isn't in a mesh");
        let bad_weight = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]])]);
                w.chunk("NODE", |w| {
                    w.node_header("bone", [0.0; 3], NO_ROTATION);
                    w.chunk("BONE", |w| {
                        w.int(3).floats(&[1.0]);
                    });
                });
            });
        });
        assert_eq!(error(bad_weight), "bone `bone` weights vertex 3, but the mesh has 3");
        let nan = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, &[[f32::NAN, 0.0, 0.0]; 3], &[(-1, &[[0, 1, 2]])]);
            });
        });
        assert_eq!(error(nan), "the model has numbers that are infinite or NaN");
    }

    #[test]
    fn gltf_embeds_the_buffer() {
        let (gltf, _) = convert_b3d(&static_model());
        let json: Value = serde_json::from_slice(&gltf.to_gltf()).unwrap();
        let uri = json["buffers"][0]["uri"].as_str().unwrap();
        assert_eq!(uri.strip_prefix("data:application/octet-stream;base64,").unwrap(), base64(&gltf.buffer));
        assert_eq!(json["buffers"][0]["byteLength"], gltf.buffer.len());
        assert_eq!([base64(b"Man"), base64(b"Ma"), base64(b"M"), base64(b"")], ["TWFu", "TWE=", "TQ==", ""]);
    }

    #[test]
    fn glb_has_json_and_binary_chunks() {
        let (gltf, _) = convert_b3d(&animated_model());
        let glb = gltf.to_glb();
        let u32_at = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!((u32_at(4), u32_at(8)), (2, glb.len()));
        let json_len = u32_at(12);
        assert_eq!(&glb[16..20], b"JSON");
        let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json, gltf.json);
        assert!(json["buffers"][0].get("uri").is_none());
        let bin = 20 + json_len;
        assert_eq!(&glb[bin + 4..bin + 8], b"BIN\0");
        assert_eq!(glb[bin + 8..bin + 8 + gltf.buffer.len()], gltf.buffer);
        assert_eq!((json_len % 4, u32_at(bin) % 4), (0, 0));
    }

    #[test]
    fn inverse_undoes_transforms() {
        let m = trs([1.0, -2.0, 3.0], rotation([0.1, 0.2, 0.3, 0.9]), [2.0, 0.5, 1.5]);
        let product = mul(&m, &inverse(&m).unwrap());
        assert_near(&product.concat(), &IDENTITY.concat());
        assert!(inverse(&trs([0.0; 3], NO_ROTATION, [1.0, 0.0, 1.0])).is_none());
    }
}
