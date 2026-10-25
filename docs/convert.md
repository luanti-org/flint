---
title: Convert
---

converts from one format to another deterined by the extensions of the input and output:

```sh
flint convert models/mymod_mob.b3d models/mymod_mob.glb
flint convert models/mymod_table.obj models/mymod_table.lua
flint convert models/mymod_table.obj models/mymod_table.lua --scale 0.5
flint convert locale/mymod.de.tr locale/mymod.de.po
```

| from | to | what's made |
|---|---|---|
| `.b3d` | `.gltf`, `.glb` | a glTF model, see [b3d to glTF](#b3d-to-gltf) |
| `.obj` | `.lua` | a node box of the model, see [obj to node box](#obj-to-node-box) |
| `.tr` | `.po` | gettext translations, the same as the fix of [`luanti_legacy_tr_translation`](lint.md#package-lints). the file must be named `<textdomain>.<lang>.tr` |

| option | purpose |
|---|---|
| `--scale` | multiply the node box's coordinates by this, for `.obj` models. defaults to `1` |

an existing output file is overwritten. `.lua` files are formatted like [`flint format`](format.md) does, with its settings from [flint.toml](config.md).

## b3d to glTF

ported from [modlib](https://github.com/appgurueu/modlib)'s converter. meshes, normals, vertex colors, texture coordinates, bones and their weights, and keyframes are converted, so the model loads in Luanti like the `.b3d` did. `.glb` is recommended, as it's smaller.

usually only the file name in the mod needs changing:

* frame numbers stay the same, so `set_animation` ranges keep working
* `textures` keep applying to the same parts of the model
* the model faces the same way

the exception is a static model used as a node's `mesh`: Luanti uses 1 unit per node for static `.b3d` models, but 10 for glTF, so multiply the node's `visual_scale` by 10. animated models and entities use 10 units per node for both.

what Luanti ignores in a `.b3d` is left out, e.g. texture flags and brush blend modes. a warning is printed for anything that changes how the model looks, e.g. scaled textures, which Luanti stretches the texture coordinates of for `.b3d` but not glTF.

## obj to node box

ported from [objtonodebox](https://github.com/regulus79/objtonodebox). every connected part of the model, i.e. vertices joined by faces, becomes a box around it, so a model made of cuboids becomes a node box of those cuboids. the X axis is flipped, like Luanti does for `.obj` models, so the node box lines up with the mesh.

the result is a Lua file returning the node box, to use as is or copy into the node definition:

```lua
return {
	type = "fixed",
	fixed = {
		{ -0.5, 0.375, -0.5, 0.5, 0.5, 0.5 },
		{ 0.3125, -0.5, -0.4375, 0.4375, 0.375, -0.3125 },
	},
}
```

```lua
local node_box = dofile(core.get_modpath("mymod") .. "/models/mymod_table.lua")

core.register_node("mymod:table", {
	drawtype = "mesh",
	mesh = "mymod_table.obj",
	selection_box = node_box,
	collision_box = node_box,
	-- ...
})
```
