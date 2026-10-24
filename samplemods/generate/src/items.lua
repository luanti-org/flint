local S = core.get_translator("generate")
local FS = function(...)
	return core.formspec_escape(S(...))
end

core.register_craftitem("generate:widget", {
	description = S("Widget"),
	inventory_image = "generate_widget.png",
	on_use = function(_, user)
		-- needs 5.5
		local pos = vector.zero()
		core.show_formspec(user:get_player_name(), "generate:widget", "label[0,0;" .. FS("A \"quoted\" label") .. "]")
		return nil, pos
	end,
})

-- a string in another textdomain gets it as its msgctxt
print(core.translate("other_domain", "Elsewhere"))
