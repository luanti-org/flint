local S = core.get_translator("unformatted")
  core.register_node("unformatted:junk", {
description = S("Junk Block"),
      tiles = {"unformatted_junk.png","unformatted_junk.png","unformatted_junk_side.png"},
  groups = {cracky=3, oddly_breakable_by_hand=1},
      on_punch=function(pos,node,puncher,pointed_thing)
          local meta=core.get_meta(pos) meta:set_int("hits",meta:get_int("hits")+1)
      end,
})
for i=1,3 do core.register_craftitem("unformatted:scrap_"..i,{description=S("Scrap @1",i),inventory_image="unformatted_scrap.png"}) end
