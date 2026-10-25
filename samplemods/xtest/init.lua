-- a robot as .x and as glTF made by `flint convert`, to compare them side by side.
-- its nose is on its front, and its arm waves. right-click either to switch animations, using the .x's frame ranges
-- for both. the model and texture are made for flint, see models/xtest_robot.x

local animations = {
	{ name = "stand", range = { x = 0, y = 10 } },
	{ name = "wave", range = { x = 10, y = 30 } },
}

local function register(name, mesh)
	core.register_entity("xtest:" .. name, {
		initial_properties = {
			visual = "mesh",
			mesh = mesh,
			textures = { "xtest_robot.png" },
			collisionbox = { -0.3, 0.0, -0.3, 0.3, 1.0, 0.3 },
			physical = true,
		},
		on_activate = function(self)
			self.animation = 2
			self:play()
		end,
		play = function(self)
			local animation = animations[self.animation]
			self.object:set_animation(animation.range, 15, 0, true)
			self.object:set_properties({ nametag = name .. ": " .. animation.name })
		end,
		on_rightclick = function(self)
			self.animation = self.animation % #animations + 1
			self:play()
		end,
	})
end

register("x", "xtest_robot.x")
register("gltf", "xtest_robot.glb")
