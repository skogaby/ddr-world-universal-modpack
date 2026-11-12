"""EXAMPLE: build a Blender armature + skinned meshes from a CyberConnect2 XFBIN character (JoJo's
Bizarre Adventure: Eyes of Heaven, PS4; the same code reads All Star Battle R). Used by
port_character_eoh.py; the decoder is scripts/xfbin_dump.py (format notes: docs/jojo_eoh_research.md).

A character is `<code>bod1.xfbin` (data/spc/PS4/: the body, face, eyes, teeth, tongue models and the
clump rig, with the `_m` / `_n` / iris textures) plus `<code>col<N>.xfbin` (data/spc/: colour set N's
`_d` diffuse and `_s` shadow sheets, referenced from the body file's materials as `t_<code>_body01_d_#0`).

Coordinates stay in the file's space (Z-up centimetres, facing -Y, left at +X: Blender's axes), so
the vertices need no conversion and the port's PRESCALE is metres per cm. Bones are placed at the
clump coords' world positions (trs chain, see xfbin_dump), tails toward MAIN_CHILD; the `<code>t0 `
name prefix is dropped ('pelvis', 'l upperarm', 'r cloth03', ...). Skinned meshes are in root space
and weight by coord index; an unskinned model (teeth) is authored in its coord's frame and is moved
there and weighted 1.0 to it.

Returns (armature, [mesh objects], info). Each mesh object gets one material slot per NUD mesh
named after the material chunk ('3jtr01bod1_skin'); the material carries `eoh_textures` = the
texture chunk names in the chunk's order (diffuse first) and `eoh_uv_count`; UV sets are
`UVMap` (set 0) and `UVMap1` (set 1 -- the eyes' iris projection, centred on 0 with wrap).
"""
import os
import sys

import bpy
from mathutils import Matrix, Vector

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS = os.path.normpath(os.path.join(_HERE, '..', '..', '..', 'scripts'))
if _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)
import xfbin_dump as X  # noqa: E402

# bone -> the child that sets its tail direction (Biped-style CC2 rig)
MAIN_CHILD = {
    'pelvis': 'spine', 'spine': 'spine1', 'spine1': 'spine2', 'spine2': 'neck', 'neck': 'head',
}
for _s in ('l', 'r'):
    MAIN_CHILD.update({
        _s + ' clavicle': _s + ' upperarm', _s + ' upperarm': _s + ' forearm', _s + ' forearm': _s + ' hand',
        _s + ' thigh': _s + ' calf', _s + ' calf': _s + ' foot', _s + ' foot': _s + ' toe0',
    })


# model names (after `short`) that are effects, alternates or duplicates, never part of the dancer
SKIP_MODEL_KEYS = ('_eff', 'eff_', 'bodyeffect', 'spiral', 'nobone', ' m2', 'teeth 02', 'face damage', 'face nofacial',
                   'ridebullet', 'arm cool', 'add arm', 'add leg', 'shadow', 'knife', 'missile', 'propeller')


def humanoid_missing(root, code):
    """The Biped joints port_character_eoh.py places that this body lacks ([] = portable)."""
    bx = X.read_xfbin(body_path(root, code))
    clumps = bx.by_kind('nuccChunkClump')
    clump = next((c for c in clumps if c.name == code + 'bod1'), None) or (max(clumps, key=lambda c: len(c.coords)) if clumps else None)
    if clump is None:
        return ['(no clump)']
    names = {short(c.name, code) for c in clump.coords}
    # spine1 / spine2 / neck / clavicle / toe0 are optional (Tusk ACT4 has no neck, Doppio no right
    # clavicle); the port builds its chains from what is there
    need = ['pelvis', 'spine', 'head']
    for sd in ('l', 'r'):
        need += [sd + ' upperarm', sd + ' forearm', sd + ' hand', sd + ' thigh', sd + ' calf', sd + ' foot']
    return [n for n in need if n not in names]


def short(name, code):
    """'3jtr01t0 l upperarm' -> 'l upperarm'; the root '3jtr01t0' -> 'root'."""
    pre = code + 't0'
    if name == pre:
        return 'root'
    if name.startswith(pre + ' '):
        return name[len(pre) + 1:]
    return name


def body_path(root, code):
    return os.path.join(root, 'dataRegion0', 'data', 'spc', 'PS4', code + 'bod1.xfbin')


def col_path(root, code, col):
    return os.path.join(root, 'data0', 'data', 'spc', '%scol%d.xfbin' % (code, col))


def _mat4(M):
    return Matrix([list(r) for r in M])


def build(root, code, name=None, skip_models=(), col=0):
    """Build the character from <root>/dataRegion0/.../<code>bod1.xfbin (+ col file for the diffuse)."""
    name = name or code
    bx = X.read_xfbin(body_path(root, code))
    clumps = bx.by_kind('nuccChunkClump')
    # a file may carry a lower LOD (`<code+1>bod1`) or a GHA arm clump beside the body
    clump = next((c for c in clumps if c.name == code + 'bod1'), None) or max(clumps, key=lambda c: len(c.coords))
    W = clump.world_matrices()
    names = [short(c.name, code) for c in clump.coords]
    assert len(set(names)) == len(names), 'duplicate coord names'
    world = {n: _mat4(W[c]) for n, c in zip(names, clump.coords)}
    parent = {n: (short(c.parent.name, code) if c.parent is not None else None) for n, c in zip(names, clump.coords)}

    # --- armature ---------------------------------------------------------------------------------------
    arm_data = bpy.data.armatures.new(name + '_src_rig')
    arm = bpy.data.objects.new(name + '_src_rig', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    eb = {}
    for n in names:
        e = arm_data.edit_bones.new(n)
        e.head = world[n].to_translation()
        eb[n] = e
    for n in names:
        e = eb[n]
        ch = MAIN_CHILD.get(n)
        head = e.head.copy()
        if ch in world and (world[ch].to_translation() - head).length > 1e-3:
            e.tail = world[ch].to_translation()
        else:
            # terminal: along the coord's local X (Biped bones point down +X), 3 cm
            d = (world[n].to_3x3() @ Vector((1.0, 0.0, 0.0))).normalized()
            if d.length < 0.5:
                d = Vector((0.0, 0.0, 1.0))
            e.tail = head + d * 3.0
        p = parent[n]
        if p in eb:
            e.parent = eb[p]
            e.use_connect = False
    bpy.ops.object.mode_set(mode='OBJECT')

    # --- meshes -----------------------------------------------------------------------------------------
    meshes = []
    info = {'code': code, 'coords': names, 'world': world, 'models': []}
    for mdl in clump.models:
        if not isinstance(mdl, X.ModelChunk):
            continue
        mname = short(mdl.name, code)
        if mname in skip_models or any(k in mname for k in SKIP_MODEL_KEYS):
            print('skip model', mname)
            continue
        groups = [(g, m) for g in mdl.nud.mesh_groups for m in g.meshes]
        verts, faces, loops_uv0, loops_uv1, loops_n, poly_mat, weights = [], [], [], [], [], [], []
        base = 0
        slot_names = []
        attach = None
        if not any(m.bone_ids for _, m in groups):
            # unskinned: vertices are in the model coord's frame
            attach = short(mdl.coord.name, code) if mdl.coord is not None else None
        for si, (g, m) in enumerate(groups):
            matc = mdl.materials[si] if si < len(mdl.materials) else (mdl.materials[-1] if mdl.materials else None)
            slot_names.append(matc.name if matc else '%s_slot%d' % (mname, si))
            T = world[attach] if attach else Matrix.Identity(4)
            for i, p in enumerate(m.positions):
                verts.append(T @ Vector(p))
                if m.bone_ids:
                    weights.append([(names[b], w) for b, w in zip(m.bone_ids[i], m.bone_weights[i]) if w > 0.0 and b < len(names)])
                else:
                    weights.append([(attach, 1.0)] if attach else [])
            R = T.to_3x3()
            for (a, b, c) in m.faces:
                faces.append((base + a, base + b, base + c))
                poly_mat.append(si)
                for k in (a, b, c):
                    uv = m.uvs[k] if m.uvs else []
                    loops_uv0.append((uv[0][0], 1.0 - uv[0][1]) if len(uv) > 0 else (0.0, 0.0))
                    loops_uv1.append((uv[1][0], 1.0 - uv[1][1]) if len(uv) > 1 else (0.0, 0.0))
                    nrm = m.normals[k]
                    loops_n.append((R @ Vector(nrm)).normalized() if nrm else Vector((0, 0, 1)))
            base += len(m.positions)
        if not faces:
            continue
        me = bpy.data.meshes.new(name + '_' + mname)
        me.from_pydata([tuple(v) for v in verts], [], faces)
        me.update()
        ob = bpy.data.objects.new(name + '_' + mname, me)
        bpy.context.scene.collection.objects.link(ob)
        uv0 = me.uv_layers.new(name='UVMap')
        uv1 = me.uv_layers.new(name='UVMap1')
        flat0 = [c for uv in loops_uv0 for c in uv]
        flat1 = [c for uv in loops_uv1 for c in uv]
        if len(me.loops) == len(loops_uv0):
            uv0.data.foreach_set('uv', flat0)
            uv1.data.foreach_set('uv', flat1)
            me.normals_split_custom_set(loops_n)
        else:
            print('WARN %s: %d loops for %d corners (degenerate faces dropped); UVs by polygon' % (mname, len(me.loops), len(loops_uv0)))
            # from_pydata drops degenerate triangles: rebuild per polygon from its vertex indices
            lookup = {}
            for fi, f in enumerate(faces):
                for k, vi in enumerate(f):
                    lookup[(fi, vi)] = (loops_uv0[3 * fi + k], loops_uv1[3 * fi + k], loops_n[3 * fi + k])
            # polygon order is kept for the surviving faces
            kept = [fi for fi, f in enumerate(faces) if len(set(f)) == 3]
            nl = []
            for pi, poly in enumerate(me.polygons):
                fi = kept[pi]
                for li in poly.loop_indices:
                    vi = me.loops[li].vertex_index
                    u0, u1, nn = lookup[(fi, vi)]
                    uv0.data[li].uv = u0
                    uv1.data[li].uv = u1
                    nl.append(nn)
            me.normals_split_custom_set(nl)
            poly_mat = [poly_mat[fi] for fi in kept]
        for sn in slot_names:
            mat = bpy.data.materials.get(sn) or bpy.data.materials.new(sn)
            me.materials.append(mat)
        for p, mi in zip(me.polygons, poly_mat):
            p.material_index = mi
        me.update()
        # material metadata: texture chunk names in order (the diffuse `_d_#N` first on the body)
        for si, sn in enumerate(slot_names):
            matc = mdl.materials[si] if si < len(mdl.materials) else None
            if matc is not None:
                me.materials[si]['eoh_textures'] = [t.name for t in matc.textures()]
                me.materials[si]['eoh_cull'] = groups[si][1].materials[0].cull_mode if groups[si][1].materials else 0
        vgs = {}
        for i, ws in enumerate(weights):
            for bn, w in ws:
                vg = vgs.get(bn)
                if vg is None:
                    vg = vgs[bn] = ob.vertex_groups.new(name=bn)
                vg.add([i], w, 'ADD')
        ob.parent = arm
        mod = ob.modifiers.new('Armature', 'ARMATURE')
        mod.object = arm
        meshes.append(ob)
        info['models'].append((mname, mdl.rigging_flag, attach, slot_names, len(verts), len(faces)))
        print('model %-16s rig=%3d attach=%-12s verts=%5d tris=%5d slots=%s' % (mname, mdl.rigging_flag, attach, len(verts), len(faces), slot_names))
    bpy.context.view_layer.update()
    return arm, meshes, info


def roll_half_dds(dds):
    """Shift a DXT DDS by half its width and height (the NTP3 iris sheets keep the iris at UV (0, 0)
    and rely on wrap; after the roll it sits in the centre). Block-aligned, so lossless; mip levels
    narrower than 8 px are dropped."""
    import struct
    h = dds[:128]
    height, width = struct.unpack('<II', h[12:20])
    mips = struct.unpack('<I', h[28:32])[0] or 1
    fourcc = h[84:88]
    bb = 8 if fourcc == b'DXT1' else 16
    out = []
    off = 128
    w, hh = width, height
    kept = 0
    for _ in range(mips):
        bw, bh = max(1, w // 4), max(1, hh // 4)
        n = bw * bh * bb
        lv = dds[off:off + n]
        off += n
        if w >= 8 and hh >= 8:
            rows = [lv[r * bw * bb:(r + 1) * bw * bb] for r in range(bh)]
            rows = rows[bh // 2:] + rows[:bh // 2]
            rows = [r[(bw // 2) * bb:] + r[:(bw // 2) * bb] for r in rows]
            out.append(b''.join(rows))
            kept += 1
        w, hh = max(1, w // 2), max(1, hh // 2)
    hdr = bytearray(h)
    hdr[28:32] = struct.pack('<I', kept)
    return bytes(hdr) + b''.join(out)
