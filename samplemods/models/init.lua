-- models/models_table.lua is made by `flint convert models/models_table.obj models/models_table.lua`
local modpath = core.get_modpath("models")
local node_box = dofile(modpath .. "/models/models_table.lua")

core.register_node("models:table", {
	description = "Table",
	drawtype = "mesh",
	mesh = "models_table.obj",
	tiles = { "default_wood.png" },
	paramtype = "light",
	selection_box = node_box,
	collision_box = node_box,
})
