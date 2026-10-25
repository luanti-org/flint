//! builds the glTF of a model as it's read, which is what Luanti would make of it whatever its format (its `SkinnedMesh`)
//!
//! readers do what Luanti's loader of their format does, in glTF's coordinates: Luanti mirrors glTF along Z, so readers
//! mirror what Luanti reads as is. what's left is how Luanti's glTF loader differs, which `finish` handles:
//! * it reverses the order of vertex indices, so triangles are written backwards
//! * it uses keyframe times as frame numbers, so they're written as is, not as seconds
//! * a primitive gets the `textures` entry of its glTF texture index, so primitive `n` gets texture `n`

use std::collections::BTreeMap;

use gltf_json as json;
use json::accessor::{ComponentType, GenericComponentType, Type};
use json::animation::{Interpolation, Property};
use json::buffer::Target;
use json::mesh::{Mode, Semantic};
use json::validation::Checked::Valid;
use json::validation::USize64;
use json::{Index, Root};

use super::gltf::Gltf;

/// Luanti stores vertex indices as 16 bit, and doesn't allow the max value
const MAX_VERTICES: usize = u16::MAX as usize - 1;

/// row-major 4x4 matrix
pub type Mat = [[f64; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

#[derive(Default)]
pub struct Builder {
    /// Luanti's joints, each a node
    pub nodes: Vec<Node>,
    /// Luanti's mesh buffers, in its order, which a model's `textures` follow
    pub primitives: Vec<Primitive>,
}

pub struct Node {
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub transform: Transform,
    /// its keyframes, if it's in the animation. one with a matrix transform starts from no transform when animated
    pub keys: Option<Keys>,
    /// set by the file, otherwise the inverse of its transform relative to the model
    pub inverse_bind: Option<Mat>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    Trs { translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3] },
    Matrix(Mat),
}

impl Transform {
    pub const NONE: Transform = Transform::Trs { translation: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] };

    pub fn matrix(&self) -> Mat {
        match *self {
            Transform::Trs { translation, rotation, scale } => trs(translation, rotation, scale),
            Transform::Matrix(m) => m,
        }
    }
}

/// keyframes by frame number
#[derive(Default, Debug, PartialEq)]
pub struct Keys {
    pub position: Vec<(f32, [f32; 3])>,
    pub rotation: Vec<(f32, [f32; 4])>,
    pub scale: Vec<(f32, [f32; 3])>,
}

impl Keys {
    /// like Luanti: drops keyframes that aren't after the one before, and the middle of three that are the same
    pub fn cleanup(&mut self) {
        cleanup(&mut self.position);
        cleanup(&mut self.rotation);
        cleanup(&mut self.scale);
    }
}

fn cleanup<T: PartialEq + Copy>(frames: &mut Vec<(f32, T)>) {
    let mut ordered: Vec<(f32, T)> = Vec::new();
    for &frame in frames.iter() {
        if ordered.last().is_none_or(|last| frame.0 > last.0) {
            ordered.push(frame);
        }
    }
    frames.clear();
    for (i, &frame) in ordered.iter().enumerate() {
        let redundant = i > 0 && i + 1 < ordered.len() && ordered[i - 1].1 == frame.1 && ordered[i + 1].1 == frame.1;
        if !redundant {
            frames.push(frame);
        }
    }
}

#[derive(Default)]
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[usize; 3]>,
    pub has_normals: bool,
    pub has_colors: bool,
    /// texture coordinate sets, up to 2
    pub uv_sets: usize,
    /// the node it moves with, for primitives that aren't skinned
    pub node: Option<usize>,
    pub material: Material,
}

impl Primitive {
    pub fn is_skinned(&self) -> bool {
        self.vertices.iter().any(|v| v.weights.iter().any(|&(_, w)| w > 0.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [[f32; 2]; 2],
    /// (node, weight), not normalized
    pub weights: [(usize, f32); 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Vertex { pos: [0.0; 3], normal: [0.0; 3], color: [1.0; 4], uv: [[0.0; 2]; 2], weights: [(0, 0.0); 4] }
    }
}

impl Vertex {
    /// like Luanti, which keeps the 4 strongest
    pub fn add_weight(&mut self, node: usize, weight: f32) {
        let weakest = (0..4).min_by(|&a, &b| self.weights[a].1.total_cmp(&self.weights[b].1)).unwrap();
        if self.weights[weakest].1 <= weight {
            self.weights[weakest] = (node, weight);
        }
    }
}

/// what's kept of a material: Luanti only uses its texture slot
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Material {
    pub name: Option<String>,
    pub texture: Option<String>,
    pub color: Option<[f32; 4]>,
    pub shininess: Option<f32>,
}

#[derive(Debug, PartialEq)]
pub struct Warning {
    pub message: String,
    /// whether Luanti shows the result differently from the original
    pub changes_model: bool,
}

impl Builder {
    /// `node`'s transform relative to the model, in its rest pose
    pub fn global(&self, node: usize) -> Mat {
        let mut matrix = self.nodes[node].transform.matrix();
        let mut parent = self.nodes[node].parent;
        // a cycle is caught by `finish`, this just mustn't hang
        for _ in 0..self.nodes.len() {
            let Some(p) = parent else { break };
            matrix = mul(&self.nodes[p].transform.matrix(), &matrix);
            parent = self.nodes[p].parent;
        }
        matrix
    }

    /// the glTF, and warnings about what couldn't be converted
    pub fn finish(&self) -> Result<(Gltf, Vec<Warning>), String> {
        if self.nodes.is_empty() && self.primitives.iter().all(|p| p.triangles.is_empty()) {
            return Err("the model is empty".to_string());
        }
        self.check_tree()?;
        let mut w = Writer::default();

        // Luanti's joints are the first nodes, by index
        for (id, node) in self.nodes.iter().enumerate() {
            let children: Vec<Index<json::Node>> =
                (0..self.nodes.len()).filter(|&c| self.nodes[c].parent == Some(id)).map(index).collect();
            let mut gltf_node = json::Node {
                name: node.name.clone().filter(|n| !n.is_empty()),
                children: (!children.is_empty()).then_some(children),
                ..json::Node::default()
            };
            match (node.transform, &node.keys) {
                // glTF matrices are column-major
                (Transform::Matrix(m), None) => gltf_node.matrix = Some(std::array::from_fn(|i| m[i % 4][i / 4] as f32)),
                (transform, keys) => {
                    // animated nodes need a translation, rotation and scale, rather than a matrix
                    let (translation, rotation, scale) = match transform {
                        Transform::Trs { translation, rotation, scale } => (translation, rotation, scale),
                        Transform::Matrix(m) => decompose(&m).unwrap_or_else(|| {
                            let name = node.name.as_deref().unwrap_or_default();
                            w.warn(format!("joint `{name}`'s rest pose can't be split into a position, rotation and scale"), true);
                            ([m[0][3], m[1][3], m[2][3]].map(|t| t as f32), [0.0, 0.0, 0.0, 1.0], [1.0; 3])
                        }),
                    };
                    gltf_node.translation = Some(translation);
                    gltf_node.rotation = Some(json::scene::UnitQuaternion(rotation));
                    gltf_node.scale = Some(scale);
                    if let Some(keys) = keys {
                        // Luanti starts animated joints with a matrix from no transform, rather than their rest pose
                        let from_nothing = matches!(transform, Transform::Matrix(_));
                        w.add_channel(id, Property::Translation, Type::Vec3, &or_nothing(&keys.position, from_nothing, [0.0; 3]));
                        w.add_channel(id, Property::Rotation, Type::Vec4, &or_nothing(&keys.rotation, from_nothing, [0.0, 0.0, 0.0, 1.0]));
                        w.add_channel(id, Property::Scale, Type::Vec3, &or_nothing(&keys.scale, from_nothing, [1.0; 3]));
                    }
                }
            }
            w.root.push(gltf_node);
        }
        let mut scene: Vec<usize> = (0..self.nodes.len()).filter(|&n| self.nodes[n].parent.is_none()).collect();

        // skinned primitives share a skin of the nodes weighting them. vertices without weights don't move, so get a
        // bone that doesn't: a root above all nodes, which also gives them the common root glTF needs
        let skinned: Vec<&Primitive> = self.primitives.iter().filter(|p| p.is_skinned()).collect();
        let root = (!skinned.is_empty()).then(|| {
            let children = scene.iter().map(|&n| index(n)).collect::<Vec<_>>();
            let root = w.root.push(json::Node { children: (!children.is_empty()).then_some(children), ..json::Node::default() });
            scene = vec![root.value()];
            root.value()
        });
        let weighted = |node: usize| skinned.iter().flat_map(|p| &p.vertices).any(|v| v.weights.iter().any(|&(n, w)| n == node && w > 0.0));
        let mut skin_nodes: Vec<usize> = (0..self.nodes.len()).filter(|&n| weighted(n)).collect();
        if skinned.iter().flat_map(|p| &p.vertices).any(|v| v.weights.iter().all(|&(_, w)| w <= 0.0)) {
            skin_nodes.push(self.nodes.len());
        }

        // primitives in a mesh each: skinned ones together, ones following a node on that node, others together
        // by the node they follow, and whether they're skinned
        let mut meshes: Vec<(MeshKey, Vec<json::mesh::Primitive>)> = Vec::new();
        for (i, primitive) in self.primitives.iter().enumerate() {
            w.add_material(&primitive.material);
            if primitive.triangles.is_empty() {
                continue;
            }
            let key = (if primitive.is_skinned() { None } else { primitive.node }, primitive.is_skinned());
            let gltf_primitive = w.add_primitive(i, primitive, &skin_nodes);
            match meshes.iter_mut().find(|(k, _)| *k == key) {
                Some((_, primitives)) => primitives.push(gltf_primitive),
                None => meshes.push((key, vec![gltf_primitive])),
            }
        }
        for ((node, skinned), primitives) in meshes {
            let mesh = w.root.push(json::Mesh { primitives, weights: None, name: None, extensions: None, extras: Default::default() });
            match node {
                Some(node) => w.root.nodes[node].mesh = Some(mesh),
                None => {
                    let skin = skinned.then(|| index(0));
                    let node = w.root.push(json::Node { mesh: Some(mesh), skin, ..json::Node::default() });
                    scene.push(node.value());
                }
            }
        }

        if let Some(root) = root {
            let mut matrices = Vec::new();
            for &node in &skin_nodes {
                let inverse_bind = match self.nodes.get(node) {
                    Some(n) => match n.inverse_bind {
                        Some(m) => m,
                        None => inverse(&self.global(node)).ok_or_else(|| format!("joint `{}` is scaled to zero", n.name.as_deref().unwrap_or_default()))?,
                    },
                    // the root
                    None => IDENTITY,
                };
                // glTF matrices are column-major
                matrices.push(std::array::from_fn::<f32, 16, _>(|i| inverse_bind[i % 4][i / 4] as f32));
            }
            let inverse_binds = w.accessor(Type::Mat4, ComponentType::F32, None, matrices.len(), Data::floats(matrices));
            let joints = skin_nodes.iter().map(|&n| index(if n == self.nodes.len() { root } else { n })).collect();
            w.root.push(json::Skin { inverse_bind_matrices: Some(inverse_binds), joints, name: None, skeleton: Some(index(root)), extensions: None, extras: Default::default() });
        }
        if w.non_finite {
            return Err("the model has numbers that are infinite or NaN".to_string());
        }

        let Writer { mut root, buffer, warnings, samplers, channels, .. } = w;
        root.asset = json::Asset { generator: Some("flint convert".into()), version: "2.0".into(), copyright: None, min_version: None, extensions: None, extras: Default::default() };
        root.scenes.push(json::Scene { nodes: scene.into_iter().map(index).collect(), name: None, extensions: None, extras: Default::default() });
        root.scene = Some(index(0));
        if !channels.is_empty() {
            // Luanti only has a single animation
            root.push(json::Animation { channels, samplers, name: None, extensions: None, extras: Default::default() });
        }
        if !buffer.is_empty() {
            root.push(json::Buffer { byte_length: USize64(buffer.len() as u64), uri: None, name: None, extensions: None, extras: Default::default() });
        }
        Ok((Gltf { root, buffer }, warnings))
    }

    /// nodes' parents must exist and not loop
    fn check_tree(&self) -> Result<(), String> {
        for (id, node) in self.nodes.iter().enumerate() {
            let mut parent = node.parent;
            for _ in 0..self.nodes.len() {
                match parent {
                    None => break,
                    Some(p) if p >= self.nodes.len() => return Err(format!("joint {id}'s parent doesn't exist")),
                    Some(p) => parent = self.nodes[p].parent,
                }
            }
            if parent.is_some() {
                return Err(format!("joint `{}` is its own parent", node.name.as_deref().unwrap_or_default()));
            }
        }
        Ok(())
    }
}

/// the node a mesh's primitives follow, and whether they're skinned
type MeshKey = (Option<usize>, bool);

fn index<T>(i: usize) -> Index<T> {
    Index::new(i as u32)
}

/// `keys`, or `nothing` throughout when the node starts from nothing
fn or_nothing<const N: usize>(keys: &[(f32, [f32; N])], from_nothing: bool, nothing: [f32; N]) -> Vec<(f32, [f32; N])> {
    match (keys.is_empty(), from_nothing) {
        (true, true) => vec![(0.0, nothing)],
        _ => keys.to_vec(),
    }
}

/// the glTF as it's written
#[derive(Default)]
struct Writer {
    root: Root,
    buffer: Vec<u8>,
    warnings: Vec<Warning>,
    non_finite: bool,
    samplers: Vec<json::animation::Sampler>,
    channels: Vec<json::animation::Channel>,
}

impl Writer {
    fn warn(&mut self, message: String, changes_model: bool) {
        let warning = Warning { message, changes_model };
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// adds an accessor of `count` elements, with `data` in its own buffer view
    fn accessor(&mut self, kind: Type, component: ComponentType, target: Option<Target>, count: usize, data: Data) -> Index<json::Accessor> {
        self.non_finite |= data.non_finite;
        // keep every view aligned to 4 bytes, as accessors must be aligned to their component size
        self.buffer.resize(self.buffer.len().next_multiple_of(4), 0);
        let view = self.root.push(json::buffer::View {
            buffer: index(0),
            byte_length: USize64(data.bytes.len() as u64),
            byte_offset: Some(USize64(self.buffer.len() as u64)),
            byte_stride: None,
            name: None,
            target: target.map(Valid),
            extensions: None,
            extras: Default::default(),
        });
        self.buffer.extend(data.bytes);
        let numbers = |values: Vec<f32>| json::Value::Array(values.into_iter().map(number).collect());
        let (min, max) = data.bounds.map_or((None, None), |(min, max)| (Some(numbers(min)), Some(numbers(max))));
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: None,
            count: USize64(count as u64),
            component_type: Valid(GenericComponentType(component)),
            extensions: None,
            extras: Default::default(),
            type_: Valid(kind),
            min,
            max,
            name: None,
            normalized: false,
            sparse: None,
        })
    }

    fn add_channel<const N: usize>(&mut self, node: usize, path: Property, kind: Type, keys: &[(f32, [f32; N])]) {
        if keys.is_empty() {
            return;
        }
        let times = Data::floats(keys.iter().map(|&(frame, _)| [frame])).with_bounds();
        let input = self.accessor(Type::Scalar, ComponentType::F32, None, keys.len(), times);
        let output = self.accessor(kind, ComponentType::F32, None, keys.len(), Data::floats(keys.iter().map(|&(_, v)| v)));
        // linear interpolation, like Luanti's
        self.samplers.push(json::animation::Sampler { input, interpolation: Valid(Interpolation::Linear), output, extensions: None, extras: Default::default() });
        self.channels.push(json::animation::Channel {
            sampler: index(self.samplers.len() - 1),
            target: json::animation::Target { node: index(node), path: Valid(path), extensions: None, extras: Default::default() },
            extensions: None,
            extras: Default::default(),
        });
    }

    /// the `i`th primitive, using the texture of that index. `skin_nodes` are the nodes of the skin, by index, the root
    /// being the one past the last node
    fn add_primitive(&mut self, i: usize, primitive: &Primitive, skin_nodes: &[usize]) -> json::mesh::Primitive {
        let vertices = &primitive.vertices;
        let count = vertices.len();
        if count > MAX_VERTICES {
            self.warn(format!("a mesh has {count} vertices, Luanti can only load up to {MAX_VERTICES} from glTF"), true);
        }
        let mut attributes = BTreeMap::new();
        let mut attribute = |w: &mut Self, semantic, kind, component, data| {
            attributes.insert(Valid(semantic), w.accessor(kind, component, Some(Target::ArrayBuffer), count, data));
        };
        let floats = ComponentType::F32;
        attribute(self, Semantic::Positions, Type::Vec3, floats, Data::floats(vertices.iter().map(|v| v.pos)).with_bounds());
        if primitive.has_normals {
            // glTF needs them normalized, Luanti does that anyway
            attribute(self, Semantic::Normals, Type::Vec3, floats, Data::floats(vertices.iter().map(|v| normalize(v.normal))));
        }
        if primitive.has_colors {
            let colors = Data::floats(vertices.iter().map(|v| v.color.map(|c| c.clamp(0.0, 1.0))));
            attribute(self, Semantic::Colors(0), Type::Vec4, floats, colors);
        }
        for set in 0..primitive.uv_sets.min(2) {
            attribute(self, Semantic::TexCoords(set as u32), Type::Vec2, floats, Data::floats(vertices.iter().map(|v| v.uv[set])));
        }
        if primitive.is_skinned() {
            let (mut joints, mut weights) = (Data::default(), Vec::new());
            for vertex in vertices {
                // a node twice is once with both weights, and they're normalized, both like Luanti
                let mut merged: Vec<(usize, f32)> = Vec::new();
                for &(node, weight) in vertex.weights.iter().filter(|&&(_, w)| w > 0.0) {
                    match merged.iter_mut().find(|(n, _)| *n == node) {
                        Some((_, w)) => *w += weight,
                        None => merged.push((node, weight)),
                    }
                }
                let total: f32 = merged.iter().map(|&(_, w)| w).sum();
                let slots: Vec<(usize, f32)> = match total > 0.0 {
                    true => merged.iter().map(|&(node, w)| (skin_nodes.iter().position(|&n| n == node).unwrap(), w / total)).collect(),
                    false => vec![(skin_nodes.len() - 1, 1.0)],
                };
                let slot = |n: usize| slots.get(n).copied().unwrap_or((0, 0.0));
                for n in 0..4 {
                    joints.bytes.extend((slot(n).0 as u16).to_le_bytes());
                }
                weights.push([0, 1, 2, 3].map(|n| slot(n).1));
            }
            attribute(self, Semantic::Joints(0), Type::Vec4, ComponentType::U16, joints);
            attribute(self, Semantic::Weights(0), Type::Vec4, floats, Data::floats(weights));
        }

        let mut indices = Data::default();
        for tri in &primitive.triangles {
            // Luanti reverses the order of glTF indices, flipping the winding order back after mirroring
            for &v in tri.iter().rev() {
                indices.bytes.extend((v as u32).to_le_bytes());
            }
        }
        let indices = self.accessor(Type::Scalar, ComponentType::U32, Some(Target::ElementArrayBuffer), primitive.triangles.len() * 3, indices);
        json::mesh::Primitive {
            attributes,
            indices: Some(indices),
            material: Some(index(i)),
            mode: Valid(Mode::Triangles),
            targets: None,
            extensions: None,
            extras: Default::default(),
        }
    }

    /// the material and texture of the next primitive, at the index it has in the model
    fn add_material(&mut self, material: &Material) {
        // textures only have a name, as Luanti gets them from the mod
        let name = material.texture.as_ref().or(material.name.as_ref()).filter(|n| !n.is_empty()).cloned();
        let texture = self.root.push(json::Texture { name, sampler: None, source: Index::new(u32::MAX), extensions: None, extras: Default::default() });
        let pbr = json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor(material.color.unwrap_or([1.0; 4]).map(|c| c.clamp(0.0, 1.0))),
            base_color_texture: Some(json::texture::Info { index: texture, tex_coord: 0, extensions: None, extras: Default::default() }),
            metallic_factor: json::material::StrengthFactor(material.shininess.unwrap_or(0.0).clamp(0.0, 1.0)),
            ..Default::default()
        };
        self.root.push(json::Material { name: material.name.clone(), pbr_metallic_roughness: pbr, ..Default::default() });
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

/// a normal glTF accepts: zero ones point up
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = v.iter().map(|c| c * c).sum::<f32>().sqrt();
    if len == 0.0 || !len.is_finite() { [0.0, 1.0, 0.0] } else { v.map(|c| c / len) }
}

/// an f32 as the shortest decimal that reads back the same, e.g. `0.1` rather than `0.10000000149011612`
fn number(v: f32) -> json::Value {
    v.to_string().parse::<f64>().map_or(json::Value::Null, json::Value::from)
}

/// translation * rotation * scale, as glTF composes node transforms
pub fn trs(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Mat {
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

/// translation, rotation and scale that `trs` makes `m` of, if it's made of only those
pub fn decompose(m: &Mat) -> Option<([f32; 3], [f32; 4], [f32; 3])> {
    let column = |j: usize| [m[0][j], m[1][j], m[2][j]];
    let length = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let mut scale = [length(column(0)), length(column(1)), length(column(2))];
    if scale.iter().any(|&s| s < 1e-12) {
        return None;
    }
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    // a mirroring is a negative scale
    if det < 0.0 {
        scale[0] = -scale[0];
    }
    let r: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| m[i][j] / scale[j]));
    // Shepperd's method, from the largest of w, x, y, z
    let trace = r[0][0] + r[1][1] + r[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(r[2][1] - r[1][2]) / s, (r[0][2] - r[2][0]) / s, (r[1][0] - r[0][1]) / s, s / 4.0]
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        [s / 4.0, (r[0][1] + r[1][0]) / s, (r[0][2] + r[2][0]) / s, (r[2][1] - r[1][2]) / s]
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        [(r[0][1] + r[1][0]) / s, s / 4.0, (r[1][2] + r[2][1]) / s, (r[0][2] - r[2][0]) / s]
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        [(r[0][2] + r[2][0]) / s, (r[1][2] + r[2][1]) / s, s / 4.0, (r[1][0] - r[0][1]) / s]
    };
    let length = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    let rotation = q.map(|c| (c / length) as f32);
    let translation = [m[0][3], m[1][3], m[2][3]].map(|t| t as f32);
    let scale = scale.map(|s| s as f32);

    // only when it's made back, i.e. it has no shear or projection
    let made = trs(translation, rotation, scale);
    let size = m.iter().flatten().fold(1.0f64, |max, v| max.max(v.abs()));
    made.iter().flatten().zip(m.iter().flatten()).all(|(a, b)| (a - b).abs() <= 1e-4 * size).then_some((translation, rotation, scale))
}

pub fn mul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Gauss-Jordan elimination, `None` if it isn't invertible
pub fn inverse(m: &Mat) -> Option<Mat> {
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

/// Luanti's coordinates to glTF's, which it mirrors along Z
pub fn vector([x, y, z]: [f32; 3]) -> [f32; 3] {
    [x, y, -z]
}

/// a rotation in Luanti's coordinates to glTF's, normalized as glTF requires
pub fn rotation([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    let len = (x * x + y * y + z * z + w * w).sqrt();
    if len == 0.0 || !len.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [x / len, y / len, -z / len, w / len]
}

/// a matrix in Luanti's coordinates to glTF's, from the 16 numbers Luanti stores it as
pub fn matrix(stored: [f32; 16]) -> Mat {
    // Luanti's matrices are column-major, like glTF's. mirroring is Z * m * Z, negating the third row and column
    let sign = |i: usize| if i == 2 { -1.0 } else { 1.0 };
    std::array::from_fn(|r| std::array::from_fn(|c| f64::from(stored[c * 4 + r]) * sign(r) * sign(c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::gltf::base64;
    use crate::convert::{b3d, x};
    use serde_json::{Value, json};
    use crate::parsers::b3d::tests::{Writer, b3d};

    const NO_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    /// the glTF and its JSON, as tests look at it
    struct Out {
        json: Value,
        buffer: Vec<u8>,
        gltf: Gltf,
    }

    fn convert_ok(builder: &Builder) -> Out {
        let (gltf, warnings) = builder.finish().unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        // as written, where numbers are f32s
        let json = serde_json::from_slice(&serde_json::to_vec(&gltf.root).unwrap()).unwrap();
        Out { json, buffer: gltf.buffer.clone(), gltf }
    }

    fn get<'a>(gltf: &'a Out, pointer: &str) -> &'a Value {
        gltf.json.pointer(pointer).unwrap_or_else(|| panic!("no {pointer} in {}", gltf.json))
    }

    /// an accessor's values as f64s, whatever their component type
    fn read(gltf: &Out, accessor: &Value) -> Vec<f64> {
        let accessor = &gltf.json["accessors"][accessor.as_u64().unwrap() as usize];
        let view = &gltf.json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        let start = view["byteOffset"].as_u64().unwrap() as usize;
        let bytes = &gltf.buffer[start..start + view["byteLength"].as_u64().unwrap() as usize];
        let values: Vec<f64> = match accessor["componentType"].as_u64().unwrap() as u32 {
            5126 => bytes.chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap()).into()).collect(),
            5125 => bytes.chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()).into()).collect(),
            5123 => bytes.chunks(2).map(|b| u16::from_le_bytes(b.try_into().unwrap()).into()).collect(),
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
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4), "{a:?} vs {b:?}");
    }

    fn node_matrix(node: &Value) -> Mat {
        if let Some(m) = node.get("matrix") {
            let m = floats(m);
            return std::array::from_fn(|r| std::array::from_fn(|c| m[c * 4 + r]));
        }
        let get = |key, default: &[f32]| node.get(key).map_or(default.to_vec(), |v| floats(v).iter().map(|&f| f as f32).collect());
        let (t, r, s) = (get("translation", &[0.0; 3]), get("rotation", &NO_ROTATION), get("scale", &[1.0; 3]));
        trs(t.try_into().unwrap(), r.try_into().unwrap(), s.try_into().unwrap())
    }

    /// node transforms in the scene, combined with their parents'
    fn global_matrices(gltf: &Out) -> Vec<Mat> {
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

    /// checks skinned vertices are where the model has them in its rest pose, as glTF skins them
    fn assert_skin_keeps_rest_pose(gltf: &Out, model: &Builder) {
        let globals = global_matrices(gltf);
        let joints: Vec<usize> = floats(get(gltf, "/skins/0/joints")).iter().map(|&j| j as usize).collect();
        let inverse_binds = read(gltf, get(gltf, "/skins/0/inverseBindMatrices"));
        let joint_matrix = |joint: usize| -> Mat {
            let ibm: Mat = std::array::from_fn(|i| std::array::from_fn(|j| inverse_binds[joint * 16 + j * 4 + i]));
            mul(&globals[joints[joint]], &ibm)
        };
        let skinned_mesh = gltf.json["nodes"].as_array().unwrap().iter().find(|n| n.get("skin").is_some()).unwrap()["mesh"].as_u64().unwrap();
        let primitives = gltf.json["meshes"][skinned_mesh as usize]["primitives"].as_array().unwrap();
        for primitive in primitives {
            let attributes = &primitive["attributes"];
            let positions = read(gltf, &attributes["POSITION"]);
            let (joint_ids, weights) = (read(gltf, &attributes["JOINTS_0"]), read(gltf, &attributes["WEIGHTS_0"]));
            let buffer = &model.primitives[primitive["material"].as_u64().unwrap() as usize];
            for vertex in 0..positions.len() / 3 {
                let pos: [f64; 3] = positions[vertex * 3..vertex * 3 + 3].try_into().unwrap();
                assert_eq!(pos, buffer.vertices[vertex].pos.map(f64::from));
                let mut skinned = [0.0; 3];
                for slot in vertex * 4..vertex * 4 + 4 {
                    let moved = apply(&joint_matrix(joint_ids[slot] as usize), pos);
                    for axis in 0..3 {
                        skinned[axis] += weights[slot] * moved[axis];
                    }
                }
                assert_near(&skinned, &pos);
            }
        }
    }

    fn static_b3d() -> Vec<u8> {
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
                w.node_header("quad", [1.0, 2.0, 3.0], NO_ROTATION);
                mesh(w, 1, &[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 2.0], [0.0, 0.0, 2.0]], &[(0, &[[0, 1, 2]]), (-1, &[[0, 2, 3]])]);
            });
        })
    }

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

    #[test]
    fn static_model_is_a_mesh_beside_its_joints() {
        let model = b3d::read(&static_b3d()).unwrap();
        let gltf = convert_ok(&model);
        assert!(gltf.json.get("skins").is_none() && gltf.json.get("animations").is_none());
        // the joint, then the mesh, whose vertices are already where the joint puts them
        assert_eq!(gltf.json["scenes"], json!([{"nodes": [0, 1]}]));
        assert_eq!(get(&gltf, "/nodes/0/name"), "quad");
        assert_eq!(floats(get(&gltf, "/nodes/0/translation")), [1.0, 2.0, -3.0]);
        assert_eq!(get(&gltf, "/nodes/1"), &json!({"mesh": 0}));
        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        assert_eq!(read(&gltf, &attributes["POSITION"]), [1.0, 2.0, -3.0, 2.0, 2.0, -3.0, 2.0, 2.0, -5.0]);
        let position = &gltf.json["accessors"][attributes["POSITION"].as_u64().unwrap() as usize];
        assert_eq!((floats(&position["min"]), floats(&position["max"])), (vec![1.0, 2.0, -5.0], vec![2.0, 2.0, -3.0]));
        assert_eq!(read(&gltf, &attributes["NORMAL"])[..3], [0.0, 1.0, 0.0]);
        assert_eq!(read(&gltf, &attributes["TEXCOORD_0"])[..2], [0.0, 0.5]);
        assert!(attributes.get("JOINTS_0").is_none());
    }

    #[test]
    fn winding_order_is_reversed() {
        // Luanti reverses glTF indices when loading, getting back the original order
        let gltf = convert_ok(&b3d::read(&static_b3d()).unwrap());
        assert_eq!(read(&gltf, get(&gltf, "/meshes/0/primitives/0/indices")), [2.0, 1.0, 0.0]);
        assert_eq!(read(&gltf, get(&gltf, "/meshes/0/primitives/1/indices")), [2.0, 1.0, 0.0]);
    }

    #[test]
    fn each_buffer_has_the_texture_of_its_index() {
        let gltf = convert_ok(&b3d::read(&static_b3d()).unwrap());
        assert_eq!(get(&gltf, "/meshes/0/primitives/0/material"), 0);
        assert_eq!(get(&gltf, "/meshes/0/primitives/1/material"), 1);
        // named after the texture, or the material
        assert_eq!(gltf.json["textures"], json!([{"name": "model_skin.png"}, {"name": "plain"}]));
        assert_eq!(get(&gltf, "/materials/0/name"), "textured");
        assert_eq!(get(&gltf, "/materials/0/pbrMetallicRoughness/baseColorTexture/index"), 0);
        assert_eq!(get(&gltf, "/materials/0/pbrMetallicRoughness/metallicFactor"), 0.25);
        assert_eq!(get(&gltf, "/materials/0/pbrMetallicRoughness/baseColorFactor"), &json!([1.0, 0.5, 0.25, 1.0]));
        // shininess is clamped to glTF's range
        assert_eq!(get(&gltf, "/materials/1/pbrMetallicRoughness/metallicFactor"), 1.0);
    }

    #[test]
    fn empty_buffers_keep_their_texture_slots() {
        let mut model = b3d::read(&static_b3d()).unwrap();
        model.primitives.insert(0, Primitive::default());
        let gltf = convert_ok(&model);
        assert_eq!(gltf.json["materials"].as_array().unwrap().len(), 3);
        assert_eq!(get(&gltf, "/meshes/0/primitives/0/material"), 1);
    }

    /// a mesh with a root bone, an arm bone in it, and keyframes
    fn animated_b3d() -> Vec<u8> {
        b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("Armature", [0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0]);
                let positions = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.5], [2.0, 0.0, 0.0], [3.0, 3.0, 3.0]];
                mesh(w, -1, &positions, &[(-1, &[[0, 1, 2], [2, 3, 4]])]);
                w.chunk("NODE", |w| {
                    w.node_header("root", [0.0, 1.0, 0.0], NO_ROTATION);
                    w.chunk("BONE", |w| {
                        w.int(0).floats(&[1.0]).int(1).floats(&[2.0]).int(2).floats(&[0.25]);
                    });
                    w.chunk("NODE", |w| {
                        w.node_header("arm", [1.0, 0.0, 0.0], [0.0, 0.0, 0.38268343, 0.9238795]);
                        w.chunk("BONE", |w| {
                            w.int(2).floats(&[0.75]).int(3).floats(&[1.0]);
                        });
                        w.chunk("KEYS", |w| {
                            w.int(1 | 4);
                            w.int(1).floats(&[1.0, 0.0, 0.0]).floats(&[1.0, 0.0, 0.0, 0.0]);
                            w.int(11).floats(&[1.0, 0.5, 0.0]).floats(&[0.9238795, 0.0, 0.0, 0.38268343]);
                        });
                    });
                });
            });
        })
    }

    #[test]
    fn bones_become_a_skin_with_a_root() {
        let gltf = convert_ok(&b3d::read(&animated_b3d()).unwrap());
        // the joints, the root above them, then the skinned mesh
        assert_eq!(gltf.json["nodes"].as_array().unwrap().len(), 5);
        assert_eq!(get(&gltf, "/nodes/3"), &json!({"children": [0]}));
        assert_eq!(get(&gltf, "/nodes/4"), &json!({"mesh": 0, "skin": 0}));
        assert_eq!(gltf.json["scenes"], json!([{"nodes": [3, 4]}]));
        // vertex 4 has no bones, so the root, which doesn't move, is its bone
        assert_eq!(get(&gltf, "/skins/0"), &json!({"inverseBindMatrices": get(&gltf, "/skins/0/inverseBindMatrices"), "joints": [1, 2, 3], "skeleton": 3}));
        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        #[rustfmt::skip]
        assert_eq!(read(&gltf, &attributes["JOINTS_0"]), [
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            2.0, 0.0, 0.0, 0.0,
        ]);
        #[rustfmt::skip]
        assert_eq!(read(&gltf, &attributes["WEIGHTS_0"]), [
            1.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            0.25, 0.75, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0,
        ]);
    }

    #[test]
    fn skinned_b3d_vertices_stay_put() {
        let model = b3d::read(&animated_b3d()).unwrap();
        assert_skin_keeps_rest_pose(&convert_ok(&model), &model);
    }

    #[test]
    fn keyframes_become_one_animation() {
        let gltf = convert_ok(&b3d::read(&animated_b3d()).unwrap());
        let animations = gltf.json["animations"].as_array().unwrap();
        assert_eq!(animations.len(), 1);
        let channels = animations[0]["channels"].as_array().unwrap();
        let paths: Vec<_> = channels.iter().map(|c| (c["target"]["node"].as_u64().unwrap(), c["target"]["path"].as_str().unwrap())).collect();
        assert_eq!(paths, [(2, "translation"), (2, "rotation")]);
        let sampler = |path: &str| {
            let channel = channels.iter().find(|c| c["target"]["path"] == path).unwrap();
            &animations[0]["samplers"][channel["sampler"].as_u64().unwrap() as usize]
        };
        // Luanti's glTF frame numbers are b3d's minus one, rather than seconds
        assert_eq!(read(&gltf, &sampler("translation")["input"]), [0.0, 10.0]);
        let input = &gltf.json["accessors"][sampler("rotation")["input"].as_u64().unwrap() as usize];
        assert_eq!((&input["min"], &input["max"]), (&json!([0.0]), &json!([10.0])));
        assert_eq!(read(&gltf, &sampler("translation")["output"]), [1.0, 0.0, 0.0, 1.0, 0.5, 0.0]);
        assert_near(&read(&gltf, &sampler("rotation")["output"]), &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -0.38268343, 0.9238795]);
    }

    /// an x model with a skin whose offsets match its frames, a rigid mesh, and an animated matrix frame
    const ANIMATED_X: &str = "xof 0303txt 0032\n\
        Frame Root {\n FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 1,2,3,1;; }\n\
          Frame Arm { FrameTransformMatrix { 0,1,0,0, -1,0,0,0, 0,0,1,0, 0,1,0,1;; } }\n\
          Mesh { 4; 0;0;0;, 1;0;0;, 1;1;0;, 5;5;5;; 2; 3;0,1,2;, 3;0,2,3;;\n\
            SkinWeights { \"Arm\"; 2; 1, 2; 1.0, 0.5; 0,-1,0,0, 1,0,0,0, 0,0,1,0, -3,1,-3,1;; }\n\
            SkinWeights { \"Root\"; 1; 2; 0.5; 1,0,0,0, 0,1,0,0, 0,0,1,0, -1,-2,-3,1;; } }\n\
        }\n\
        Frame Prop { FrameTransformMatrix { 2,0,0,0, 0,2,0,0, 0,0,2,0, 0,0,0,1;; } Mesh { 3; 0;0;0;, 1;0;0;, 1;1;0;; 1; 3;0,1,2;; } }\n\
        AnimationSet { Animation { { Arm } AnimationKey { 0; 1; 4; 4; 1,0,0,0;;; } } }\n";

    #[test]
    fn skinned_x_vertices_stay_put() {
        let (model, _) = x::read(ANIMATED_X.as_bytes()).unwrap();
        let gltf = convert_ok(&model);
        assert_skin_keeps_rest_pose(&gltf, &model);
        // the file's offsets are the inverse bind matrices
        let inverse_binds = read(&gltf, get(&gltf, "/skins/0/inverseBindMatrices"));
        // Root's, then Arm's, column-major
        assert_eq!((&inverse_binds[12..16], &inverse_binds[28..32]), (&[-1.0, -2.0, 3.0, 1.0][..], &[-3.0, 1.0, 3.0, 1.0][..]));
    }

    #[test]
    fn sample_robot_keeps_its_rest_pose_and_is_up_to_date() {
        let (model, warnings) = x::read(include_bytes!("../../samplemods/xtest/models/xtest_robot.x")).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let gltf = convert_ok(&model);
        assert_skin_keeps_rest_pose(&gltf, &model);
        assert_eq!(gltf.json["animations"][0]["channels"].as_array().unwrap().len(), 3);
        assert!(gltf.gltf.to_glb() == include_bytes!("../../samplemods/xtest/models/xtest_robot.glb"), "run `flint convert` on it again");
    }

    #[test]
    fn rigid_meshes_are_on_their_joint() {
        let (model, _) = x::read(ANIMATED_X.as_bytes()).unwrap();
        let gltf = convert_ok(&model);
        let prop = gltf.json["nodes"].as_array().unwrap().iter().find(|n| n.get("name") == Some(&json!("Prop"))).unwrap();
        assert_eq!(floats(&prop["matrix"]), [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let mesh = prop["mesh"].as_u64().unwrap() as usize;
        assert!(gltf.json["meshes"][mesh]["primitives"][0]["attributes"].get("JOINTS_0").is_none());
    }

    #[test]
    fn animated_matrix_joints_start_from_nothing() {
        let (model, _) = x::read(ANIMATED_X.as_bytes()).unwrap();
        let gltf = convert_ok(&model);
        let arm = 1;
        // its rest pose, turned 90 degrees around Z, which mirroring along Z doesn't change
        let node = get(&gltf, &format!("/nodes/{arm}"));
        assert!(node.get("matrix").is_none());
        assert_near(&floats(&node["translation"]), &[0.0, 1.0, 0.0]);
        assert_near(&floats(&node["rotation"]), &[0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2]);
        // Luanti ignores it when animating: what the keyframes don't set is nothing
        let animation = &gltf.json["animations"][0];
        let output = |path: &str| {
            let channel = animation["channels"].as_array().unwrap().iter().find(|c| c["target"] == json!({"node": arm, "path": path})).unwrap();
            read(&gltf, &animation["samplers"][channel["sampler"].as_u64().unwrap() as usize]["output"])
        };
        assert_eq!(output("translation"), [0.0, 0.0, 0.0]);
        assert_eq!(output("scale"), [1.0, 1.0, 1.0]);
        // the keyframe, which Luanti negates
        assert_eq!(output("rotation"), [0.0, 0.0, 0.0, -1.0]);
    }

    #[test]
    fn weights_are_merged_and_normalized() {
        let mut vertex = Vertex::default();
        for (joint, weight) in [(0, 1.0), (0, 1.0), (1, 2.0)] {
            vertex.add_weight(joint, weight);
        }
        let joint = |name: &str| Node { name: Some(name.into()), parent: None, transform: Transform::NONE, keys: None, inverse_bind: None };
        let model = Builder {
            nodes: vec![joint("a"), joint("b")],
            primitives: vec![Primitive { vertices: vec![vertex; 3], triangles: vec![[0, 1, 2]], material: Material::default(), ..Primitive::default() }],
        };
        let gltf = convert_ok(&model);
        let attributes = get(&gltf, "/meshes/0/primitives/0/attributes");
        assert_eq!(read(&gltf, &attributes["JOINTS_0"])[..4], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(read(&gltf, &attributes["WEIGHTS_0"])[..4], [0.5, 0.5, 0.0, 0.0]);
        // no vertex lacks weights, so the root isn't a bone
        assert_eq!(get(&gltf, "/skins/0/joints"), &json!([0, 1]));
    }

    #[test]
    fn errors() {
        assert_eq!(Builder::default().finish().err().unwrap(), "the model is empty");
        let looped = Node { name: Some("a".into()), parent: Some(0), transform: Transform::NONE, keys: Some(Keys::default()), inverse_bind: None };
        assert_eq!(Builder { nodes: vec![looped], primitives: Vec::new() }.finish().err().unwrap(), "joint `a` is its own parent");
    }

    #[test]
    fn gltf_embeds_the_buffer() {
        let gltf = convert_ok(&b3d::read(&static_b3d()).unwrap());
        let json: Value = serde_json::from_slice(&gltf.gltf.to_gltf()).unwrap();
        let uri = json["buffers"][0]["uri"].as_str().unwrap();
        assert_eq!(uri.strip_prefix("data:application/octet-stream;base64,").unwrap(), base64(&gltf.buffer));
        assert_eq!(json["buffers"][0]["byteLength"], gltf.buffer.len());
        assert_eq!([base64(b"Man"), base64(b"Ma"), base64(b"M"), base64(b"")], ["TWFu", "TWE=", "TQ==", ""]);
    }

    #[test]
    fn glb_has_json_and_binary_chunks() {
        let gltf = convert_ok(&b3d::read(&animated_b3d()).unwrap());
        let glb = gltf.gltf.to_glb();
        let u32_at = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!((u32_at(4), u32_at(8)), (2, glb.len()));
        let json_len = u32_at(12);
        assert_eq!(&glb[16..20], b"JSON");
        let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json, gltf.json);
        let bin = 20 + json_len;
        assert_eq!(&glb[bin + 4..bin + 8], b"BIN\0");
        assert_eq!(glb[bin + 8..bin + 8 + gltf.buffer.len()], gltf.buffer);
        assert_eq!((json_len % 4, u32_at(bin) % 4), (0, 0));
    }

    #[test]
    fn cleanup_drops_frames_like_luanti() {
        let mut keys = Keys { position: vec![(0.0, [0.0; 3]), (5.0, [1.0; 3]), (3.0, [2.0; 3]), (5.0, [3.0; 3]), (9.0, [4.0; 3])], ..Keys::default() };
        keys.cleanup();
        assert_eq!(keys.position, [(0.0, [0.0; 3]), (5.0, [1.0; 3]), (9.0, [4.0; 3])]);
        let mut keys = Keys { scale: vec![(0.0, [1.0; 3]), (1.0, [1.0; 3]), (2.0, [1.0; 3]), (3.0, [2.0; 3])], ..Keys::default() };
        keys.cleanup();
        assert_eq!(keys.scale, [(0.0, [1.0; 3]), (2.0, [1.0; 3]), (3.0, [2.0; 3])]);
    }

    #[test]
    fn keeps_the_4_strongest_weights() {
        let mut v = Vertex::default();
        for (node, weight) in [(1, 0.5), (2, 0.1), (3, 0.3), (4, 0.2), (5, 0.4), (6, 0.05)] {
            v.add_weight(node, weight);
        }
        let mut kept: Vec<_> = v.weights.to_vec();
        kept.sort_by_key(|&(node, _)| node);
        assert_eq!(kept, [(1, 0.5), (3, 0.3), (4, 0.2), (5, 0.4)]);
    }

    #[test]
    fn decomposes_transforms() {
        let (t, r, s) = ([1.0, -2.0, 3.0], rotation([0.1, 0.2, 0.3, 0.9]), [2.0, 0.5, 1.5]);
        let (dt, dr, ds) = decompose(&trs(t, r, s)).unwrap();
        assert_eq!(dt, t);
        assert!(dr.iter().zip(r).all(|(a, b)| (a - b).abs() < 1e-5), "{dr:?} {r:?}");
        assert!(ds.iter().zip(s).all(|(a, b)| (a - b).abs() < 1e-5), "{ds:?}");
        let mirrored = trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [-1.0, 1.0, 1.0]);
        assert_eq!(decompose(&mirrored).unwrap().2, [-1.0, 1.0, 1.0]);
        let mut sheared = IDENTITY;
        sheared[0][1] = 1.0;
        assert_eq!(decompose(&sheared), None);
    }

    #[test]
    fn inverse_undoes_transforms() {
        let m = trs([1.0, -2.0, 3.0], rotation([0.1, 0.2, 0.3, 0.9]), [2.0, 0.5, 1.5]);
        let product = mul(&m, &inverse(&m).unwrap());
        assert!(product.concat().iter().zip(IDENTITY.concat()).all(|(a, b)| (a - b).abs() < 1e-9));
        assert!(inverse(&trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 1.0])).is_none());
    }

    #[test]
    fn coordinates_are_mirrored_along_z() {
        assert_eq!(vector([1.0, 2.0, 3.0]), [1.0, 2.0, -3.0]);
        assert_eq!(rotation([0.1, 0.2, 0.3, 0.9]).map(|f| (f * 1e4).round()), [1026.0, 2052.0, -3078.0, 9234.0]);
        assert_eq!(rotation([0.0; 4]), [0.0, 0.0, 0.0, 1.0]);
        // a translation by (1, 2, 3), stored column-major
        let m = matrix([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0]);
        assert_eq!([m[0][3], m[1][3], m[2][3]], [1.0, 2.0, -3.0]);
    }

    #[test]
    fn output_is_valid_gltf() {
        let model = b3d::read(&animated_b3d()).unwrap();
        let gltf = convert_ok(&model).gltf;
        let mut errors = Vec::new();
        json::validation::Validate::validate(&gltf.root, &gltf.root, json::Path::new, &mut |path, error| errors.push(format!("{}: {error}", path())));
        // glTF doesn't require textures to have an image, Luanti gets them from the mod, but gltf-json does
        errors.retain(|e| !e.ends_with(".source: Missing data"));
        assert!(errors.is_empty(), "{errors:?}");
    }
}
