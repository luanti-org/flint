//! `flint convert`: convert a file to another format, picked by their extensions

use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::lint::package::translations;
use crate::parsers::b3d;

mod gltf;
mod nodebox;

const SUPPORTED: &str = "`.b3d` to `.gltf` or `.glb`, `.obj` to a node box `.lua`, and `.tr` to `.po`";

pub fn run(config: &Config, input: &Path, output: &Path, scale: Option<f64>) -> Result<(), String> {
    let extension = |path: &Path| path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let (from, to) = (extension(input), extension(output));
    if !matches!((from.as_str(), to.as_str()), ("b3d", "gltf" | "glb") | ("obj", "lua") | ("tr", "po")) {
        return Err(format!("cannot convert `.{from}` to `.{to}`, only {SUPPORTED}"));
    }
    if scale.is_some() && from != "obj" {
        return Err("`--scale` only applies to `.obj` files".to_string());
    }
    let data = fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let text = || String::from_utf8(data.clone()).map_err(|_| "not UTF-8".to_string());

    let converted = match (from.as_str(), to.as_str()) {
        ("b3d", "gltf" | "glb") => b3d::parse(&data).and_then(|b3d| gltf::convert(&b3d)).map(|(gltf, warnings)| {
            (if to == "glb" { gltf.to_glb() } else { gltf.to_gltf() }, warnings)
        }),
        ("obj", "lua") => text()
            .and_then(|obj| nodebox::convert(&obj, scale.unwrap_or(1.0)))
            // like `flint format` would, e.g. for long numbers
            .and_then(|lua| crate::format::format_code(&lua, config.stylua()?))
            .map(|lua| (lua.into_bytes(), Vec::new())),
        ("tr", "po") => text().and_then(|tr| translations::tr_to_po(input, &tr)).map(|po| (po.into_bytes(), Vec::new())),
        _ => unreachable!("checked above"),
    };
    let (bytes, warnings) = converted.map_err(|e| format!("{}: {e}", input.display()))?;

    for warning in warnings {
        eprintln!("warning: {}: {warning}", input.display());
    }
    fs::write(output, bytes).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    println!("converted {} to {}", input.display(), output.display());
    Ok(())
}
