//! `.x`: DirectX models, read like Luanti's `CXMeshFileLoader.cpp`, quirks included, as that's how mods' models look
//!
//! only the text format, which Luanti mods use

use super::build::{Builder, Keys, Material, Node, Primitive, Transform, Vertex, Warning, decompose, matrix, rotation, vector};

pub fn read(data: &[u8]) -> Result<(Builder, Vec<Warning>), String> {
    if data.len() < 16 || !data.starts_with(b"xof ") {
        return Err("not an x file".to_string());
    }
    match &data[8..12] {
        b"txt " => {}
        b"bin " => return Err("binary x files aren't supported, export it as text".to_string()),
        _ => return Err("compressed x files aren't supported, and Luanti can't load them".to_string()),
    }
    if !matches!(&data[12..16], b"0032" | b"0064") {
        return Err("Luanti can't load x files with this float size".to_string());
    }
    let mut parser = Parser { text: data, pos: 16, model: Builder::default(), meshes: Vec::new(), warnings: Vec::new() };
    parser.read_until_end_of_line();
    while parser.data_object()? {}
    parser.load();
    Ok((parser.model, parser.warnings))
}

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
    model: Builder,
    meshes: Vec<XMesh>,
    warnings: Vec<Warning>,
}

#[derive(Default)]
struct XMesh {
    vertices: Vec<Vertex>,
    /// 3 per triangle, polygons are split into them
    indices: Vec<usize>,
    /// of each face in the file
    index_count_per_face: Vec<usize>,
    materials: Vec<Material>,
    /// of each triangle
    face_materials: Vec<usize>,
    has_normals: bool,
    has_colors: bool,
    has_uvs: bool,
    /// (joint, vertex, weight)
    weights: Vec<(usize, usize, f32)>,
    joint: Option<usize>,
    has_skinning: bool,
}

/// `Ok(false)` stops reading without an error, like Luanti does in places
type Step = Result<bool, String>;

impl Parser<'_> {
    fn error<T>(&self, message: &str) -> Result<T, String> {
        let line = self.text[..self.pos.min(self.text.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        Err(format!("line {line}: {message}"))
    }

    fn eof(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn at(&self, offset: usize) -> u8 {
        self.text.get(self.pos + offset).copied().unwrap_or(0)
    }

    fn read_until_end_of_line(&mut self) {
        while !self.eof() {
            let c = self.at(0);
            self.pos += 1;
            if c == b'\n' || c == b'\r' {
                return;
            }
        }
    }

    fn is_comment(&self) -> bool {
        (self.at(0) == b'/' && self.at(1) == b'/') || self.at(0) == b'#'
    }

    fn skip_whitespace(&mut self) {
        loop {
            while !self.eof() && is_space(self.at(0)) {
                self.pos += 1;
            }
            if self.eof() || !self.is_comment() {
                return;
            }
            self.read_until_end_of_line();
        }
    }

    fn token(&mut self) -> String {
        self.skip_whitespace();
        let mut token = String::new();
        while !self.eof() && !is_space(self.at(0)) {
            let c = self.at(0);
            if matches!(c, b';' | b'}' | b'{' | b',') {
                if token.is_empty() {
                    token.push(c as char);
                    self.pos += 1;
                }
                break;
            }
            token.push(c as char);
            self.pos += 1;
        }
        token
    }

    /// skips to where a number could start, which skips anything else
    fn skip_to_number(&mut self) {
        while !self.eof() && !matches!(self.at(0), b'-' | b'.' | b'0'..=b'9') {
            if self.is_comment() {
                self.read_until_end_of_line();
            } else {
                self.pos += 1;
            }
        }
    }

    /// like `strtoul`: nothing is read if there's no number
    fn int(&mut self) -> usize {
        self.skip_to_number();
        let start = self.pos;
        let negative = self.at(0) == b'-';
        let digits_start = start + negative as usize;
        let mut end = digits_start;
        while self.text.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == digits_start {
            return 0;
        }
        self.pos = end;
        let value = std::str::from_utf8(&self.text[digits_start..end]).unwrap().parse::<u64>().unwrap_or(u64::MAX) as u32;
        (if negative { value.wrapping_neg() } else { value }) as usize
    }

    /// like `strtod`: nothing is read if there's no number
    fn float(&mut self) -> f32 {
        self.skip_to_number();
        let text = self.text;
        let digits = |mut i: usize| {
            let start = i;
            while text.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            (i, i > start)
        };
        let mut end = self.pos + (self.at(0) == b'-') as usize;
        let (after, mut any) = digits(end);
        end = after;
        if text.get(end) == Some(&b'.') {
            let (after, fraction) = digits(end + 1);
            if any || fraction {
                end = after;
                any = true;
            }
        }
        if !any {
            return 0.0;
        }
        if matches!(text.get(end), Some(b'e' | b'E')) {
            let sign = matches!(text.get(end + 1), Some(b'+' | b'-')) as usize;
            if let (after, true) = digits(end + 1 + sign) {
                end = after;
            }
        }
        let number = std::str::from_utf8(&text[self.pos..end]).unwrap().parse::<f64>().unwrap_or(0.0);
        self.pos = end;
        number as f32
    }

    fn floats<const N: usize>(&mut self) -> [f32; N] {
        std::array::from_fn(|_| self.float())
    }

    /// on failure, Luanti steps back one character, whatever it read
    fn semicolon(&mut self) -> bool {
        if self.token() == ";" {
            return true;
        }
        self.pos = self.pos.saturating_sub(1);
        false
    }

    fn two_semicolons(&mut self) -> bool {
        self.semicolon() && self.semicolon()
    }

    fn closing_brace(&mut self) -> bool {
        self.token() == "}"
    }

    /// the object's name, if it has one, and its opening brace
    fn head(&mut self) -> Result<String, String> {
        let token = self.token();
        if token == "{" {
            return Ok(String::new());
        }
        if self.token() != "{" {
            return self.error("expected `{`");
        }
        Ok(token)
    }

    fn string(&mut self) -> Option<String> {
        self.skip_whitespace();
        if self.at(0) != b'"' {
            return None;
        }
        self.pos += 1;
        let start = self.pos;
        while !self.eof() && self.at(0) != b'"' {
            self.pos += 1;
        }
        let string = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
        if self.at(0) != b'"' || self.at(1) != b';' {
            return None;
        }
        self.pos += 2;
        Some(string)
    }

    fn matrix(&mut self) -> [f32; 16] {
        let numbers = self.floats();
        self.semicolon();
        numbers
    }

    fn unknown_object(&mut self) -> Step {
        loop {
            match self.token().as_str() {
                "" => return Ok(false),
                "{" => break,
                _ => {}
            }
        }
        let mut depth = 1;
        while depth > 0 {
            match self.token().as_str() {
                "" => return Ok(false),
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
        }
        Ok(true)
    }

    fn joint_named(&self, name: &str) -> Option<usize> {
        self.model.nodes.iter().position(|j| j.name.as_deref() == Some(name))
    }

    fn add_joint(&mut self, parent: Option<usize>, name: String) -> usize {
        self.model.nodes.push(Node { name: Some(name), parent, transform: Transform::NONE, keys: None, inverse_bind: None });
        self.model.nodes.len() - 1
    }

    fn data_object(&mut self) -> Step {
        match self.token().as_str() {
            "" => Ok(false),
            "template" => self.template(),
            "Frame" => self.frame(None),
            "Mesh" => self.mesh(None),
            "AnimationSet" => self.animation_set(),
            "AnimTicksPerSecond" => self.ticks_per_second(),
            "}" => Ok(true),
            _ => self.unknown_object(),
        }
    }

    fn template(&mut self) -> Step {
        self.head()?;
        // GUID
        self.token();
        loop {
            match self.token().as_str() {
                "}" => return Ok(true),
                "" => return Ok(false),
                _ => {}
            }
        }
    }

    fn frame(&mut self, parent: Option<usize>) -> Step {
        let name = self.head()?;
        let joint = match self.joint_named(&name).filter(|_| !name.is_empty()) {
            Some(joint) => {
                self.model.nodes[joint].parent = parent;
                joint
            }
            None => self.add_joint(parent, name),
        };
        loop {
            let ok = match self.token().as_str() {
                "" => return self.error("unexpected end of file in Frame"),
                "}" => return Ok(true),
                "Frame" => self.frame(Some(joint))?,
                "FrameTransformMatrix" => {
                    self.head()?;
                    let stored = self.matrix();
                    self.semicolon();
                    if !self.closing_brace() {
                        return self.error("expected `}` after FrameTransformMatrix");
                    }
                    self.model.nodes[joint].transform = Transform::Matrix(matrix(stored));
                    true
                }
                "Mesh" => self.mesh(Some(joint))?,
                _ => self.unknown_object()?,
            };
            if !ok {
                return Ok(false);
            }
        }
    }

    fn mesh(&mut self, joint: Option<usize>) -> Step {
        self.head()?;
        let mut mesh = XMesh { joint, ..XMesh::default() };
        let count = self.int();
        mesh.vertices = (0..count).map(|_| Vertex { pos: vector(self.floats()), ..Vertex::default() }).collect();
        self.two_semicolons();

        let faces = self.int();
        if faces > 1 << 28 {
            return self.error("too many faces");
        }
        for _ in 0..faces {
            let corners = self.int();
            if corners < 3 {
                return self.error("a face has less than 3 corners");
            }
            if mesh.indices.len() + (corners - 2) * 3 > 1 << 28 {
                return self.error("too many faces");
            }
            let polygon: Vec<usize> = (0..corners).map(|_| self.int()).collect();
            // split into triangles around the first corner
            for i in 1..corners - 1 {
                mesh.indices.extend([polygon[0], polygon[i], polygon[i + 1]]);
            }
            mesh.index_count_per_face.push((corners - 2) * 3);
        }
        if mesh.indices.iter().any(|&i| i >= mesh.vertices.len()) {
            return self.error("a face uses a vertex that doesn't exist");
        }
        self.two_semicolons();

        let ok = loop {
            let ok = match self.token().as_str() {
                "" => return self.error("unexpected end of file in Mesh"),
                "}" => break true,
                "MeshNormals" => self.normals(&mut mesh)?,
                "MeshTextureCoords" => self.texture_coords(&mut mesh)?,
                "MeshVertexColors" => self.vertex_colors(&mut mesh)?,
                "MeshMaterialList" => self.material_list(&mut mesh)?,
                "XSkinMeshHeader" => {
                    self.head()?;
                    for _ in 0..3 {
                        self.int();
                    }
                    self.token();
                    if !self.closing_brace() {
                        return self.error("expected `}` after XSkinMeshHeader");
                    }
                    true
                }
                "SkinWeights" => self.skin_weights(&mut mesh)?,
                name @ ("DeclData" | "FVFData") => {
                    let warning = Warning { message: format!("{name} isn't supported, so normals or texture coordinates may be missing"), changes_model: true };
                    if !self.warnings.contains(&warning) {
                        self.warnings.push(warning);
                    }
                    self.unknown_object()?
                }
                _ => self.unknown_object()?,
            };
            if !ok {
                break false;
            }
        };
        // Luanti keeps meshes it stopped reading in
        self.meshes.push(mesh);
        Ok(ok)
    }

    fn normals(&mut self, mesh: &mut XMesh) -> Step {
        self.head()?;
        let count = self.int();
        let normals: Vec<[f32; 3]> = (0..count).map(|_| vector(self.floats())).collect();
        self.two_semicolons();
        let faces = self.int();
        if faces > mesh.index_count_per_face.len() {
            return self.error("more faces have normals than there are");
        }
        // each corner of each triangle sets its vertex's normal, so the last one wins
        let mut corner = 0;
        for face in 0..faces {
            let corners = self.int();
            if corners.wrapping_sub(2).wrapping_mul(3) != mesh.index_count_per_face[face] {
                return self.error("a face's normals don't match its corners");
            }
            let polygon: Vec<usize> = (0..corners).map(|_| self.int()).collect();
            for i in 1..corners - 1 {
                for normal in [polygon[0], polygon[i], polygon[i + 1]] {
                    let Some(&vertex) = mesh.indices.get(corner) else { continue };
                    corner += 1;
                    if let Some(&normal) = normals.get(normal) {
                        mesh.vertices[vertex].normal = normal;
                    }
                }
            }
        }
        self.two_semicolons();
        if !self.closing_brace() {
            return self.error("expected `}` after MeshNormals");
        }
        mesh.has_normals = true;
        Ok(true)
    }

    fn texture_coords(&mut self, mesh: &mut XMesh) -> Step {
        self.head()?;
        let count = self.int();
        if count > mesh.vertices.len() {
            return self.error("more texture coordinates than vertices");
        }
        for vertex in &mut mesh.vertices[..count] {
            vertex.uv[0] = self.floats();
        }
        self.two_semicolons();
        if !self.closing_brace() {
            return self.error("expected `}` after MeshTextureCoords");
        }
        mesh.has_uvs = true;
        Ok(true)
    }

    fn vertex_colors(&mut self, mesh: &mut XMesh) -> Step {
        self.head()?;
        mesh.has_colors = true;
        for _ in 0..self.int() {
            let index = self.int();
            if index >= mesh.vertices.len() {
                return self.error("a vertex color is for a vertex that doesn't exist");
            }
            mesh.vertices[index].color = self.floats();
            self.semicolon();
            self.semicolon();
        }
        self.semicolon();
        Ok(self.closing_brace())
    }

    fn material_list(&mut self, mesh: &mut XMesh) -> Step {
        self.head()?;
        let count = self.int().clamp(1, 30000);
        mesh.materials = vec![Material::default(); count];
        let listed = self.int();
        let mut material = 0;
        for face in 0..mesh.index_count_per_face.len() {
            // faces past the listed ones have the last one's material
            if face < listed {
                material = self.int();
            }
            if material >= count {
                return self.error("a face uses a material that doesn't exist");
            }
            mesh.face_materials.extend(std::iter::repeat_n(material, mesh.index_count_per_face[face] / 3));
        }
        if self.at(0) == b';' {
            self.pos += 1;
        }

        // Luanti adds a material for each one in the list, after the ones the faces use, which shifts later meshes'
        // textures. the first ones' names are kept for the ones the faces use
        let mut listed_materials = 0;
        loop {
            match self.token().as_str() {
                "" => return self.error("unexpected end of file in MeshMaterialList"),
                "}" => return Ok(true),
                "{" => {
                    // a reference to a material elsewhere
                    self.token();
                    self.token();
                }
                "Material" => {
                    let start = self.pos;
                    if !self.unknown_object()? {
                        return Ok(false);
                    }
                    let info = material_info(&self.text[start..self.pos]);
                    if let Some(used) = mesh.materials.get_mut(listed_materials) {
                        *used = info.clone();
                    }
                    listed_materials += 1;
                    mesh.materials.push(info);
                }
                ";" => {}
                _ => {
                    if !self.unknown_object()? {
                        return Ok(false);
                    }
                }
            }
        }
    }

    fn skin_weights(&mut self, mesh: &mut XMesh) -> Step {
        self.head()?;
        let Some(name) = self.string() else {
            return self.error("expected a quoted bone name in SkinWeights");
        };
        mesh.has_skinning = true;
        let joint = self.joint_named(&name).unwrap_or_else(|| self.add_joint(None, name));
        let count = self.int();
        let vertices: Vec<usize> = (0..count).map(|_| self.int()).collect();
        for vertex in vertices {
            mesh.weights.push((joint, vertex, self.float()));
        }
        // the inverse of the bone's rest pose, relative to the model
        self.model.nodes[joint].inverse_bind = Some(matrix(self.matrix()));
        self.semicolon();
        if !self.closing_brace() {
            return self.error("expected `}` after SkinWeights");
        }
        Ok(true)
    }

    fn animation_set(&mut self) -> Step {
        self.head()?;
        loop {
            let ok = match self.token().as_str() {
                "" => return self.error("unexpected end of file in AnimationSet"),
                "}" => return Ok(true),
                "Animation" => self.animation()?,
                _ => self.unknown_object()?,
            };
            if !ok {
                return Ok(false);
            }
        }
    }

    fn ticks_per_second(&mut self) -> Step {
        self.head()?;
        // unused, Luanti's frame numbers are the ticks
        self.int();
        if !self.semicolon() || !self.closing_brace() {
            return self.error("expected `;}` after AnimTicksPerSecond");
        }
        Ok(true)
    }

    fn animation(&mut self) -> Step {
        self.head()?;
        let mut keys = Keys::default();
        let mut frame = String::new();
        loop {
            match self.token().as_str() {
                "" => return self.error("unexpected end of file in Animation"),
                "}" => break,
                "AnimationKey" => {
                    if !self.animation_key(&mut keys)? {
                        return Ok(false);
                    }
                }
                "AnimationOptions" => {
                    if !self.unknown_object()? {
                        return Ok(false);
                    }
                }
                "{" => {
                    frame = self.token();
                    if !self.closing_brace() {
                        return self.error("expected `}` after the animated frame's name");
                    }
                }
                _ => {
                    if !self.unknown_object()? {
                        return self.error("unexpected end of file in Animation");
                    }
                }
            }
        }
        if !frame.is_empty() {
            let joint = self.joint_named(&frame).unwrap_or_else(|| self.add_joint(None, frame));
            let existing = self.model.nodes[joint].keys.get_or_insert_default();
            existing.position.extend(keys.position);
            existing.rotation.extend(keys.rotation);
            existing.scale.extend(keys.scale);
        }
        Ok(true)
    }

    fn animation_key(&mut self, keys: &mut Keys) -> Step {
        self.head()?;
        let kind = self.int();
        if kind > 4 {
            return self.error("unknown kind of AnimationKey");
        }
        let count = self.int();
        if count == 0 {
            self.semicolon();
        }
        for _ in 0..count {
            let frame = self.int() as u32 as f32;
            let numbers = self.int();
            match kind {
                0 => {
                    if numbers != 4 {
                        return self.error("expected 4 numbers for a rotation");
                    }
                    let [w, x, y, z] = self.floats::<4>().map(|c| -c);
                    self.two_semicolons();
                    keys.rotation.push((frame, rotation([x, y, z, w])));
                }
                1 | 2 => {
                    if numbers != 3 {
                        return self.error("expected 3 numbers for a scale or position");
                    }
                    let v = self.floats();
                    self.two_semicolons();
                    if kind == 2 {
                        keys.position.push((frame, vector(v)));
                    } else {
                        keys.scale.push((frame, v));
                    }
                }
                _ => {
                    if numbers != 16 {
                        return self.error("expected 16 numbers for a matrix");
                    }
                    let m = matrix(self.matrix());
                    self.semicolon();
                    // Luanti takes its rotation and position, and drops its scale
                    let translation = [m[0][3], m[1][3], m[2][3]].map(|t| t as f32);
                    match decompose(&m) {
                        Some((_, r, s)) => {
                            if s.iter().any(|&s| (s - 1.0).abs() > 1e-4) {
                                self.warn_change("matrix keyframes with a scale aren't converted exactly");
                            }
                            keys.rotation.push((frame, r));
                        }
                        None => self.warn_change("matrix keyframes that aren't a rotation and position aren't converted"),
                    }
                    keys.position.push((frame, translation));
                }
            }
        }
        if !self.semicolon() {
            self.pos = self.pos.saturating_sub(1);
        }
        if !self.closing_brace() {
            return self.error("expected `}` after AnimationKey");
        }
        Ok(true)
    }

    fn warn_change(&mut self, message: &str) {
        let warning = Warning { message: message.to_string(), changes_model: true };
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// makes buffers of the meshes like Luanti: one for each material of each mesh
    fn load(&mut self) {
        for joint in &mut self.model.nodes {
            if let Some(keys) = &mut joint.keys {
                keys.cleanup();
            }
        }
        for mut mesh in std::mem::take(&mut self.meshes) {
            if mesh.materials.is_empty() {
                mesh.materials.push(Material::default());
            }
            let offset = self.model.primitives.len();
            for material in &mesh.materials {
                self.model.primitives.push(Primitive {
                    has_normals: mesh.has_normals,
                    has_colors: mesh.has_colors,
                    uv_sets: mesh.has_uvs as usize,
                    // ones with weights follow their bones instead
                    node: if mesh.has_skinning { None } else { mesh.joint },
                    material: material.clone(),
                    ..Primitive::default()
                });
            }
            if mesh.face_materials.is_empty() {
                mesh.face_materials = vec![0; mesh.indices.len() / 3];
            }

            // a vertex used by several materials is copied for each later face, and only the first gets weights
            let mut material_of: Vec<Option<usize>> = vec![None; mesh.vertices.len()];
            for (triangle, &material) in mesh.face_materials.iter().enumerate() {
                for corner in triangle * 3..triangle * 3 + 3 {
                    let vertex = mesh.indices[corner];
                    if material_of[vertex].is_some_and(|m| m != material) {
                        mesh.vertices.push(mesh.vertices[vertex]);
                        material_of.push(None);
                        mesh.indices[corner] = mesh.vertices.len() - 1;
                    }
                    material_of[mesh.indices[corner]] = Some(material);
                }
            }
            let mut index_in_buffer = vec![0; mesh.vertices.len()];
            for (vertex, material) in material_of.iter().enumerate() {
                if let Some(material) = material {
                    let buffer = &mut self.model.primitives[offset + material];
                    index_in_buffer[vertex] = buffer.vertices.len();
                    buffer.vertices.push(mesh.vertices[vertex]);
                }
            }
            for (triangle, &material) in mesh.face_materials.iter().enumerate() {
                let corners = std::array::from_fn(|i| index_in_buffer[mesh.indices[triangle * 3 + i]]);
                self.model.primitives[offset + material].triangles.push(corners);
            }
            for &(joint, vertex, weight) in &mesh.weights {
                if let Some(Some(material)) = material_of.get(vertex) {
                    self.model.primitives[offset + material].vertices[index_in_buffer[vertex]].add_weight(joint, weight);
                }
            }
        }
    }
}

/// like `core::isspace`
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// the name and texture file of a `Material` object, from its text after `Material`
fn material_info(text: &[u8]) -> Material {
    let text = String::from_utf8_lossy(text);
    let name = text.split('{').next().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
    let texture = text.split_once("TextureFilename").and_then(|(_, rest)| rest.split('"').nth(1)).map(str::to_owned);
    Material { name, texture, ..Material::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_ok(text: &str) -> Builder {
        let (model, warnings) = read(format!("xof 0303txt 0032\n{text}").as_bytes()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        model
    }

    /// a square of 2 triangles, as a quad
    const SQUARE: &str = "Mesh square {\n 4;\n 0;0;0;, 1;0;0;, 1;1;0;, 0;1;1;;\n 1;\n 4;0,1,2,3;;\n";

    #[test]
    fn rejects_unsupported_files() {
        assert!(read(b"nope").is_err());
        assert!(read(b"xof 0303bin 0032").err().unwrap().contains("binary"));
        assert!(read(b"xof 0303tzip0032").err().unwrap().contains("compressed"));
    }

    #[test]
    fn polygons_are_split_into_triangles() {
        let model = read_ok(&format!("{SQUARE}}}\n"));
        assert_eq!(model.primitives.len(), 1);
        assert_eq!(model.primitives[0].triangles, [[0, 1, 2], [0, 2, 3]]);
        // mirrored along Z
        assert_eq!(model.primitives[0].vertices[3].pos, [0.0, 1.0, -1.0]);
        assert_eq!(model.primitives[0].node, None);
        assert!(!model.finish().unwrap().0.is_animated());
    }

    #[test]
    fn templates_comments_and_unknown_objects_are_skipped() {
        let text = format!(
            "template Mesh {{ <3D82AB44-62DA-11cf-AB39-0020AF71E433> DWORD n; [...] }}\n\
             // a comment {{\nHeader {{ 1; 0; 1; }}\n# another\n{SQUARE} VertexDuplicationIndices {{ 4; 4; 0,1,2,3;; }}\n}}\n"
        );
        assert_eq!(read_ok(&text).primitives[0].triangles.len(), 2);
    }

    #[test]
    fn normals_and_texture_coordinates() {
        let text = format!(
            "{SQUARE} MeshNormals {{ 2; 0;0;1;, 0;2;0;;\n 1; 4;0,1,0,1;; }}\n\
             MeshTextureCoords {{ 4; 0;0;, 1;0;, 1;1;, 0;1;; }}\n}}\n"
        );
        let model = read_ok(&text);
        let buffer = &model.primitives[0];
        assert!(buffer.has_normals && buffer.uv_sets == 1);
        // the quad's normals 0,1,0,1 are split like it, into 0,1,0 and 0,0,1 for vertices 0,1,2 and 0,2,3.
        // each corner sets its vertex's normal, so the last one wins
        assert_eq!(buffer.vertices[0].normal, [0.0, 0.0, -1.0]);
        assert_eq!(buffer.vertices[1].normal, [0.0, 2.0, 0.0]);
        assert_eq!(buffer.vertices[2].normal, [0.0, 0.0, -1.0]);
        assert_eq!(buffer.vertices[3].normal, [0.0, 2.0, 0.0]);
        assert_eq!(buffer.vertices[2].uv[0], [1.0, 1.0]);
    }

    #[test]
    fn each_material_is_a_buffer() {
        // two triangles of a quad: material 1, then the faces after the listed ones repeat it
        let text = "Mesh {\n 4; 0;0;0;, 1;0;0;, 1;1;0;, 0;1;0;;\n 2; 3;0,1,2;, 3;0,2,3;;\n\
             MeshMaterialList { 2; 1; 1;;\n Material red { 1;0;0;1;; 0; 0;0;0;; 0;0;0;; TextureFilename { \"red.png\"; } }\n\
             Material blue { 0;0;1;1;; 0; 0;0;0;; 0;0;0;; }\n }\n}\n\
             Mesh { 3; 0;0;0;, 1;0;0;, 1;1;0;; 1; 3;0,1,2;; }\n";
        let model = read_ok(text);
        // Luanti's 2 listed materials, then 2 more for the Material objects, then the next mesh's
        assert_eq!(model.primitives.len(), 5);
        assert!(model.primitives[0].triangles.is_empty());
        assert_eq!(model.primitives[1].triangles, [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(model.primitives[1].material.name.as_deref(), Some("blue"));
        assert_eq!(model.primitives[0].material.texture.as_deref(), Some("red.png"));
        assert_eq!(model.primitives[4].triangles, [[0, 1, 2]]);
    }

    #[test]
    fn vertices_of_several_materials_are_copied() {
        let text = "Mesh {\n 4; 0;0;0;, 1;0;0;, 1;1;0;, 0;1;0;;\n 2; 3;0,1,2;, 3;0,2,3;;\n\
             MeshMaterialList { 2; 2; 0, 1;; { a } { b } }\n\
             SkinWeights { \"bone\"; 2; 0, 3; 1.0, 0.5; 1;0;0;0;0;1;0;0;0;0;1;0;0;0;0;1;; }\n}\n";
        let model = read_ok(text);
        assert_eq!(model.primitives.len(), 2);
        assert_eq!(model.primitives[0].vertices.len(), 3);
        assert_eq!(model.primitives[1].vertices.len(), 3);
        // the first buffer's copy of vertex 0 has the weight, the second's doesn't
        assert_eq!(model.primitives[0].vertices[0].weights[0], (0, 1.0));
        assert!(model.primitives[1].vertices.iter().any(|v| v.weights[0] == (0, 0.5)));
        assert!(model.primitives[1].vertices.iter().filter(|v| v.weights[0].1 == 0.0).count() == 2);
        assert!(model.primitives.iter().all(|b| b.node.is_none()));
    }

    #[test]
    fn frames_bones_and_animations() {
        let text = format!(
            "Frame Root {{\n FrameTransformMatrix {{ 1,0,0,0, 0,1,0,0, 0,0,1,0, 1,2,3,1;; }}\n\
               Frame Arm {{ FrameTransformMatrix {{ 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,1,0,1;; }} }}\n\
               {SQUARE} SkinWeights {{ \"Arm\"; 1; 2; 1.0; 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,-1,0,1;; }}\n }}\n}}\n\
             Frame Prop {{ Mesh {{ 3; 0;0;0;, 1;0;0;, 1;1;0;; 1; 3;0,1,2;; }} }}\n\
             AnimTicksPerSecond {{ 24; }}\n\
             AnimationSet Walk {{\n Animation {{ {{ Arm }}\n\
               AnimationKey {{ 0; 2; 0; 4; 1,0,0,0;;, 10; 4; 0,0,0,1;;; }}\n\
               AnimationKey {{ 2; 1; 5; 3; 1,2,3;;; }}\n }}\n\
             Animation {{ {{ Ghost }} AnimationKey {{ 1; 1; 0; 3; 2,2,2;;; }} }}\n}}\n"
        );
        let model = read_ok(&text);
        let names: Vec<_> = model.nodes.iter().map(|j| j.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["Root", "Arm", "Prop", "Ghost"]);
        assert_eq!(model.nodes[1].parent, Some(0));
        assert_eq!(model.nodes[3].parent, None);
        let Transform::Matrix(root) = model.nodes[0].transform else { panic!("not a matrix") };
        assert_eq!([root[0][3], root[1][3], root[2][3]], [1.0, 2.0, -3.0]);
        let inverse_bind = model.nodes[1].inverse_bind.unwrap();
        assert_eq!(inverse_bind[1][3], -1.0);

        // skinned buffers don't follow their frame, others do
        assert_eq!(model.primitives[0].node, None);
        assert_eq!(model.primitives[0].vertices[2].weights[0], (1, 1.0));
        assert_eq!(model.primitives[1].node, Some(2));

        let keys = model.nodes[1].keys.as_ref().unwrap();
        // Luanti negates every number of rotations, which is the same rotation
        assert_eq!(keys.rotation, [(0.0, [0.0, 0.0, 0.0, -1.0]), (10.0, [0.0, 0.0, 1.0, -0.0])]);
        assert_eq!(keys.position, [(5.0, [1.0, 2.0, -3.0])]);
        assert_eq!(model.nodes[3].keys.as_ref().unwrap().scale, [(0.0, [2.0, 2.0, 2.0])]);
        assert!(model.finish().unwrap().0.is_animated());
    }

    #[test]
    fn matrix_keyframes_are_a_rotation_and_position() {
        let text = "Frame Bone { }\nAnimationSet { Animation { { Bone }\n\
             AnimationKey { 4; 1; 3; 16; 1,0,0,0, 0,1,0,0, 0,0,1,0, 4,5,6,1;;; }\n } }\n";
        let model = read_ok(text);
        let keys = model.nodes[0].keys.as_ref().unwrap();
        assert_eq!(keys.position, [(3.0, [4.0, 5.0, -6.0])]);
        assert_eq!(keys.rotation, [(3.0, [0.0, 0.0, 0.0, 1.0])]);
        assert!(keys.scale.is_empty());
    }

    #[test]
    fn numbers_are_read_like_luanti() {
        let mut parser = Parser { text: b"  -1.5e2;x 7 // 9\n .25 -", pos: 0, model: Builder::default(), meshes: Vec::new(), warnings: Vec::new() };
        assert_eq!(parser.float(), -150.0);
        assert_eq!(parser.int(), 7);
        assert_eq!(parser.float(), 0.25);
        assert_eq!(parser.int(), 0);
    }

    #[test]
    fn errors_have_lines() {
        let err = read(b"xof 0303txt 0032\nMesh {\n 3; 0;0;0;, 1;0;0;, 1;1;0;;\n 1; 3;0,1,5;;\n}\n").err().unwrap();
        assert_eq!(err, "line 4: a face uses a vertex that doesn't exist");
    }
}
