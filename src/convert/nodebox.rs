//! obj to a node box, ported from [objtonodebox](https://github.com/regulus79/objtonodebox)
//!
//! each connected part of the model, i.e. vertices joined by faces, becomes a box around it

/// a Lua file returning a `fixed` node box, to `dofile` or copy into a node definition
pub fn convert(obj: &str, scale: f64) -> Result<String, String> {
    let Obj { points, faces } = parse(obj)?;

    // union-find of vertices sharing a face
    let mut parents: Vec<usize> = (0..points.len()).collect();
    fn root(parents: &mut [usize], mut i: usize) -> usize {
        while parents[i] != i {
            parents[i] = parents[parents[i]];
            i = parents[i];
        }
        i
    }
    let mut used = vec![false; points.len()];
    for face in &faces {
        let first = root(&mut parents, face[0]);
        for &vertex in face {
            used[vertex] = true;
            let r = root(&mut parents, vertex);
            parents[r] = first;
        }
    }

    // boxes in the order of their first vertex, skipping vertices that aren't part of a face
    let mut boxes: Vec<(usize, [f64; 6])> = Vec::new();
    for (vertex, point) in points.iter().enumerate().filter(|&(vertex, _)| used[vertex]) {
        // Luanti flips the X axis of obj models
        let p = [-point[0] * scale, point[1] * scale, point[2] * scale];
        let group = root(&mut parents, vertex);
        match boxes.iter_mut().find(|(g, _)| *g == group) {
            Some((_, b)) => {
                for axis in 0..3 {
                    b[axis] = b[axis].min(p[axis]);
                    b[axis + 3] = b[axis + 3].max(p[axis]);
                }
            }
            None => boxes.push((group, [p[0], p[1], p[2], p[0], p[1], p[2]])),
        }
    }
    if boxes.is_empty() {
        return Err("the model has no faces".to_string());
    }

    let mut lua = String::from("return {\n\ttype = \"fixed\",\n\tfixed = {\n");
    for (_, b) in &boxes {
        let numbers: Vec<String> = b.iter().map(|&n| number(n)).collect();
        lua.push_str(&format!("\t\t{{ {} }},\n", numbers.join(", ")));
    }
    lua.push_str("\t},\n}\n");
    Ok(lua)
}

struct Obj {
    points: Vec<[f64; 3]>,
    /// 0-based vertex indices
    faces: Vec<Vec<usize>>,
}

fn parse(obj: &str) -> Result<Obj, String> {
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for (index, line) in obj.lines().enumerate() {
        let err = |message: String| format!("line {}: {message}", index + 1);
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") => {
                let mut coord = || -> Result<f64, String> {
                    let word = words.next().ok_or_else(|| err("vertex has less than 3 coordinates".to_string()))?;
                    word.parse().map_err(|_| err(format!("`{word}` isn't a number")))
                };
                points.push([coord()?, coord()?, coord()?]);
            }
            Some("f") => {
                let face = words
                    .map(|word| {
                        // `v/vt/vn`, only the vertex is needed. negative indices count back from the latest vertex
                        let v = word.split('/').next().unwrap_or_default();
                        let i: i64 = v.parse().map_err(|_| err(format!("`{word}` isn't a vertex index")))?;
                        let resolved = if i < 0 { points.len() as i64 + i } else { i - 1 };
                        usize::try_from(resolved)
                            .ok()
                            .filter(|&r| r < points.len())
                            .ok_or_else(|| err(format!("vertex {i} doesn't exist")))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if !face.is_empty() {
                    faces.push(face);
                }
            }
            _ => {}
        }
    }
    Ok(Obj { points, faces })
}

/// rounded to drop float noise from scaling, e.g. `0.0375` rather than `0.037500000000000006`, without a negative zero
fn number(n: f64) -> String {
    let n = (n * 1e9).round() / 1e9;
    if n == 0.0 { "0".to_string() } else { n.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = include_str!("../../samplemods/models/models/models_table.obj");

    fn boxes(lua: &str) -> Vec<&str> {
        lua.lines().filter(|l| l.starts_with("\t\t{")).map(|l| l.trim().trim_end_matches(',')).collect()
    }

    #[test]
    fn matches_objtonodebox() {
        // output of objtonodebox's `python convert.py models_table.obj`
        assert_eq!(
            boxes(&convert(TABLE, 1.0).unwrap()),
            vec![
                "{ -0.5, 0.375, -0.5, 0.5, 0.5, 0.5 }",
                "{ 0.3125, -0.5, -0.4375, 0.4375, 0.375, -0.3125 }",
                "{ -0.4375, -0.5, -0.4375, -0.3125, 0.375, -0.3125 }",
                "{ 0.3125, -0.5, 0.3125, 0.4375, 0.375, 0.4375 }",
                "{ -0.4375, -0.5, 0.3125, -0.3125, 0.375, 0.4375 }",
            ]
        );
        // and `python convert.py models_table.obj 0.5`
        assert_eq!(
            boxes(&convert(TABLE, 0.5).unwrap()),
            vec![
                "{ -0.25, 0.1875, -0.25, 0.25, 0.25, 0.25 }",
                "{ 0.15625, -0.25, -0.21875, 0.21875, 0.1875, -0.15625 }",
                "{ -0.21875, -0.25, -0.21875, -0.15625, 0.1875, -0.15625 }",
                "{ 0.15625, -0.25, 0.15625, 0.21875, 0.1875, 0.21875 }",
                "{ -0.21875, -0.25, 0.15625, -0.15625, 0.1875, 0.21875 }",
            ]
        );
    }

    #[test]
    fn sample_mod_node_box_is_up_to_date() {
        let lua = include_str!("../../samplemods/models/models/models_table.lua");
        assert_eq!(convert(TABLE, 1.0).unwrap(), lua.replace("\r\n", "\n"));
    }

    #[test]
    fn output_is_a_node_box() {
        let lua = convert("v 0 0 0\nv 1 1 1\nv 0 1 0\nf 1 2 3\n", 1.0).unwrap();
        assert_eq!(lua, "return {\n\ttype = \"fixed\",\n\tfixed = {\n\t\t{ -1, 0, 0, 0, 1, 1 },\n\t},\n}\n");
    }

    #[test]
    fn faces_joined_through_a_later_face_are_one_box() {
        // objtonodebox's grouping splits these: the last face joins two groups that already exist
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 5 5 5\nv 6 5 5\nv 5 6 5\nf 1 2 3\nf 4 5 6\nf 3 4 2\n";
        assert_eq!(boxes(&convert(obj, 1.0).unwrap()), vec!["{ -6, 0, 0, 0, 6, 5 }"]);
    }

    #[test]
    fn index_forms() {
        // v/vt/vn, v//vn, and negative indices relative to the latest vertex
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/1/1 2//2 -1\nv 9 9 9\nv 8 8 8\nv 9 8 9\nf -3 -2 -1\n";
        assert_eq!(boxes(&convert(obj, 1.0).unwrap()), vec!["{ -1, 0, 0, 0, 1, 0 }", "{ -9, 8, 8, -8, 9, 9 }"]);
    }

    #[test]
    fn float_noise_is_rounded_off() {
        let obj = "v 0.375 0.1 0\nv 0 0 -0.0000000001\nv 0 0 0.3\nf 1 2 3\n";
        assert_eq!(boxes(&convert(obj, 0.1).unwrap()), vec!["{ -0.0375, 0, 0, 0, 0.01, 0.03 }"]);
    }

    #[test]
    fn vertices_without_faces_are_skipped() {
        let obj = "v 100 100 100\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 2 3 4\n";
        assert_eq!(boxes(&convert(obj, 1.0).unwrap()), vec!["{ -1, 0, 0, 0, 1, 0 }"]);
    }

    #[test]
    fn errors() {
        assert_eq!(convert("v 0 0 0\n", 1.0).unwrap_err(), "the model has no faces");
        assert_eq!(convert("v 0 0 0\nf 1 2 3\n", 1.0).unwrap_err(), "line 2: vertex 2 doesn't exist");
        assert_eq!(convert("v 0 x 0\n", 1.0).unwrap_err(), "line 1: `x` isn't a number");
        assert_eq!(convert("v 0 0\n", 1.0).unwrap_err(), "line 1: vertex has less than 3 coordinates");
        assert_eq!(convert("v 0 0 0\nf 0\n", 1.0).unwrap_err(), "line 2: vertex 0 doesn't exist");
    }
}
