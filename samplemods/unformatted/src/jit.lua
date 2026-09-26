-- LuaJIT-only syntax
local ffi_like = 0xFFULL
local signed = -5LL
local imag = 3i
for i=1,10 do
  if i%2==0 then goto continue end
  print(i)
  ::continue::
end
