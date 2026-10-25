//! `.b3d` to a model, like Luanti's `CB3DMeshFileLoader.cpp`

use super::build::{self, Builder, IDENTITY, Keys, Mat, Material, Primitive, Transform, Vertex, mul, rotation, trs, vector};
use crate::parsers::b3d::{self, B3d, Brush, Content, Mesh, Node};

pub fn read(data: &[u8]) -> Result<Builder, String> {
    let b3d = b3d::parse(data)?;
    let mut reader = Reader { b3d: &b3d, model: Builder::default(), vertices: Vec::new(), vertices_start: 0 };
    for node in &b3d.nodes {
        reader.node(node, None, &IDENTITY)?;
    }
    Ok(reader.model)
}

struct Reader<'a> {
    b3d: &'a B3d,
    model: Builder,
    /// every mesh's vertices, with the buffer and index they're linked to
    vertices: Vec<(Vertex, Option<(usize, usize)>)>,
    /// where the latest mesh's vertices start, which bones' vertex ids count from
    vertices_start: usize,
}

impl Reader<'_> {
    fn node(&mut self, node: &Node, parent: Option<usize>, parent_global: &Mat) -> Result<(), String> {
        let (translation, turn) = (vector(node.position), rotation(node.rotation));
        let joint = self.model.nodes.len();
        self.model.nodes.push(build::Node {
            name: Some(node.name.clone()),
            parent,
            transform: Transform::Trs { translation, rotation: turn, scale: node.scale },
            keys: None,
            inverse_bind: None,
        });
        let global = mul(parent_global, &trs(translation, turn, node.scale));

        for content in &node.contents {
            match content {
                Content::Node(child) => self.node(child, Some(joint), &global)?,
                Content::Mesh(mesh) => self.mesh(mesh, &global)?,
                Content::Bone(weights) => {
                    for &(id, weight) in weights {
                        let Some((_, link)) = self.vertices.get(id + self.vertices_start) else {
                            return Err(format!("bone `{}` weights vertex {id}, which doesn't exist", node.name));
                        };
                        // vertices no triangle uses are ignored
                        if let Some((buffer, index)) = *link
                            && weight > 0.0
                        {
                            self.model.primitives[buffer].vertices[index].add_weight(joint, weight);
                        }
                    }
                }
                // Luanti only uses a joint's first
                Content::Keys(keys) if self.model.nodes[joint].keys.is_none() => {
                    let mut converted = Keys::default();
                    for key in keys {
                        // Luanti's frames start at 0, moving those before 1 to it
                        let frame = (key.frame.max(1) - 1) as f32;
                        converted.position.extend(key.position.map(|p| (frame, vector(p))));
                        converted.rotation.extend(key.rotation.map(|r| (frame, rotation(r))));
                        converted.scale.extend(key.scale.map(|s| (frame, s)));
                    }
                    converted.cleanup();
                    self.model.nodes[joint].keys = Some(converted);
                }
                Content::Keys(_) => {}
            }
        }
        Ok(())
    }

    /// the vertices are put where `global` moves them, and each triangle set becomes a buffer
    fn mesh(&mut self, mesh: &Mesh, global: &Mat) -> Result<(), String> {
        let b3d = self.b3d;
        let mesh_brush = brush(b3d, mesh.brush_id)?;
        let vertices = &mesh.vertices;
        let size = vertices.tex_coord_set_size;
        self.vertices_start = self.vertices.len();
        for v in &vertices.list {
            let uv = |set: usize| match vertices.tex_coord_sets > set && size >= 2 {
                true => [v.tex_coords[set * size], v.tex_coords[set * size + 1]],
                false => [0.0; 2],
            };
            let pos = apply(global, vector(v.pos), 1.0);
            let vertex = Vertex { pos, normal: normalize(apply(global, vector(v.normal), 0.0)), color: v.color, uv: [uv(0), uv(1)], ..Vertex::default() };
            self.vertices.push((vertex, None));
        }

        for set in &mesh.triangle_sets {
            let brush = brush(b3d, set.brush_id)?;
            let buffer = self.model.primitives.len();
            // Luanti only uses the triangle set's brush for vertex colors and texture scale
            let texture = brush.and_then(|b| first_texture(b3d, b));
            let named = brush.or(mesh_brush);
            self.model.primitives.push(Primitive {
                has_normals: true,
                has_colors: vertices.has_colors,
                uv_sets: if size >= 2 { vertices.tex_coord_sets } else { 0 },
                material: Material {
                    name: named.map(|b| b.name.clone()),
                    texture: named.and_then(|b| first_texture(b3d, b)).map(|t| t.file.clone()),
                    color: named.map(|b| b.color),
                    shininess: named.map(|b| b.shininess),
                },
                ..Primitive::default()
            });
            for tri in &set.triangles {
                let mut indices = [0; 3];
                for (corner, &id) in tri.iter().enumerate() {
                    let (vertex, link) = &mut self.vertices[self.vertices_start + id];
                    // a vertex in another buffer is copied into this one, and only the copy gets weights
                    if link.is_some_and(|(b, _)| b != buffer) {
                        *link = None;
                    }
                    if link.is_none() {
                        let mut copy = *vertex;
                        if let Some(brush) = brush {
                            if !vertices.has_colors {
                                copy.color = brush.color;
                            } else if copy.color[3] >= 1.0 {
                                copy.color[3] = brush.color[3];
                            }
                        }
                        if let Some(texture) = texture {
                            copy.uv[0][0] *= texture.scale[0];
                            copy.uv[0][1] *= texture.scale[1];
                        }
                        let buffer_vertices = &mut self.model.primitives[buffer].vertices;
                        *link = Some((buffer, buffer_vertices.len()));
                        buffer_vertices.push(copy);
                    }
                    indices[corner] = link.unwrap().1;
                }
                self.model.primitives[buffer].triangles.push(indices);
            }
            if !vertices.has_normals {
                face_normals(&mut self.model.primitives[buffer]);
            }
        }
        Ok(())
    }
}

/// Luanti doesn't load models using brushes that don't exist
fn brush(b3d: &B3d, id: Option<usize>) -> Result<Option<&Brush>, String> {
    id.map(|id| b3d.brushes.get(id).ok_or_else(|| format!("triangles use brush {id}, which doesn't exist"))).transpose()
}

/// the texture Luanti uses: one for secondary texture coordinates is moved behind the next
fn first_texture<'a>(b3d: &'a B3d, brush: &Brush) -> Option<&'a b3d::Texture> {
    let textures = &b3d.textures;
    let mut ids: Vec<Option<usize>> = brush.texture_ids.iter().map(|id| id.filter(|&id| id < textures.len())).collect();
    ids.resize(ids.len().max(2), None);
    if ids[0].is_some_and(|id| textures[id].flags & 65536 != 0) {
        ids.swap(0, 1);
    }
    ids.into_iter().flatten().next().map(|id| &textures[id])
}

/// normals of the faces around each vertex, as Luanti makes when the file has none
fn face_normals(buffer: &mut Primitive) {
    for tri in &buffer.triangles {
        let [a, b, c] = tri.map(|i| buffer.vertices[i].pos);
        let (u, v) = (sub(b, a), sub(c, a));
        // Luanti's (b - a) x (c - a) is mirrored along Z, which flips it
        let normal = normalize([-(u[1] * v[2] - u[2] * v[1]), -(u[2] * v[0] - u[0] * v[2]), -(u[0] * v[1] - u[1] * v[0])]);
        for &i in tri {
            let n = &mut buffer.vertices[i].normal;
            *n = [n[0] + normal[0], n[1] + normal[1], n[2] + normal[2]];
        }
    }
    for vertex in &mut buffer.vertices {
        vertex.normal = normalize(vertex.normal);
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `m` applied to `v` as a point when `w` is 1, or a direction when 0
fn apply(m: &Mat, v: [f32; 3], w: f64) -> [f32; 3] {
    let v = v.map(f64::from);
    std::array::from_fn(|i| (m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + m[i][3] * w) as f32)
}

/// zero stays zero, like Luanti
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = v.iter().map(|c| c * c).sum::<f32>().sqrt();
    if len == 0.0 { v } else { v.map(|c| c / len) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::b3d::tests::{Writer, b3d};

    const NO_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    /// a MESH chunk of vertices with one set of UVs, and triangle sets with their brush
    pub(crate) fn mesh(w: &mut Writer, brush: i32, normals: bool, positions: &[[f32; 3]], sets: &[(i32, &[[i32; 3]])]) {
        w.chunk("MESH", |w| {
            w.int(brush);
            w.chunk("VRTS", |w| {
                w.int(normals as i32).int(1).int(2);
                for (i, pos) in positions.iter().enumerate() {
                    w.floats(pos);
                    if normals {
                        w.floats(&[0.0, 2.0, 0.0]);
                    }
                    w.floats(&[i as f32 / 10.0, 0.5]);
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
    fn vertices_are_put_where_their_node_is() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                // rotated 90 degrees around Y
                let half = std::f32::consts::FRAC_1_SQRT_2;
                w.node_header("quad", [1.0, 2.0, 3.0], [0.0, half, 0.0, half]);
                mesh(w, -1, true, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 2.0]], &[(-1, &[[0, 1, 2]])]);
            });
        });
        let model = read(&data).unwrap();
        assert_eq!(model.nodes[0].name.as_deref(), Some("quad"));
        let Transform::Trs { translation, .. } = model.nodes[0].transform else { panic!() };
        assert_eq!(translation, [1.0, 2.0, -3.0]);
        let positions: Vec<_> = model.primitives[0].vertices.iter().map(|v| v.pos.map(|c| (c * 1e4).round() / 1e4)).collect();
        // mirrored along Z, then turned and moved by the node
        assert_eq!(positions, [[1.0, 2.0, -3.0], [1.0, 2.0, -4.0], [-1.0, 2.0, -3.0]]);
        assert_eq!(model.primitives[0].vertices[0].normal.map(|c| c.round()), [0.0, 1.0, 0.0]);
        assert_eq!(model.primitives[0].node, None);
    }

    #[test]
    fn each_triangle_set_is_a_buffer_with_its_own_vertices() {
        let data = b3d(|w| {
            w.chunk("TEXS", |w| {
                w.string("skin.png").int(1).int(2).floats(&[0.0, 0.0, 2.0, 3.0, 0.0]);
            });
            w.chunk("BRUS", |w| {
                w.int(1);
                w.string("textured").floats(&[1.0, 0.5, 0.25, 0.5, 0.25]).int(1).int(0).int(0);
            });
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, true, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0; 3]], &[(0, &[[0, 1, 2]]), (-1, &[[1, 3, 2]])]);
            });
        });
        let model = read(&data).unwrap();
        assert_eq!(model.primitives.len(), 2);
        assert_eq!(model.primitives[0].triangles, [[0, 1, 2]]);
        assert_eq!(model.primitives[1].triangles, [[0, 1, 2]]);
        assert_eq!(model.primitives[1].vertices.iter().map(|v| v.pos).collect::<Vec<_>>(), [[1.0, 0.0, 0.0], [1.0, 1.0, -1.0], [0.0, 1.0, 0.0]]);
        // the triangle set's brush colors vertices and scales their texture coordinates
        assert_eq!(model.primitives[0].vertices[1].color, [1.0, 0.5, 0.25, 0.5]);
        assert_eq!(model.primitives[0].vertices[1].uv[0], [0.2, 1.5]);
        assert_eq!(model.primitives[1].vertices[0].uv[0], [0.1, 0.5]);
        assert_eq!(model.primitives[0].material.texture.as_deref(), Some("skin.png"));
        assert_eq!(model.primitives[1].material, Material::default());
    }

    #[test]
    fn normals_are_made_when_missing() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, false, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], &[(-1, &[[0, 1, 2]])]);
            });
        });
        let model = read(&data).unwrap();
        // the triangle faces down in Luanti, (1, 0, 0) x (0, 0, 1), which is the same in glTF
        assert_eq!(model.primitives[0].vertices[0].normal, [0.0, -1.0, 0.0]);
    }

    /// a mesh, a bone in it weighting its vertices, and that bone's keys
    fn boned(bone: &[(i32, f32)], keys: impl FnOnce(&mut Writer)) -> Vec<u8> {
        b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, -1, true, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [5.0; 3]], &[(-1, &[[0, 1, 2]]), (-1, &[[2, 1, 0]])]);
                w.chunk("NODE", |w| {
                    w.node_header("bone", [0.0; 3], NO_ROTATION);
                    w.chunk("BONE", |w| {
                        for &(id, weight) in bone {
                            w.int(id).floats(&[weight]);
                        }
                    });
                    keys(w);
                });
            });
        })
    }

    #[test]
    fn weights_go_to_the_latest_copy_of_a_vertex() {
        let model = read(&boned(&[(0, 1.0), (3, 1.0), (1, 0.0)], |_| {})).unwrap();
        // vertices are copied into the second buffer, and weights apply there
        assert_eq!(model.primitives[0].vertices[0].weights[0], (0, 0.0));
        assert!(model.primitives[1].vertices.iter().any(|v| v.weights.contains(&(1, 1.0))));
        assert!(!model.primitives[1].vertices.iter().any(|v| v.weights.iter().any(|&(_, w)| w != 0.0 && w != 1.0)));
        assert!(model.finish().unwrap().0.is_animated());
        assert!(read(&boned(&[(4, 1.0)], |_| {})).err().unwrap().contains("vertex 4, which doesn't exist"));
    }

    #[test]
    fn only_a_joints_first_keys_are_used() {
        let data = boned(&[], |w| {
            w.chunk("KEYS", |w| {
                w.int(1);
                // frames before 1 are moved to it, then out of order and repeated ones are dropped
                w.int(0).floats(&[1.0, 0.0, 0.0]).int(5).floats(&[2.0, 0.0, 0.0]).int(3).floats(&[3.0, 0.0, 0.0]);
                w.int(5).floats(&[4.0, 0.0, 0.0]).int(9).floats(&[5.0, 0.0, 1.0]);
            });
            w.chunk("KEYS", |w| {
                w.int(2).int(1).floats(&[2.0; 3]);
            });
        });
        let model = read(&data).unwrap();
        let keys = model.nodes[1].keys.as_ref().unwrap();
        assert_eq!(keys.position, [(0.0, [1.0, 0.0, 0.0]), (4.0, [2.0, 0.0, 0.0]), (8.0, [5.0, 0.0, -1.0])]);
        assert!(keys.scale.is_empty());
    }

    #[test]
    fn missing_brushes_are_errors() {
        let data = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, 2, true, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]])]);
            });
        });
        assert_eq!(read(&data).err().unwrap(), "triangles use brush 2, which doesn't exist");
    }

    #[test]
    fn secondary_texture_coordinates_texture_isnt_used() {
        let data = b3d(|w| {
            w.chunk("TEXS", |w| {
                w.string("lightmap.png").int(65536).int(2).floats(&[0.0, 0.0, 1.0, 1.0, 0.0]);
                w.string("skin.png").int(1).int(2).floats(&[0.0, 0.0, 1.0, 1.0, 0.0]);
            });
            w.chunk("BRUS", |w| {
                w.int(2).string("b").floats(&[1.0; 5]).int(1).int(0).int(0).int(1);
            });
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], NO_ROTATION);
                mesh(w, 0, true, &[[0.0; 3]; 3], &[(-1, &[[0, 1, 2]])]);
            });
        });
        assert_eq!(read(&data).unwrap().primitives[0].material.texture.as_deref(), Some("skin.png"));
    }
}
