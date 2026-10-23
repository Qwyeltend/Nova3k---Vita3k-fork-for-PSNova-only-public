#!/usr/bin/env bash
# Nova3K: after `cargo ndk build -p nova3k_embed`, make sure Lua (used by PhantasyServer scripts) gets linked.
# mlua builds Lua 5.1 from source (vendored) as a separate static library; if it is not inside
# libnova3k_embed.a, copy it next to it as liblua_nova3k.a so vita3k/CMakeLists.txt links it too.
# Run from external/phantasyserver.
set -u
REL=target/aarch64-linux-android/release
echo "== mlua-sys build script output =="
for f in $REL/build/mlua-sys-*/output; do echo "-- $f"; grep "cargo:" "$f"; done
NM=$(ls "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-nm 2>/dev/null || command -v nm)
if "$NM" -A "$REL/libnova3k_embed.a" 2>/dev/null | grep -q " T lua_settop"; then
  echo "Lua is inside libnova3k_embed.a"
  exit 0
fi
LUA=$(find $REL/build -name 'liblua*.a' | head -n1)
echo "Lua library found: ${LUA:-none}"
if [ -n "$LUA" ]; then
  cp "$LUA" $REL/liblua_nova3k.a
  "$NM" "$REL/liblua_nova3k.a" | grep " T lua_settop"
fi
