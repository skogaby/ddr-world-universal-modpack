"""EXAMPLE: build a Blender armature + skinned mesh from a GTA-style character rip saved as ASCII
FBX 6.1 (3ds Max / FBX SDK 2011) — Blender's FBX importer rejects ASCII files. Used by
port_character_gta_fbx6.py; written for the GTA San Andreas Carl Johnson rip (2026-09-26).

Coordinates stay in the source's 3ds Max space (Z-up inches, character facing -Y) — the same axes as
Blender, so the vertices need no conversion; the skin clusters' TransformLink matrices are FBX global
(Y-up) and are rotated back. Bones are placed from the clusters' TransformLink (the bind pose);
MAIN_CHILD below names the rig's bones (XNALara-style GTA names: 'spine upper', 'arm left shoulder 1'
…) — adapt it for another skeleton. Returns (armature, mesh object, parsed FBX dict).
"""
import os
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fbx6_ascii  # noqa: E402

# FBX global (Y-up) -> Max / Blender (Z-up): +90 deg about X
Y_UP_TO_Z_UP = Matrix.Rotation(1.5707963267948966, 4, 'X')

# main child that sets a bone's tail (and so its Y axis); terminal bones get an explicit direction
MAIN_CHILD = {
    'root ground': 'root hips', 'root hips': 'spine lower', 'pelvis': None,
    'spine lower': 'spine middle', 'spine middle': 'spine upper', 'spine upper': 'head neck lower',
    'head neck lower': 'head neck upper', 'head jaw': None,
}
for _s in ('left', 'right'):
    MAIN_CHILD.update({
        'leg %s thigh' % _s: 'leg %s knee' % _s, 'leg %s knee' % _s: 'leg %s ankle' % _s,
        'leg %s ankle' % _s: 'leg %s toes' % _s,
        'arm %s shoulder 1' % _s: 'arm %s shoulder 2' % _s, 'arm %s shoulder 2' % _s: 'arm %s elbow' % _s,
        'arm %s elbow' % _s: 'arm %s wrist' % _s, 'arm %s wrist' % _s: 'arm %s finger 2a' % _s,
        'arm %s finger 2a' % _s: 'arm %s finger 2b' % _s, 'arm %s finger 2b' % _s: 'arm %s finger 2c' % _s,
        'arm %s finger 1a' % _s: 'arm %s finger 1b' % _s, 'arm %s finger 1b' % _s: 'arm %s finger 1c' % _s,
    })


def _m(rows):
    return Matrix([list(r) for r in rows])


def build(path, name='CJ'):
    d = fbx6_ascii.load(path)
    parent = d['parent']
    bones = [n for n, m in d['models'].items() if m['type'] == 'LimbNode']
    cl_of_bone = {b: c for c, b in d['cluster_bone'].items()}
    heads = {}
    axes = {}
    for b in bones:
        c = d['clusters'].get(cl_of_bone.get(b))
        if c is None or c['transform_link'] is None:
            continue
        TL = Y_UP_TO_Z_UP @ _m(c['transform_link'])
        heads[b] = TL.to_translation()
        axes[b] = TL.to_3x3().normalized()
        # consistency: the inverse of Transform should give the same origin in mesh space
        if c['transform'] is not None:
            alt = _m(c['transform']).inverted().to_translation()
            if (alt - heads[b]).length > 1e-2:
                print('NOTE cluster %s: inv(Transform) origin %s vs TransformLink %s' % (b, tuple(round(x, 3) for x in alt),
                                                                                         tuple(round(x, 3) for x in heads[b])))
    # weightless bones (root hips): take the first placed child's head
    for b in bones:
        if b not in heads:
            kids = [k for k in bones if parent.get(k) == b and k in heads]
            heads[b] = heads[kids[0]].copy() if kids else Vector()
            print('bone %s has no cluster: head from child %s' % (b, kids[:1]))
    # armature
    arm_data = bpy.data.armatures.new(name + '_src_rig')
    arm = bpy.data.objects.new(name + '_src_rig', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    eb = {}
    for b in bones:
        e = arm_data.edit_bones.new(b)
        e.head = heads[b]
        eb[b] = e
    for b in bones:
        e = eb[b]
        ch = MAIN_CHILD.get(b)
        if ch and ch in heads and (heads[ch] - heads[b]).length > 1e-4:
            e.tail = heads[ch]
        else:
            # terminal bone: continue the parent's direction (fingers, toes) or point up (head)
            p = parent.get(b)
            if b in ('head neck upper', 'pelvis', 'head jaw') or b.startswith('head '):
                direc = Vector((0, 0, 1)) if b != 'pelvis' else Vector((0, 0, -1))
                ln = 6.0 if b == 'head neck upper' else 1.0
            elif b.startswith('leg') and b.endswith('toes'):
                direc = Vector((0, -1, 0))
                ln = 3.0
            elif p in heads and (heads[b] - heads[p]).length > 1e-4:
                direc = (heads[b] - heads[p]).normalized()
                ln = 1.0
            else:
                direc = Vector((0, 0, 1))
                ln = 1.0
            e.tail = heads[b] + direc * ln
        p = parent.get(b)
        if p in eb:
            e.parent = eb[p]
            e.use_connect = False
    bpy.ops.object.mode_set(mode='OBJECT')
    # mesh
    v = d['vertices']
    co = [Vector((v[3 * i], v[3 * i + 1], v[3 * i + 2])) for i in range(len(v) // 3)]
    me = bpy.data.meshes.new(name + '_body')
    me.from_pydata([tuple(p) for p in co], [], [tuple(p) for p in d['polygons']])
    me.update()
    assert len(me.polygons) == len(d['polygons'])
    # UVs (ByPolygonVertex / IndexToDirect)
    uvd = d['uv']
    uvl = me.uv_layers.new(name='UVMap')
    uv_flat = uvd['uv']
    idx = uvd['index']
    for p in me.polygons:
        for li in p.loop_indices:
            k = idx[li] if uvd['ref'] == 'IndexToDirect' else li
            uvl.data[li].uv = (uv_flat[2 * k], uv_flat[2 * k + 1])
    # materials
    mats = []
    for mn in d['materials']:
        m = bpy.data.materials.get(mn) or bpy.data.materials.new(mn)
        mats.append(m)
        me.materials.append(m)
    ml = d['material_layer']
    if ml['mapping'] == 'ByPolygon':
        for p, mi in zip(me.polygons, ml['materials']):
            p.material_index = mi
    elif ml['mapping'] == 'AllSame':
        for p in me.polygons:
            p.material_index = ml['materials'][0]
    # custom split normals (ByPolygonVertex / Direct), in mesh space
    nd = d['normals']
    if nd and nd['mapping'] == 'ByPolygonVertex' and nd['ref'] == 'Direct':
        nn = nd['normals']
        if len(nn) == 3 * len(me.loops):
            me.normals_split_custom_set([Vector((nn[3 * i], nn[3 * i + 1], nn[3 * i + 2])).normalized() for i in range(len(me.loops))])
    me.update()
    ob = bpy.data.objects.new(name + '_body', me)
    bpy.context.scene.collection.objects.link(ob)
    # skin weights
    for cname, c in d['clusters'].items():
        b = d['cluster_bone'].get(cname)
        if not b or not c['indexes']:
            continue
        vg = ob.vertex_groups.get(b) or ob.vertex_groups.new(name=b)
        for i, w in zip(c['indexes'], c['weights']):
            if w > 0:
                vg.add([i], w, 'ADD')
    ob.parent = arm
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    bpy.context.view_layer.update()
    return arm, ob, d
