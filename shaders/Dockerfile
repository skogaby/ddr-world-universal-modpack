# Shader build environment for DDR World .gsp replacements.
#
# vkd3d-compiler (Wine project) compiles HLSL -> legacy D3D9 bytecode
# (d3dbc, SM1-3) natively — no wine, no fxc.exe, runs on arm64 and x86_64
# alike. Fedora ships a modern vkd3d (Debian's is too old — 1.2 has no HLSL
# frontend).
#
# Built automatically by scripts/build_shaders.sh (image tag: ddr-shader-build).
FROM fedora:42

RUN dnf install -y --setopt=install_weak_deps=False vkd3d-compiler python3 \
    && dnf clean all

WORKDIR /work
