"""EXAMPLE: give an already-exported dancer (`pl_<key>.model`, no source rip at hand) a dynamic
chain for the Background Dancers solver (`dynbones_logic.rs`) -- written 2026-10-26 for EoH
Joseph Joestar's scarf, whose tail EoH authors streaming straight back from the neck in the
T-pose (port_character_eoh.py's DYN_CHAINS does this from the source bones; here the chain is
fitted to the geometry instead).

  1. import the .model (rest = bind) and pick the chain's vertices by a box (`select`);
  2. fit joints along the strip: slice it into `segments` along its long axis, joint i = the
     centroid of the slice at that station;
  3. re-pose it to hang (`dirs`, Blender axes, per segment), moving vertices AND their exact
     normals (`ddr_normal`) by the blend of their two nearest segments' rigid transforms -- that
     pose is the solver's rest;
  4. append `DB_*` bones at the posed joints (game +x = the segment), weight the strip along them
     (a short blend back into the original weights at the root), export over the original
     folder (OUT) and write `dynbones.txt`;
  5. checks: bone count, weight sums, codec round trip, mesh / material set unchanged, previews.

Run: IN=<dir with pl_<key>.model> OUT=<dir> CHAIN=joseph \
     /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup --python \
     tools/blender_ddr_addon/examples/add_dynchain_dancer.py
"""
import math
import os
import shutil
import sys

import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import port_lib as P  # noqa: E402
from blender_ddr_addon import export_model, import_model  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

CHAINS = {
    # EoH 2jsp01: the scarf tail (mesh0, weighted to Neck) runs +Y (back) from y 0.17 to 0.93 m at
    # x -0.21..-0.13, z 1.36..1.43 -- re-posed down the back like Dio's muffler, same solver feel
    'joseph': dict(
        key='eoh2jsp01', parent='Neck', name='scarf',
        select=dict(axis=1, start=0.17, end=1.2, box_min=(-0.30, 0.17, 1.25), box_max=(-0.05, 1.2, 1.60)),
        segments=4, blend=0.05,
        dirs=[(0.0, 0.5, -0.87), (0.0, 0.15, -0.99), (0.0, 0.05, -1.0), (0.0, 0.03, -1.0)],
        stiffness=0.08, damping=0.25, limit=0.9, gravity=1,
        boxes=[dict(id=1, joint='Spine', min=(-0.08, -0.15, -0.22), max=(0.42, 0.15, 0.22))]),
}

IN = os.path.expanduser(os.environ['IN'])
OUT = os.path.expanduser(os.environ['OUT'])
C = CHAINS[os.environ.get('CHAIN', 'joseph')]
KEY = C['key']
PREVIEW = os.environ.get('PREVIEW', '1') == '1'


def rotation_between(a, b):
    return Vector(a).normalized().rotation_difference(Vector(b).normalized())


def main():
    P.fresh_scene()
    src = os.path.join(IN, 'pl_%s.model' % KEY)
    arm, objs = import_model.load_model(src, import_textures=True, with_armature=True)
    order = list(arm['ddr_bone_order'])
    assert len(order) == 33 and not any(n.startswith('DB_') for n in order), order
    before = K.parse_model(open(src, 'rb').read())
    sel = C['select']
    ax = sel['axis']
    lo, hi = Vector(sel['box_min']), Vector(sel['box_max'])

    # 1. the strip's vertices (world = armature space; import_model leaves the meshes unparented
    #    transforms at identity, asserted)
    picked = {}
    for o in objs:
        assert o.matrix_world == Matrix.Identity(4), o.name
        ids = [v.index for v in o.data.vertices if all(lo[i] <= v.co[i] <= hi[i] for i in range(3))]
        if ids:
            picked[o.name] = ids
    n_pick = sum(len(v) for v in picked.values())
    assert n_pick > 50, picked
    pts = np.array([list(bpy.data.objects[n].data.vertices[i].co) for n, ids in picked.items() for i in ids])

    # 2. joints: stations along the long axis, centroid of the vertices near each station
    a0, a1 = float(pts[:, ax].min()), float(pts[:, ax].max())
    stations = np.linspace(a0, a1, C['segments'] + 1)
    half = (a1 - a0) / C['segments'] / 2.0
    joints = []
    for s in stations:
        near = pts[np.abs(pts[:, ax] - s) <= half * 0.6]
        c = near.mean(axis=0) if len(near) else pts[np.argmin(np.abs(pts[:, ax] - s))]
        c[ax] = s
        joints.append(Vector(c))
    seg_len = [(joints[i + 1] - joints[i]).length for i in range(len(joints) - 1)]
    print('JOINTS', [tuple(round(c, 3) for c in j) for j in joints], 'segments', [round(x, 3) for x in seg_len])

    # 3. the drape: posed joints + one rigid transform per segment (rotate about the joint)
    posed = [joints[0].copy()]
    rots = []
    for i, d in enumerate(C['dirs'][:len(seg_len)]):
        q = rotation_between(joints[i + 1] - joints[i], d)
        rots.append(q)
        posed.append(posed[i] + Vector(d).normalized() * seg_len[i])
    xforms = [Matrix.Translation(posed[i]) @ rots[i].to_matrix().to_4x4() @ Matrix.Translation(-joints[i]) for i in range(len(seg_len))]

    def seg_param(co):
        """(segment index, fraction in it) of a rest position, by its long-axis coordinate."""
        t = (co[ax] - a0) / max(a1 - a0, 1e-9) * len(seg_len)
        i = min(len(seg_len) - 1, max(0, int(math.floor(t))))
        return i, min(1.0, max(0.0, t - i))

    def blend_xform(co):
        """Vertex transform: segment i's, eased into i+1's over the segment's last 30 %."""
        i, f = seg_param(co)
        if i + 1 < len(seg_len) and f > 0.7:
            w = (f - 0.7) / 0.3
            qa, qb = rots[i], rots[i + 1]
            pa, pb = xforms[i] @ Vector(co), xforms[i + 1] @ Vector(co)
            return pa.lerp(pb, w), qa.slerp(qb, w)
        return xforms[i] @ Vector(co), rots[i]

    # 4a. bones
    bpy.context.view_layer.objects.active = arm
    for o in bpy.context.view_layer.objects:
        o.select_set(o == arm)
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = arm.data.edit_bones
    names = ['DB_%s_%02d' % (C['name'], i + 1) for i in range(len(seg_len))]
    prev = None
    for i, n in enumerate(names):
        x = (posed[i + 1] - posed[i]).normalized()
        up = Vector((0.0, 0.0, 1.0)) if abs(x.z) < 0.9 else Vector((0.0, -1.0, 0.0))
        z = x.cross(up).normalized()
        y = z.cross(x).normalized()
        p = posed[i]
        e = ebs.new(n)
        e.head = p
        e.tail = p + y * max(seg_len[i], 0.02)
        e.matrix = Matrix(((x.x, y.x, z.x, p.x), (x.y, y.y, z.y, p.y), (x.z, y.z, z.z, p.z), (0.0, 0.0, 0.0, 1.0)))
        e.use_connect = False
        e.parent = ebs[prev] if prev else ebs[C['parent']]
        e.inherit_scale = 'NONE'
        prev = n
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = order + names

    # 3b + 4b. move the strip and weight it along the chain
    moved = 0
    for oname, ids in picked.items():
        o = bpy.data.objects[oname]
        me = o.data
        groups = {vg.name: vg for vg in o.vertex_groups}
        for n in names:
            groups[n] = o.vertex_groups.new(name=n)
        nrm_attr = me.attributes.get('ddr_normal')
        for vi in ids:
            v = me.vertices[vi]
            co = v.co.copy()
            new_co, q = blend_xform(co)
            v.co = new_co
            if nrm_attr is not None:
                nv = Vector(nrm_attr.data[vi].vector)
                nrm_attr.data[vi].vector = (q @ nv).normalized()
            i, f = seg_param(co)
            # joint-centred weights: bone i owns its segment, easing into bone i+1 past mid-segment
            ws = {names[i]: 1.0}
            if i + 1 < len(names) and f > 0.5:
                w = (f - 0.5) / 0.5 * 0.5
                ws = {names[i]: 1.0 - w, names[i + 1]: w}
            # the root's first `blend` metres ease back into the original (Neck) weights
            dist = co[ax] - a0
            keep = 0.0 if dist >= C['blend'] else 1.0 - dist / C['blend']
            orig = [(o.vertex_groups[g.group].name, g.weight) for g in v.groups if g.weight > 0.0]
            for g in list(v.groups):
                o.vertex_groups[g.group].remove([vi])
            out = {n: w * (1.0 - keep) for n, w in ws.items()}
            tot_o = sum(w for _, w in orig) or 1.0
            for n, w in orig:
                out[n] = out.get(n, 0.0) + keep * w / tot_o
            top = sorted(((n, w) for n, w in out.items() if w > 1e-4), key=lambda kv: -kv[1])[:4]
            tot = sum(w for _, w in top)
            for n, w in top:
                groups[n].add([vi], w / tot, 'REPLACE')
            moved += 1
        me.update()
    print('MOVED', moved, 'vertices of', {k: len(v) for k, v in picked.items()})

    # 5. export over a fresh folder: the model + the original textures
    tmp = OUT + '.new'
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for f in os.listdir(IN):
        if f.lower().endswith('.dds'):
            shutil.copy(os.path.join(IN, f), os.path.join(tmp, f))
    bpy.context.view_layer.update()
    written, spec = export_model.export_model(os.path.join(tmp, 'pl_%s.model' % KEY), arm, objs, write_textures=False, raw_axes=False)
    data = open(os.path.join(tmp, 'pl_%s.model' % KEY), 'rb').read()
    m = K.parse_model(data)
    assert len(m['bones']) == 33 + len(names), len(m['bones'])
    assert [b['identity'] for b in m['bones'][:33]] == [b['identity'] for b in before['bones']], 'DDR bones changed'
    assert sorted(mm['identity'] for mm in m['materials']) == sorted(mm['identity'] for mm in before['materials'])
    assert sorted(t['name'] for t in m['textures']) == sorted(t['name'] for t in before['textures'])
    bad = sum(1 for me in m['meshes'] for v in K.read_vertices(m, me) if abs(sum(v['WEIGHTS4']) - 1.0) > 2e-3)
    assert bad == 0, bad
    assert K.write_model(K.model_to_spec(m)) == data, 'codec round trip'
    nv0 = sum(len(K.read_vertices(before, me)) for me in before['meshes'])
    nv1 = sum(len(K.read_vertices(m, me)) for me in m['meshes'])
    print('MODEL bones %d (+%d), meshes %d -> %d, vertices %d -> %d' % (len(m['bones']), len(names), len(before['meshes']), len(m['meshes']), nv0, nv1))
    lens = seg_len + [seg_len[-1]]
    axis = '1,1,1,%.6f,%.6f,%.6f,%.6f' % (-C['limit'], C['limit'], C['stiffness'], C['damping'])
    lines = ['# JoJo EoH %s: dynamic chain for the Background Dancers solver (dynbones_logic.rs)' % KEY[3:],
             '# added to the exported model by tools/blender_ddr_addon/examples/add_dynchain_dancer.py -- do not edit',
             'dynbones 1', 'units_per_metre 8.858763']
    parent = C['parent']
    excl = ','.join(str(b['id']) for b in C['boxes']) or '-'
    for i, n in enumerate(names):
        lines.append('bone %s parent=%s len=%.6f gravity=%d z=%s y=%s exclude=%s' % (
            n, parent, lens[i], C['gravity'], axis, axis, excl if i == 0 else '-'))
        parent = n
    for b in C['boxes']:
        lines.append('box %d joint=%s min=%s max=%s' % (b['id'], b['joint'], ','.join('%.6f' % v for v in b['min']),
                                                     ','.join('%.6f' % v for v in b['max'])))
    open(os.path.join(tmp, 'dynbones.txt'), 'w').write('\n'.join(lines) + '\n')
    shutil.rmtree(OUT, ignore_errors=True)
    os.rename(tmp, OUT)
    print('WROTE', sorted(os.listdir(OUT)))
    if PREVIEW:
        P.fresh_scene()
        import_model.load_model(os.path.join(OUT, 'pl_%s.model' % KEY), import_textures=True, with_armature=True)
        P.studio()
        for tag, pos in (('front', (0, -4, 1.0)), ('back', (0, 4, 1.0)), ('side', (4, 0, 1.0))):
            P.render_camera(os.path.join(os.path.dirname(OUT), '%s_dyn_%s.png' % (KEY, tag)), Vector(pos), Vector((0, 0, 1.0)), 2.4, res=(600, 700))
    print('DONE')


main()
