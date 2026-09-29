#!/usr/bin/env python3
"""Reference decoders for the DDR ULTRAMIX / Dancing Stage Unleashed (Xbox) "K3D"
dancer assets: `.ddm` models, `.ani` skeletal clips, `.xpu` pixel shaders and
the vertex-shader sources embedded in `default.xbe`.

Formats and semantics: docs/dancing_stage_unleashed_dancers_port_feasibility.md.
Extract the archive first with scripts/extract_ultramix_data.py.

Usage:
    ultramix_k3d_dump.py ddm <file.ddm>                     # header, bones, mesh stats
    ultramix_k3d_dump.py ani <file.ani>                     # tracks / frames
    ultramix_k3d_dump.py xpu <file.xpu>                     # register-combiner disassembly
    ultramix_k3d_dump.py vsh <default.xbe>                  # embedded K3D vertex-shader sources
    ultramix_k3d_dump.py obj <file.ddm> <file.ani> <frame> <out.obj>   # CPU-skinned pose
    ultramix_k3d_dump.py hierarchy <file.ani>...            # verify HIERARCHY is rigid in clips
    ultramix_k3d_dump.py match <dir with .ani> <dir with World mc_*.anm>  # same-take survey

Import-safe: `from ultramix_k3d_dump import parse_ddm, parse_ani, skin_pose, HIERARCHY`.
Needs numpy (`ddm`/`ani`/`xpu`/`vsh` do not).
"""
import glob
import os
import struct
import sys

DDM_MAGIC = b"srdd"
ANI_MAGIC = b"mina"
XPU_MAGIC = b"PSB0"

# Bone hierarchy. `.ddm` and `.ani` carry NO parent indices: the game
# uploads each clip's world-space joint matrices straight into the skinning
# constants, so it never needs one. This table was rebuilt from the joint
# names and checked with `hierarchy`: every child's offset in its parent's
# frame is constant over every frame of every shipped clip.
HIERARCHY = {
    "root": None,
    "Spine_Low": "root",
    "Spine": "Spine_Low",
    "Sternum": "Spine",
    "Neck_1": "Sternum",
    "Neck_2": "Neck_1",
    "Head": "Neck_2",
    "Head_End": "Head",
    "AfroBone": "Head",
    "AfroEnd": "AfroBone",
    "BreastLeft": "Spine",
    "BreastLeftEnd": "BreastLeft",
    "BreastRight": "Spine",
    "BreastRightEnd": "BreastRight",
    "Hip_L_DUM": "root",
    "Leg_L": "Hip_L_DUM",
    "Knee_L": "Leg_L",
    "Ankle_L": "Knee_L",
    "Toe_L": "Ankle_L",
    "Toe_L_end_site": "Toe_L",
    "Hip_R_DUM": "root",
    "Leg_R": "Hip_R_DUM",
    "Knee_R": "Leg_R",
    "Ankle_R": "Knee_R",
    "Toe_R": "Ankle_R",
    "Toe_R_end_site": "Toe_R",
}
for _s in ("L", "R"):
    HIERARCHY.update(
        {
            f"Clav_{_s}1": "Sternum",
            f"Clav_{_s}2": f"Clav_{_s}1",
            f"shoulder_{_s}": f"Clav_{_s}2",
            f"Elbow_{_s}": f"shoulder_{_s}",
            f"Wrist_{_s}": f"Elbow_{_s}",
            f"Wrist_{_s}2": f"Wrist_{_s}",
            f"Wrist_{_s}_end": f"Wrist_{_s}2",
        }
    )

# DSU3 (Dancing Stage Unleashed 3) rigs use a different, Maya-style set of joint names, and
# none of these names clash with the table above. Checked the same way: every child's offset
# in its parent's frame stays constant over the dance clips. The exception is the skirt tips,
# which are simulated and so are not rigid to any joint; they hang off the nearest segment.
# `R_knee` (lower-case k) is spelled that way in the game data.
HIERARCHY.update(
    {
        "M_Root": None,
        "M_Hip": "M_Root",
        "M_Spine": "M_Root",
        "M_Chest": "M_Spine",
        "M_Neck": "M_Chest",
        "M_Neck2": "M_Neck",
        "M_Head": "M_Neck2",
        "M_Hair": "M_Head",
        "M_BackskirtTip": "M_Hip",
        "M_FrontskirtTip": "M_Hip",
    }
)
for _s in ("L", "R"):
    HIERARCHY.update(
        {
            f"{_s}_Clavicle": "M_Chest",
            f"{_s}_Shoulder": f"{_s}_Clavicle",
            f"{_s}_ShoulderTwistlocke": f"{_s}_Shoulder",
            f"{_s}_ShoulderHalf": f"{_s}_ShoulderTwistlocke",
            f"{_s}_Elbow": f"{_s}_ShoulderHalf",
            f"{_s}_Wrist": f"{_s}_Elbow",
            f"{_s}_Leg": "M_Hip",
            f"{_s}_SkirtTip": f"{_s}_Leg",
            f"{_s}_Breast": "M_Chest",
            f"{_s}_Breasttip": f"{_s}_Breast",
            f"{_s}_SidehairRoot": "M_Hair",
            f"{_s}_SidehairKnot1": f"{_s}_SidehairRoot",
        }
    )
HIERARCHY.update({"L_Knee": "L_Leg", "R_knee": "R_Leg", "L_Ankle": "L_Knee", "R_Ankle": "R_knee"})


# ---------------------------------------------------------------------------
# .ddm model
# ---------------------------------------------------------------------------
DDM3_MATERIAL_SIZE = 0x14C


def _ddm_tail(data, off):
    """Bones, triangle list and vertices, starting at the bone count. Returns
    (bones, indices, vertex_count, vertex_offset), or None if the file does not end
    exactly after the vertices."""
    if off + 4 > len(data):
        return None
    (bone_count,) = struct.unpack_from("<I", data, off)
    off += 4
    if off + 0x84 * bone_count + 4 > len(data):
        return None
    bones = []
    for i in range(bone_count):
        inv_bind = struct.unpack_from("<16f", data, off)
        name = data[off + 0x40 : off + 0x80].split(b"\0")[0].decode("latin1")
        (reg,) = struct.unpack_from("<i", data, off + 0x80)
        bones.append(dict(index=i, name=name, inverse_bind=inv_bind, vs_register=reg))
        off += 0x84
    (index_count,) = struct.unpack_from("<I", data, off)
    if off + 4 + 2 * index_count + 4 > len(data):
        return None
    indices = struct.unpack_from("<%dH" % index_count, data, off + 4)
    off += 4 + 2 * index_count
    (vertex_count,) = struct.unpack_from("<I", data, off)
    vertex_off = off + 4
    if vertex_off + 44 * vertex_count != len(data):
        return None
    return bones, indices, vertex_count, vertex_off


def parse_ddm(data: bytes) -> dict:
    """`srdd` model: material(s), bones, triangle list, vertices.

    Two revisions share the magic:
      * DSU1/DSU2: one D3DMATERIAL8 + one 0x100 texture name at 0x04.
      * DSU3 (loader FUN_001f52c0): u32 material count at 0x04, then per material a
        0x14C-byte record { D3DMATERIAL8 (0x44), texture name (0x100), u32 first index,
        u32 triangle count }. Each material draws its own contiguous triangle range.
    Both are returned with `materials` = [{material, texture, first_index, triangles}];
    `texture` / `material` are the first material's (the DSU1/2 single-texture fields)."""
    if data[:4] != DDM_MAGIC:
        raise ValueError("not a .ddm (magic %r)" % data[:4])
    tail = _ddm_tail(data, 0x148)
    if tail is not None:
        material = struct.unpack_from("<17f", data, 0x04)  # Diffuse, Ambient, Specular, Emissive, Power
        texture = data[0x48:0x148].split(b"\0")[0].decode("latin1")
        bones, indices, vertex_count, vertex_off = tail
        materials = [dict(material=material, texture=texture, first_index=0, triangles=len(indices) // 3)]
    else:
        (count,) = struct.unpack_from("<I", data, 0x04)
        off = 0x08 + DDM3_MATERIAL_SIZE * count
        tail = _ddm_tail(data, off) if 0 < count < 256 else None
        if tail is None:
            raise ValueError(".ddm size mismatch: neither the DSU1/2 nor the DSU3 layout ends at EOF")
        bones, indices, vertex_count, vertex_off = tail
        materials = []
        for i in range(count):
            m = 0x08 + DDM3_MATERIAL_SIZE * i
            first, tris = struct.unpack_from("<II", data, m + 0x144)
            materials.append(dict(material=struct.unpack_from("<17f", data, m),
                                  texture=data[m + 0x44 : m + 0x144].split(b"\0")[0].decode("latin1"),
                                  first_index=first, triangles=tris))
        at = 0
        for mt in materials:
            if mt["first_index"] != at:
                raise ValueError(".ddm material %r does not start where the previous one ended" % mt["texture"])
            at += 3 * mt["triangles"]
        if at != len(indices):
            raise ValueError(".ddm materials cover %d of %d indices" % (at, len(indices)))
    return dict(
        material=materials[0]["material"],
        texture=materials[0]["texture"],
        materials=materials,
        bones=bones,
        indices=indices,
        vertex_count=vertex_count,
        vertex_offset=vertex_off,
        data=data,
    )


def ddm_vertices(model):
    """numpy (n, 11): pos xyz, normal xyz, uv, bone0 reg, bone1 reg, bone0 weight (all f32)."""
    import numpy as np

    return np.frombuffer(model["data"], "<f4", model["vertex_count"] * 11, model["vertex_offset"]).reshape(-1, 11)


# ---------------------------------------------------------------------------
# .ani clip
# ---------------------------------------------------------------------------
def parse_ani(data: bytes) -> dict:
    """`mina` clip: per track a 56-byte name, two u32 (12, 1 in the male clips;
    uninitialised in the female ones), then one 28-byte key per frame:
    quaternion (x, y, z, w) + translation, the joint's WORLD transform."""
    if data[:4] != ANI_MAGIC:
        raise ValueError("not an .ani (magic %r)" % data[:4])
    track_count, frame_count = struct.unpack_from("<II", data, 4)
    off, tracks = 12, []
    for _ in range(track_count):
        name = data[off : off + 56].split(b"\0")[0].decode("latin1")
        a, b = struct.unpack_from("<II", data, off + 56)
        tracks.append(dict(name=name, field_a=a, field_b=b, key_offset=off + 64))
        off += 64 + 28 * frame_count
    if off != len(data):
        raise ValueError(".ani size mismatch: parsed %d of %d bytes" % (off, len(data)))
    return dict(frame_count=frame_count, tracks=tracks, data=data)


def ani_keys(clip, name):
    """numpy (frames, 7) for one track: qx qy qz qw tx ty tz."""
    import numpy as np

    for t in clip["tracks"]:
        if t["name"] == name:
            return np.frombuffer(clip["data"], "<f4", clip["frame_count"] * 7, t["key_offset"]).reshape(-1, 7)
    raise KeyError(name)


def quat_rowmat(q):
    """D3DXMatrixRotationQuaternion (row-vector convention), q = (x, y, z, w)."""
    import numpy as np

    x, y, z, w = q
    return np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w)],
            [2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w)],
            [2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y)],
        ]
    )


def world_matrix(key):
    import numpy as np

    m = np.eye(4)
    m[:3, :3] = quat_rowmat(key[:4])
    m[3, :3] = key[4:7]
    return m


def skin_pose(model, clip, frame):
    """Game-equivalent skinning (the K3D `xvs.1.1` shaders): per bone
    `M = inverseBind · world(key)`, vertex = w·(v·M[b0]) + (1−w)·(v·M[b1]).
    Returns (positions, normals) in the clip's Y-up world space."""
    import numpy as np

    verts = ddm_vertices(model)
    mats = {}
    for b in model["bones"]:
        inv = np.array(b["inverse_bind"]).reshape(4, 4)
        mats[b["vs_register"]] = inv @ world_matrix(ani_keys(clip, b["name"])[frame])
    p = np.c_[verts[:, 0:3], np.ones(len(verts))]
    m0 = np.stack([mats[int(r)] for r in verts[:, 8]])
    m1 = np.stack([mats[int(r)] for r in verts[:, 9]])
    w = verts[:, 10:11]
    pos = w * np.einsum("ni,nij->nj", p, m0)[:, :3] + (1 - w) * np.einsum("ni,nij->nj", p, m1)[:, :3]
    n = verts[:, 3:6]
    nrm = w * np.einsum("ni,nij->nj", n, m0[:, :3, :3]) + (1 - w) * np.einsum("ni,nij->nj", n, m1[:, :3, :3])
    nrm /= np.linalg.norm(nrm, axis=1, keepdims=True)
    return pos, nrm


# ---------------------------------------------------------------------------
# DSU -> DDR World game space (the native-rig port, design
# .agents/planning/2026-09-28-dsu-dancer-port/design.md D3)
# ---------------------------------------------------------------------------
# Metres per DSU model unit: puts the rest pelvis at World's Hips height (0.97 m).
GAME_SCALE = 0.1026
# DSU plays the window [LOOP_IN, frames - LOOP_IN] of every clip
# (PlayAnimation(anim, dur, 15, frames - 15, …), FUN_00067450).
LOOP_IN = 15
# Bind space (Z-up, facing +Y) -> clip space (Y-up, facing -Z): (x, y, z) -> (x, z, -y).
_R_BC = ((1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0))


def _affine(r3, t):
    import numpy as np

    m = np.eye(4)
    m[:3, :3] = r3
    m[3, :3] = t
    return m


def game_space(model, scale=GAME_SCALE):
    """The two maps into World's game space (row-vector, Y-up, metres, dancer
    facing +Z with its left at +X — D3D left-handed DSU data mirrored in Z):
      bind:  p_game = p_bind · C       (C also lifts the rest mesh onto y = 0)
      clip:  p_game = p_clip · L       (L = diag(s, s, -s, 1); DSU's floor is y = 0)
    Bone frames are carried as metres, mirrored: B_game = L⁻¹ · B · C and
    W_game = L⁻¹ · W · L stay proper rotations, and the skinning product is
    preserved: v_game · B_game⁻¹ · W_game = (v_bind · B⁻¹ · W) · L."""
    import numpy as np

    lin = np.array(_R_BC) @ np.diag([1.0, 1.0, -1.0]) * scale
    lowest = float((ddm_vertices(model)[:, 0:3] @ lin)[:, 1].min())
    C = _affine(lin, (0.0, -lowest, 0.0))
    L = np.diag([scale, scale, -scale, 1.0])
    return C, L


def game_bind_matrices(model, scale=GAME_SCALE):
    """{bone name: 4x4 row-vector bind (local -> game world)}."""
    import numpy as np

    C, L = game_space(model, scale)
    Li = np.linalg.inv(L)
    return {b["name"]: Li @ np.linalg.inv(np.array(b["inverse_bind"]).reshape(4, 4)) @ C for b in model["bones"]}


def game_mesh(model, scale=GAME_SCALE):
    """Rest mesh in game space: (positions, unit normals, uv (D3D v-down),
    [(bone0 name, w0), (bone1 name, 1 - w0)] per vertex, triangles with the
    winding reversed for the mirror)."""
    import numpy as np

    C, _ = game_space(model, scale)
    v = ddm_vertices(model)
    pos = np.c_[v[:, 0:3], np.ones(len(v))] @ C
    nrm = v[:, 3:6] @ (np.array(_R_BC) @ np.diag([1.0, 1.0, -1.0]))
    nrm /= np.linalg.norm(nrm, axis=1, keepdims=True)
    reg = {b["vs_register"]: b["name"] for b in model["bones"]}
    weights = []
    for row in v:
        b0, b1, w0 = reg[int(row[8])], reg[int(row[9])], float(row[10])
        weights.append([(b0, 1.0)] if b0 == b1 else [(b0, w0), (b1, 1.0 - w0)])
    tris = np.array(model["indices"], dtype=np.int64).reshape(-1, 3)[:, [0, 2, 1]]
    return pos[:, :3], nrm, v[:, 6:8].copy(), weights, tris


def hierarchy_order(names):
    """`names` (a model's bones) parent-first: depth order, ties by name. Every
    name must be in HIERARCHY and its parent (if any) present."""
    def depth(n):
        d = 0
        while HIERARCHY[n] is not None:
            n = HIERARCHY[n]
            d += 1
        return d

    for n in names:
        if n not in HIERARCHY:
            raise KeyError("bone %r has no HIERARCHY entry" % n)
        p = HIERARCHY[n]
        if p is not None and p not in names:
            raise KeyError("bone %r's parent %r is not in the model" % (n, p))
    return sorted(names, key=lambda n: (depth(n), n))


def rowmat_to_quat(r):
    """Inverse of quat_rowmat: row-vector 3x3 rotation -> (x, y, z, w)."""
    import numpy as np

    m = np.asarray(r).T  # column-vector rotation
    t = m[0, 0] + m[1, 1] + m[2, 2]
    if t > 0:
        s = 2.0 * np.sqrt(t + 1.0)
        q = ((m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s)
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = 2.0 * np.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2])
        q = (0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s)
    elif m[1, 1] > m[2, 2]:
        s = 2.0 * np.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2])
        q = ((m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s)
    else:
        s = 2.0 * np.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1])
        q = ((m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s)
    q = np.array(q)
    return tuple(q / np.linalg.norm(q))


def clip_game_worlds(model, clip, bone_names, target_binds, scale=GAME_SCALE, loop_in=LOOP_IN):
    """Per played DSU frame, per target bone: the world matrix in game space
    for the EXPORTED bind frames `target_binds` (any rigid re-framing of the
    DSU bone frames — e.g. Blender's Y-along-the-bone rest — is absorbed by
    Q = B_target · B_game⁻¹, so the skinning product is unchanged).
    Returns (frames x bones x 4 x 4 array, the DSU frame indices used)."""
    import numpy as np

    _, L = game_space(model, scale)
    Li = np.linalg.inv(L)
    binds = game_bind_matrices(model, scale)
    q = [target_binds[i] @ np.linalg.inv(binds[n]) for i, n in enumerate(bone_names)]
    keys = [ani_keys(clip, n) for n in bone_names]
    last = clip["frame_count"] - 1
    first, end = (loop_in, last - loop_in) if last - 2 * loop_in >= 1 else (0, last)
    frames = list(range(first, end + 1))
    out = np.empty((len(frames), len(bone_names), 4, 4))
    for fi, f in enumerate(frames):
        for bi in range(len(bone_names)):
            out[fi, bi] = q[bi] @ Li @ world_matrix(keys[bi][f]) @ L
    return out, frames


def ani_to_anm_spec(model, clip, bone_names, parents, target_binds, scale=GAME_SCALE, loop_in=LOOP_IN):
    """A scripts/anm_dump.py::write_anm spec for one DSU clip on the exported
    rig (`bone_names` in file order, `parents` indices, `target_binds` 4x4 row
    matrices from the exported .model). DSU's 30 Hz keys land every 2nd
    frame of World's 60 fps timeline (explicit key times — the evaluator
    slerps between them, as DSU itself interpolates between frames): no
    resampling. Rotation = kind 0x1C, translation = 0x1D, one key when a
    channel never changes."""
    import numpy as np

    worlds, frames = clip_game_worlds(model, clip, bone_names, target_binds, scale, loop_in)
    n_f, n_b = worlds.shape[:2]
    times = [2 * i for i in range(n_f)]
    tracks = []
    for b in range(n_b):
        p = parents[b]
        locs = worlds[:, b] if p < 0 else np.einsum("fij,fjk->fik", worlds[:, b], np.linalg.inv(worlds[:, p]))
        quats, prev = [], None
        for m in locs:
            qv = rowmat_to_quat(m[:3, :3])
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            quats.append(qv)
            prev = qv
        trans = [tuple(float(x) for x in m[3, :3]) for m in locs]
        qa = np.array(quats)
        if np.abs(qa - qa[0]).max() < 1e-6:
            tracks.append(dict(kind=0x1C, target=b, keys=[quats[0]]))
        else:
            tracks.append(dict(kind=0x1C, target=b, times=times, keys=quats))
        ta = np.array(trans)
        if np.abs(ta - ta[0]).max() < 1e-5:
            tracks.append(dict(kind=0x1D, target=b, keys=[trans[0]]))
        else:
            tracks.append(dict(kind=0x1D, target=b, times=times, keys=trans))
    return dict(frame_count=times[-1], flag=0, hierarchy=list(parents), tracks=tracks), frames


# ---------------------------------------------------------------------------
# .xpu pixel shader (D3DPIXELSHADERDEF_FILE: 'PSB0' + 60-dword NV2A program)
# ---------------------------------------------------------------------------
_REG = {0: "zero", 1: "c0", 2: "c1", 3: "fog", 4: "v0", 5: "v1", 8: "t0", 9: "t1", 10: "t2", 11: "t3",
        12: "r0", 13: "r1", 14: "v1r0sum", 15: "efprod"}
_TEX = ["none", "project2d", "project3d", "cubemap", "passthru", "clipplane", "bumpenv", "bumpenv_lum",
        "brdf", "dot_st", "dot_zw", "dot_rflct_diff", "dot_rflct_spec", "dot_str_3d", "dot_str_cube",
        "dpndnt_ar", "dpndnt_gb", "dotproduct", "dot_rflct_spec_const"]
_OUT_MAP = {0: "", 1: " bias", 2: " x2", 3: " x2 bias", 4: " x4", 6: " /2"}


def _xpu_input(b, alpha):
    reg, chan, mapping = b & 0xF, (b >> 4) & 1, (b >> 5) & 7
    if reg == 0:  # the constant-producing encodings of PS_REGISTER_ZERO
        const = {0: "0", 1: "1", 2: "-1", 4: "-0.5", 5: "0.5", 6: "0"}.get(mapping)
        if const:
            return const
    s = _REG.get(reg, "reg%d" % reg) + (".a" if chan else (".b" if alpha else ".rgb"))
    return ["sat(%s)", "(1-sat(%s))", "(2*%s-1)", "-(2*%s-1)", "(%s-0.5)", "-(%s-0.5)", "%s", "-%s"][mapping] % s


def disassemble_xpu(data: bytes) -> str:
    if data[:4] != XPU_MAGIC or len(data) != 244:
        raise ValueError("not a 244-byte PSB0 .xpu")
    w = struct.unpack_from("<60I", data, 4)
    alpha_in, rgb_in, alpha_out, rgb_out = w[0:8], w[34:42], w[26:34], w[45:53]
    c0, c1 = w[10:18], w[18:26]
    count, texmodes = w[53], w[54]
    lines = ["texture stages: " + ", ".join("t%d=%s" % (i, _TEX[(texmodes >> (5 * i)) & 0x1F]) for i in range(4)),
             "combiners: %d  C0 map %08x  C1 map %08x  final consts %08x" % (count & 0xFF, w[57], w[58], w[59])]
    for i in range(count & 0xFF):
        lines.append("stage %d: c0=%08x c1=%08x" % (i, c0[i], c1[i]))
        for label, ins, out, alpha in (("rgb", rgb_in[i], rgb_out[i], False), ("a", alpha_in[i], alpha_out[i], True)):
            a, b, c, d = (_xpu_input((ins >> s) & 0xFF, alpha) for s in (24, 16, 8, 0))
            ab, cd, sm, f = (out >> 4) & 0xF, out & 0xF, (out >> 8) & 0xF, out >> 12
            ops = []
            if ab:
                ops.append("%s.%s = %s %s %s" % (_REG[ab], label, a, "dot" if f & 2 else "*", b))
            if cd:
                ops.append("%s.%s = %s %s %s" % (_REG[cd], label, c, "dot" if f & 1 else "*", d))
            if sm:
                ops.append("%s.%s = %s" % (_REG[sm], label, ("mux(%s*%s, %s*%s)" if f & 4 else "%s*%s + %s*%s") % (a, b, c, d)))
            lines.append("  %-3s %s%s" % (label, "; ".join(ops) or "nop", _OUT_MAP.get((f >> 3) & 7, " ?")))
    abcd, efg = w[8], w[9]
    A, B, C, D = (_xpu_input((abcd >> s) & 0xFF, False) for s in (24, 16, 8, 0))
    E, F, G = (_xpu_input((efg >> s) & 0xFF, False) for s in (24, 16, 8))
    lines.append("final: rgb = %s*%s + (1-%s)*%s + %s ; alpha = %s ; EF = %s*%s ; flags %02x"
                 % (A, B, A, C, D, G, E, F, efg & 0xFF))
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Embedded vertex-shader sources (assembled at boot with XGAssembleShader)
# ---------------------------------------------------------------------------
def embedded_vertex_shaders(xbe: bytes):
    """Yield (file_offset, text) for every NUL-terminated asm source in the XBE."""
    seen = set()
    for needle in (b"ertex shader for K3DModel", b"Particle vertex shader"):
        at = xbe.find(needle)
        while at >= 0:
            start = xbe.rfind(b"\0", 0, at) + 1
            if start not in seen:
                seen.add(start)
                end = xbe.index(b"\0", at)
                yield start, xbe[start:end].decode("latin1")
            at = xbe.find(needle, at + 1)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------
def cmd_ddm(path):
    import numpy as np

    m = parse_ddm(open(path, "rb").read())
    v = ddm_vertices(m)
    print("%s: texture %r  bones %d  triangles %d  vertices %d" % (
        os.path.basename(path), m["texture"], len(m["bones"]), len(m["indices"]) // 3, m["vertex_count"]))
    print("material (D3DMATERIAL8 D/A/S/E rgba, power):", [round(x, 3) for x in m["material"]])
    if len(m["materials"]) > 1:
        for mt in m["materials"]:
            print("  material %-24r first index %6d  triangles %5d" % (mt["texture"], mt["first_index"], mt["triangles"]))
    for b in m["bones"]:
        bind = np.linalg.inv(np.array(b["inverse_bind"]).reshape(4, 4))
        print("  %2d %-15s c[%4d]  bind pos (Z-up) %8.3f %8.3f %8.3f" % (b["index"], b["name"], b["vs_register"], *bind[3, :3]))
    print("bounds", v[:, :3].min(0).round(3), v[:, :3].max(0).round(3), " weight0 range", v[:, 10].min(), v[:, 10].max())


def cmd_ani(path):
    c = parse_ani(open(path, "rb").read())
    print("%s: %d tracks x %d frames" % (os.path.basename(path), len(c["tracks"]), c["frame_count"]))
    for t in c["tracks"]:
        print("  %-16s (%08x %08x)" % (t["name"], t["field_a"], t["field_b"]))


def cmd_obj(ddm_path, ani_path, frame, out_path):
    m = parse_ddm(open(ddm_path, "rb").read())
    c = parse_ani(open(ani_path, "rb").read())
    pos, nrm = skin_pose(m, c, int(frame))
    uv = ddm_vertices(m)[:, 6:8]
    with open(out_path, "w") as f:
        # D3D left-handed -> OBJ right-handed: negate Z and reverse the winding.
        f.write("# %s posed by %s frame %s (Y-up, DSU units)\n" % (os.path.basename(ddm_path), os.path.basename(ani_path), frame))
        f.writelines("v %.5f %.5f %.5f\n" % (p[0], p[1], -p[2]) for p in pos)
        f.writelines("vt %.5f %.5f\n" % (u, 1.0 - v) for u, v in uv)
        f.writelines("vn %.5f %.5f %.5f\n" % (n[0], n[1], -n[2]) for n in nrm)
        idx = m["indices"]
        for i in range(0, len(idx), 3):
            a, c3, b = idx[i] + 1, idx[i + 1] + 1, idx[i + 2] + 1
            f.write("f %d/%d/%d %d/%d/%d %d/%d/%d\n" % (a, a, a, b, b, b, c3, c3, c3))
    print("wrote", out_path)


def cmd_hierarchy(paths):
    import numpy as np

    worst = {}
    for p in paths:
        c = parse_ani(open(p, "rb").read())
        names = {t["name"] for t in c["tracks"]}
        for child, parent in HIERARCHY.items():
            if parent is None or child not in names or parent not in names:
                continue
            kc, kp = ani_keys(c, child), ani_keys(c, parent)
            local = [quat_rowmat(kp[f][:4]) @ (kc[f][4:7] - kp[f][4:7]) for f in range(c["frame_count"])]
            dev = float(np.array(local).std(0).max())
            worst[child] = max(worst.get(child, 0.0), dev)
    for child, dev in sorted(worst.items(), key=lambda kv: -kv[1]):
        print("  %-16s <- %-12s max offset std %.2e" % (child, HIERARCHY[child], dev))


def cmd_match(dsu_dir, ddr_dir):
    """Correlate each DSU clip's root height with every World clip's Hips height,
    DSU upsampled 2x (30 -> 60 Hz). >= 0.9 means the same mocap take."""
    import numpy as np

    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import anm_dump

    def detrend(s, w):
        return (s - np.convolve(s, np.ones(w) / w, "same"))[w:-w]

    ddr = {}
    for fn in sorted(glob.glob(os.path.join(ddr_dir, "**", "*.anm"), recursive=True)):
        data = open(fn, "rb").read()
        a = anm_dump.parse_anm(data)
        tracks = [t for ch in a["chunks"] if ch["type"] == 0 for t in ch["tracks"]]
        hips = [t for t in tracks if t["target"] == 1 and t["channel"] == "translation"]
        if hips:
            n = a["header"]["frame_count"]
            ddr[os.path.basename(fn)[:-4]] = np.array([tuple(anm_dump.sample_track(data, hips[0], f))[1] for f in range(n)])
    for fn in sorted(glob.glob(os.path.join(dsu_dir, "*.ani"))):
        c = parse_ani(open(fn, "rb").read())
        a = ani_keys(c, "root")[:, 5]
        x = detrend(np.interp(np.arange(2 * len(a)) / 2.0, np.arange(len(a)), a), 41)
        x = (x - x.mean()) / x.std()
        best = []
        for k, b in ddr.items():
            y = detrend(b, 21)
            y = (y - y.mean()) / y.std()
            s, l_ = (x, y) if len(x) <= len(y) else (y, x)
            if len(s) < 60:
                continue
            windows = np.lib.stride_tricks.sliding_window_view(l_, len(s))
            windows = (windows - windows.mean(1, keepdims=True)) / windows.std(1, keepdims=True)
            pearson = windows @ ((s - s.mean()) / s.std()) / len(s)  # per-window correlation
            at = int(pearson.argmax())
            best.append((float(pearson[at]), k, at - 20))  # undo the detrend trims: World frame of DSU frame 0
        best.sort(reverse=True)
        print("%-18s %4d  " % (os.path.basename(fn), c["frame_count"]) + "  ".join("%s %.2f@%d" % (k, s, o) for s, k, o in best[:2]))


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 1
    cmd, args = argv[1], argv[2:]
    if cmd == "ddm":
        cmd_ddm(args[0])
    elif cmd == "ani":
        cmd_ani(args[0])
    elif cmd == "xpu":
        print(disassemble_xpu(open(args[0], "rb").read()))
    elif cmd == "vsh":
        for off, text in embedded_vertex_shaders(open(args[0], "rb").read()):
            print("######## file offset 0x%x\n%s\n" % (off, text.replace("\t", " ").rstrip()))
    elif cmd == "obj":
        cmd_obj(*args[:4])
    elif cmd == "hierarchy":
        cmd_hierarchy(args)
    elif cmd == "match":
        cmd_match(args[0], args[1])
    else:
        print(__doc__)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
