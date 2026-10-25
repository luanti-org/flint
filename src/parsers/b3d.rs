//! `.b3d`: Blitz3D models, ported from [modlib](https://github.com/appgurueu/modlib/blob/master/b3d.lua)
//!
//! see https://github.com/blitz-research/blitz3d/blob/master/blitz3d/loader_b3d.cpp, and Luanti's `CB3DMeshFileLoader.cpp`
//! for how Luanti reads them. ids are 0-based, rotations are `[x, y, z, w]`.

pub struct B3d {
    pub textures: Vec<Texture>,
    pub brushes: Vec<Brush>,
    pub node: Option<Node>,
}

/// only what Luanti uses
pub struct Texture {
    pub file: String,
    pub scale: [f32; 2],
}

/// only what Luanti uses
pub struct Brush {
    pub name: String,
    pub color: [f32; 4],
    pub shininess: f32,
    pub texture_ids: Vec<Option<usize>>,
}

pub struct Node {
    pub name: String,
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    pub mesh: Option<Mesh>,
    /// vertex id in the mesh this is a bone of, and its weight
    pub bone: Option<Vec<(usize, f32)>>,
    pub keys: Vec<Key>,
    pub children: Vec<Node>,
}

pub struct Mesh {
    pub brush_id: Option<usize>,
    pub vertices: Vertices,
    pub triangle_sets: Vec<Tris>,
}

pub struct Vertices {
    pub tex_coord_sets: usize,
    pub tex_coord_set_size: usize,
    pub has_normals: bool,
    pub has_colors: bool,
    pub list: Vec<Vertex>,
}

pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    /// `tex_coord_sets` sets of `tex_coord_set_size` coordinates
    pub tex_coords: Vec<f32>,
}

pub struct Tris {
    pub brush_id: Option<usize>,
    /// position among all triangle sets in the file, Luanti makes a mesh buffer of each in this order
    pub buffer: usize,
    pub triangles: Vec<[usize; 3]>,
}

pub struct Key {
    pub frame: i32,
    pub position: Option<[f32; 3]>,
    pub scale: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
}

impl B3d {
    /// whether any node has keyframes or a bone weighting vertices
    pub fn is_animated(&self) -> bool {
        fn animated(node: &Node) -> bool {
            !node.keys.is_empty() || node.bone.as_ref().is_some_and(|w| !w.is_empty()) || node.children.iter().any(animated)
        }
        self.node.as_ref().is_some_and(animated)
    }
}

pub fn parse(data: &[u8]) -> Result<B3d, String> {
    let mut reader = Reader { data, pos: 0, buffers: 0 };
    let (tag, mut chunk) = reader.chunk()?;
    if &tag != b"BB3D" {
        return Err("not a b3d file".to_string());
    }
    let version = chunk.int()?;
    if version / 100 > 2 {
        return Err(format!("unsupported version {}", version / 100));
    }
    let mut b3d = B3d { textures: Vec::new(), brushes: Vec::new(), node: None };
    while !chunk.is_empty() {
        let (tag, mut sub) = chunk.chunk()?;
        match &tag {
            b"TEXS" => {
                while !sub.is_empty() {
                    let file = sub.string()?;
                    // flags, blend, position
                    sub.bytes::<16>()?;
                    let scale = sub.floats()?;
                    // rotation
                    sub.float()?;
                    b3d.textures.push(Texture { file, scale });
                }
            }
            b"BRUS" => {
                let n_texs = sub.int()?;
                if !(0..=8).contains(&n_texs) {
                    return Err(format!("brushes have {n_texs} textures, at most 8 are allowed"));
                }
                while !sub.is_empty() {
                    let (name, color, shininess) = (sub.string()?, sub.floats()?, sub.float()?);
                    // blend, fx
                    sub.bytes::<8>()?;
                    let texture_ids = (0..n_texs).map(|_| sub.optional_id()).collect::<Result<_, _>>()?;
                    b3d.brushes.push(Brush { name, color, shininess, texture_ids });
                }
            }
            b"NODE" => b3d.node = Some(sub.node()?),
            _ => {}
        }
        chunk.buffers = sub.buffers;
    }
    Ok(b3d)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    /// triangle sets read so far
    buffers: usize,
}

impl<'a> Reader<'a> {
    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let bytes = self.data.get(self.pos..self.pos + N).ok_or("unexpected end of file")?;
        self.pos += N;
        Ok(bytes.try_into().unwrap())
    }

    fn int(&mut self) -> Result<i32, String> {
        self.bytes().map(i32::from_le_bytes)
    }

    fn id(&mut self) -> Result<usize, String> {
        let id = self.int()?;
        usize::try_from(id).map_err(|_| format!("invalid id {id}"))
    }

    fn optional_id(&mut self) -> Result<Option<usize>, String> {
        match self.int()? {
            -1 => Ok(None),
            id => usize::try_from(id).map(Some).map_err(|_| format!("invalid id {id}")),
        }
    }

    fn float(&mut self) -> Result<f32, String> {
        self.bytes().map(f32::from_le_bytes)
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut floats = [0.0; N];
        for float in &mut floats {
            *float = self.float()?;
        }
        Ok(floats)
    }

    fn string(&mut self) -> Result<String, String> {
        let rest = &self.data[self.pos..];
        let len = rest.iter().position(|&b| b == 0).ok_or("unterminated string")?;
        self.pos += len + 1;
        Ok(String::from_utf8_lossy(&rest[..len]).into_owned())
    }

    fn chunk(&mut self) -> Result<([u8; 4], Reader<'a>), String> {
        let tag = self.bytes::<4>()?;
        let len = self.int()?;
        let end = usize::try_from(len).ok().map(|len| self.pos + len).filter(|&end| end <= self.data.len());
        let Some(end) = end else {
            return Err(format!("{} chunk is longer than the file", String::from_utf8_lossy(&tag)));
        };
        let chunk = Reader { data: &self.data[..end], pos: self.pos, buffers: self.buffers };
        self.pos = end;
        Ok((tag, chunk))
    }

    // Unknown chunks are skipped, like Luanti does.
    fn node(&mut self) -> Result<Node, String> {
        let mut node = Node {
            name: self.string()?,
            position: self.floats()?,
            scale: self.floats()?,
            rotation: self.rotation()?,
            mesh: None,
            bone: None,
            keys: Vec::new(),
            children: Vec::new(),
        };
        while !self.is_empty() {
            let (tag, mut sub) = self.chunk()?;
            match &tag {
                b"MESH" | b"BONE" if node.mesh.is_some() || node.bone.is_some() => {
                    return Err(format!("node `{}` has more than one mesh or bone", node.name));
                }
                b"MESH" => node.mesh = Some(sub.mesh()?),
                b"BONE" => node.bone = Some(sub.bone()?),
                b"KEYS" => sub.keys(&mut node.keys)?,
                b"NODE" => node.children.push(sub.node()?),
                _ => {}
            }
            self.buffers = sub.buffers;
        }
        Ok(node)
    }

    fn rotation(&mut self) -> Result<[f32; 4], String> {
        let [w, x, y, z] = self.floats()?;
        Ok([x, y, z, w])
    }

    fn mesh(&mut self) -> Result<Mesh, String> {
        let brush_id = self.optional_id()?;
        let mut vertices = None;
        let mut triangle_sets = Vec::new();
        while !self.is_empty() {
            let (tag, mut sub) = self.chunk()?;
            match &tag {
                b"VRTS" if vertices.is_some() => return Err("mesh has more than one VRTS chunk".to_string()),
                b"VRTS" => vertices = Some(sub.vertices()?),
                b"TRIS" => {
                    let brush_id = sub.optional_id()?;
                    let mut triangles = Vec::new();
                    while !sub.is_empty() {
                        triangles.push([sub.id()?, sub.id()?, sub.id()?]);
                    }
                    triangle_sets.push(Tris { brush_id, buffer: self.buffers, triangles });
                    self.buffers += 1;
                }
                _ => {}
            }
        }
        let vertices = vertices.ok_or("mesh has no VRTS chunk")?;
        for tri in triangle_sets.iter().flat_map(|set| &set.triangles) {
            if let Some(id) = tri.iter().find(|&&id| id >= vertices.list.len()) {
                return Err(format!("triangle uses vertex {id}, but the mesh has {}", vertices.list.len()));
            }
        }
        Ok(Mesh { brush_id, vertices, triangle_sets })
    }

    fn vertices(&mut self) -> Result<Vertices, String> {
        let flags = self.int()?;
        let (sets, size) = (self.int()?, self.int()?);
        let (Ok(tex_coord_sets @ 0..=8), Ok(tex_coord_set_size @ 0..=4)) = (usize::try_from(sets), usize::try_from(size)) else {
            return Err(format!("{sets} sets of {size} texture coordinates, at most 8 of 4 are allowed"));
        };
        let mut vertices = Vertices {
            tex_coord_sets,
            tex_coord_set_size,
            has_normals: flags & 1 != 0,
            has_colors: flags & 2 != 0,
            list: Vec::new(),
        };
        while !self.is_empty() {
            let pos = self.floats()?;
            let normal = if vertices.has_normals { self.floats()? } else { [0.0; 3] };
            let color = if vertices.has_colors { self.floats()? } else { [1.0; 4] };
            let tex_coords = (0..tex_coord_sets * tex_coord_set_size).map(|_| self.float()).collect::<Result<_, _>>()?;
            vertices.list.push(Vertex { pos, normal, color, tex_coords });
        }
        Ok(vertices)
    }

    fn bone(&mut self) -> Result<Vec<(usize, f32)>, String> {
        let mut weights = Vec::new();
        while !self.is_empty() {
            let vertex = self.id()?;
            let weight = self.float()?;
            // Many exporters include unneeded zero weights.
            if weight > 0.0 {
                weights.push((vertex, weight));
            }
        }
        Ok(weights)
    }

    fn keys(&mut self, keys: &mut Vec<Key>) -> Result<(), String> {
        let flags = self.int()?;
        while !self.is_empty() {
            keys.push(Key {
                frame: self.int()?,
                position: if flags & 1 != 0 { Some(self.floats()?) } else { None },
                scale: if flags & 2 != 0 { Some(self.floats()?) } else { None },
                rotation: if flags & 4 != 0 { Some(self.rotation()?) } else { None },
            });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// builds b3d files for tests
    #[derive(Default)]
    pub struct Writer(pub Vec<u8>);

    impl Writer {
        pub fn int(&mut self, v: i32) -> &mut Self {
            self.0.extend(v.to_le_bytes());
            self
        }

        pub fn floats(&mut self, vs: &[f32]) -> &mut Self {
            for v in vs {
                self.0.extend(v.to_le_bytes());
            }
            self
        }

        pub fn string(&mut self, s: &str) -> &mut Self {
            self.0.extend(s.as_bytes());
            self.0.push(0);
            self
        }

        pub fn chunk(&mut self, tag: &str, body: impl FnOnce(&mut Self)) -> &mut Self {
            self.0.extend(tag.as_bytes());
            let len_at = self.0.len();
            self.0.extend([0; 4]);
            body(self);
            let len = (self.0.len() - len_at - 4) as i32;
            self.0[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
            self
        }

        /// a NODE chunk's header; `rotation` is `[x, y, z, w]`
        pub fn node_header(&mut self, name: &str, position: [f32; 3], rotation: [f32; 4]) -> &mut Self {
            let [x, y, z, w] = rotation;
            self.string(name).floats(&position).floats(&[1.0, 1.0, 1.0]).floats(&[w, x, y, z])
        }
    }

    pub fn b3d(body: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut w = Writer::default();
        w.chunk("BB3D", |w| {
            w.int(1);
            body(w);
        });
        w.0
    }

    #[test]
    fn reads_textures_brushes_and_nodes() {
        let data = b3d(|w| {
            w.chunk("TEXS", |w| {
                w.string("tex.png").int(1).int(2).floats(&[0.0, 0.0, 2.0, 1.0, 0.0]);
            });
            w.chunk("BRUS", |w| {
                w.int(1).string("brush").floats(&[1.0, 0.5, 0.25, 1.0, 0.0]).int(1).int(0).int(0);
            });
            w.chunk("NODE", |w| {
                w.node_header("root", [1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0]);
                w.chunk("NODE", |w| {
                    w.node_header("child", [0.0; 3], [0.0, 1.0, 0.0, 0.0]);
                });
            });
        });
        let b3d = parse(&data).unwrap();
        assert_eq!(b3d.textures[0].file, "tex.png");
        assert_eq!(b3d.textures[0].scale, [2.0, 1.0]);
        assert_eq!(b3d.brushes[0].name, "brush");
        assert_eq!(b3d.brushes[0].color, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(b3d.brushes[0].texture_ids, vec![Some(0)]);
        let root = b3d.node.unwrap();
        assert_eq!(root.position, [1.0, 2.0, 3.0]);
        assert_eq!(root.children[0].name, "child");
        assert_eq!(root.children[0].rotation, [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn reads_meshes_bones_and_keys() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                w.chunk("MESH", |w| {
                    w.int(-1);
                    w.chunk("VRTS", |w| {
                        w.int(1).int(1).int(2);
                        for i in 0..3 {
                            w.floats(&[i as f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.5]);
                        }
                    });
                    w.chunk("TRIS", |w| {
                        w.int(-1).int(0).int(1).int(2);
                    });
                    w.chunk("TRIS", |w| {
                        w.int(-1).int(2).int(1).int(0);
                    });
                });
                w.chunk("NODE", |w| {
                    w.node_header("bone", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                    w.chunk("BONE", |w| {
                        w.int(0).floats(&[1.0]).int(1).floats(&[0.0]);
                    });
                    w.chunk("KEYS", |w| {
                        w.int(1 | 4).int(1).floats(&[1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0]);
                    });
                    w.chunk("XTRA", |w| {
                        w.int(42);
                    });
                });
            });
        });
        let node = parse(&data).unwrap().node.unwrap();
        let mesh = node.mesh.unwrap();
        assert!(mesh.vertices.has_normals && !mesh.vertices.has_colors);
        assert_eq!(mesh.vertices.list[2].pos, [2.0, 0.0, 0.0]);
        assert_eq!(mesh.vertices.list[2].tex_coords, vec![0.5, 0.5]);
        assert_eq!(mesh.triangle_sets[1].triangles, vec![[2, 1, 0]]);
        assert_eq!(mesh.triangle_sets[1].buffer, 1);
        let bone = &node.children[0];
        assert_eq!(bone.bone, Some(vec![(0, 1.0)]), "zero weights are dropped");
        assert_eq!(bone.keys[0].frame, 1);
        assert_eq!(bone.keys[0].position, Some([1.0, 2.0, 3.0]));
        assert_eq!(bone.keys[0].scale, None);
        assert_eq!(bone.keys[0].rotation, Some([0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse(b"").is_err());
        assert!(parse(b"NOPE\0\0\0\0").is_err());
        let mut truncated = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("root", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
            });
        });
        truncated.truncate(truncated.len() - 2);
        assert!(parse(&truncated).is_err());
        let out_of_range = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                w.chunk("MESH", |w| {
                    w.int(-1);
                    w.chunk("VRTS", |w| {
                        w.int(0).int(0).int(0).floats(&[0.0; 3]);
                    });
                    w.chunk("TRIS", |w| {
                        w.int(-1).int(0).int(0).int(1);
                    });
                });
            });
        });
        assert!(parse(&out_of_range).err().unwrap().contains("vertex 1"));
    }
}
