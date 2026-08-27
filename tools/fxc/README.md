# tools/fxc

Microsoft HLSL shader compiler **fxc 9.29.952.3111** (x64) + its
`D3DCompiler_43.dll`, extracted from the DirectX SDK June 2010
(`DXSDK_Jun10.exe`, `DXSDK/Utilities/bin/x64/fxc.exe` +
`DXSDK/Redist/Jun2010_D3DCompiler_43_x64.cab`). This is the same compiler
lineage the game's stock shaders were built with.

Used by `scripts/build_shaders.sh` as the golden-path compiler for the
committed `.d3dbc` blobs — its SM3 codegen is ~4.4× smaller than
vkd3d-compiler's for our pixel shaders and hits exact stock parity for the
vertex shaders (see
`.agents/planning/20260721-player-perspective-hallway/research/fxc-performance.md`).

Runs on macOS under CrossOver/Wine:

```bash
wine --bottle bemani tools/fxc/fxc.exe /nologo /T ps_3_0 /E ps_main /Fo out.d3dbc in.hlsl
```

(fxc.exe finds the DLL beside it; compiles in ~1 s.) The Docker/vkd3d path
remains as the no-wine fallback.
