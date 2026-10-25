//! `flint convert`: convert a file to another format, picked by their extensions

use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::lint::package::translations;

pub(crate) mod b3d;
pub(crate) mod build;
pub(crate) mod gltf;
mod nodebox;
mod x;

const SUPPORTED: &str = "`.b3d` and `.x` to `.gltf` or `.glb`, `.obj` to a node box `.lua`, and `.tr` to `.po`";

pub fn run(config: &Config, input: &Path, output: &Path, scale: Option<f64>) -> Result<(), String> {
    let extension = |path: &Path| path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let (from, to) = (extension(input), extension(output));
    if !matches!((from.as_str(), to.as_str()), ("b3d" | "x", "gltf" | "glb") | ("obj", "lua") | ("tr", "po")) {
        return Err(format!("cannot convert `.{from}` to `.{to}`, only {SUPPORTED}"));
    }
    if scale.is_some() && from != "obj" {
        return Err("`--scale` only applies to `.obj` files".to_string());
    }
    let data = fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let text = || String::from_utf8(data.clone()).map_err(|_| "not UTF-8".to_string());

    let converted = match (from.as_str(), to.as_str()) {
        ("b3d" | "x", "gltf" | "glb") => to_gltf(&from, &data).map(|(gltf, warnings)| {
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
        eprintln!("warning: {}: {}", input.display(), warning.message);
    }
    fs::write(output, bytes).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    println!("converted {} to {}", input.display(), output.display());
    Ok(())
}

pub(crate) fn to_gltf(extension: &str, data: &[u8]) -> Result<(gltf::Gltf, Vec<build::Warning>), String> {
    let (model, mut warnings) = read(extension, data)?;
    let (gltf, more) = model.finish()?;
    warnings.extend(more);
    Ok((gltf, warnings))
}

pub(crate) fn read(extension: &str, data: &[u8]) -> Result<(build::Builder, Vec<build::Warning>), String> {
    match extension {
        "x" => x::read(data),
        _ => Ok((b3d::read(data)?, Vec::new())),
    }
}
