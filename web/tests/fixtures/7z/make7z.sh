#!/bin/sh
# Rebuilds the 7z fixtures with 7-Zip (7z on PATH). Run from web/.
set -e
out=$(pwd)/tests/fixtures/7z
src=$(mktemp -d)
node tests/sevenzfiles.js "$src"
cd "$src"
rm -f "$out"/*.7z
7z a -bd -m0=LZMA2 "$out/lzma2.7z" ./* >/dev/null
7z a -bd -m0=LZMA -ms=off "$out/lzma-blocks.7z" ./* >/dev/null
7z a -bd -m0=Copy -mhc=off "$out/stored.7z" ./* >/dev/null
7z a -bd -m0=Deflate "$out/deflate.7z" ./* >/dev/null
7z a -bd -m0=BCJ -m1=LZMA "$out/bcj.7z" ./* >/dev/null
7z a -bd -m0=BCJ2 -m1=LZMA2 -m2=LZMA -m3=LZMA -mb0s0:1 -mb0s1:2 -mb0s2:3 "$out/bcj2.7z" ./* >/dev/null
7z a -bd -m0=Delta:4 -m1=LZMA2 "$out/delta.7z" ./* >/dev/null
7z a -bd -m0=PPMd "$out/ppmd.7z" ./* >/dev/null
7z a -bd -pfixture "$out/password.7z" ./* >/dev/null
7z a -bd -pfixture -mhe=on "$out/password-names.7z" ./* >/dev/null
rm -rf "$src"
