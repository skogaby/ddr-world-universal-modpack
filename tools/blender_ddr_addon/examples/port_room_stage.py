"""EXAMPLE: port a room/stage .blend (plain-colour materials, arbitrary scale) to a DDR stage part model.

Written for the Griffin living room (in-game verified on DDR A3, 2026-09-15); copy and adapt.
Inputs (environment): SRC_BLEND, OUT_DIR, STAGE_KEY (default griffin00), PART (default room),
ROOM_SCALE (default 0.135), ORIGIN_SRC ("x,y,z" in source units — the spot the dancer stands on).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --python examples/port_room_stage.py
What it does:
  * evaluate every mesh with its modifiers (GN etc.), bake world transforms, join into ONE mesh
  * room -> game scale/position: scale S, translated so the spot between the couch and the TV
    (the rug) is the origin; the DDR dancer stands at the origin facing -Y (Blender), the TV is
    at -Y and the couch at +Y in the source file already
  * plain-colour materials -> one 128x128 palette texture (8x8 swatches, sRGB bytes) with every
    face's UVs collapsed onto its swatch; textured materials keep their image (resized to POT,
    renamed lr_*); ONE white colour attribute (stock layout B + mdl_bg_constant_vc)
  * export via the add-on as gm_griffin00_room (static, root bone synthesized)
"""
import math
import os
import sys

import bpy
from mathutils import Matrix, Vector

SRC = os.environ['SRC_BLEND']
OUT = os.environ['OUT_DIR']
KEY = os.environ.get('STAGE_KEY', 'griffin00')
PART = os.environ.get('PART', 'room')
MODEL = 'gm_%s_%s' % (KEY, PART)
EXPORT = os.path.join(OUT, 'export', MODEL)
os.makedirs(EXPORT, exist_ok=True)
ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
addon.register()
from blender_ddr_addon import export_model  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

S = float(os.environ.get('ROOM_SCALE', '0.135'))
# source-space point that becomes the origin (the living room: midway between the couch front
# y -0.9 and the TV stand front y -15.3, on their centre line x -3.65, floor at z 0)
ORIGIN_SRC = Vector([float(c) for c in os.environ.get('ORIGIN_SRC', '-3.65,-8.1,0').split(',')])
XFORM = Matrix.Scale(S, 4) @ Matrix.Translation(-ORIGIN_SRC)
TEX_RENAME = {'CHris.png': 'lr_chris', 'MEg.png': 'lr_meg', 'STewie.png': 'lr_stewie', 'mountains.png': 'lr_mount',
              'painting.png': 'lr_paint1', 'painting2.png': 'lr_paint2', 'PeterandLois.png': 'lr_lois',
              'radio.png.001': 'lr_radio', 'Windows.png': 'lr_win', 'smalltable.png': 'lr_table'}


def srgb(c):
    return 12.92 * c if c <= 0.0031308 else 1.055 * (c ** (1 / 2.4)) - 0.055


def pot(n, lo=8, hi=512):
    return max(lo, min(hi, 2 ** int(round(math.log2(max(1, n))))))


bpy.ops.wm.open_mainfile(filepath=SRC)
dg = bpy.context.evaluated_depsgraph_get()
src_objs = [o for o in bpy.data.objects if o.type == 'MESH']
print('source meshes', len(src_objs))

# --- 1. evaluated copies, world transform + room transform baked --------------------------------
coll = bpy.data.collections.new('room_export')
bpy.context.scene.collection.children.link(coll)
parts = []
for o in src_objs:
    ev = o.evaluated_get(dg)
    me = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=True, depsgraph=dg)
    me.name = 'X_' + o.name
    me.transform(XFORM @ o.matrix_world)
    n = bpy.data.objects.new('X_' + o.name, me)
    coll.objects.link(n)
    parts.append(n)
    print('  %-16s polys %5d (base %5d) mats %s' % (o.name, len(me.polygons), len(o.data.polygons), [m.name if m else None for m in me.materials]))

# --- 2. join into one object -----------------------------------------------------------------
for o in bpy.data.objects:
    o.select_set(False)
for p in parts:
    p.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
with bpy.context.temp_override(active_object=parts[0], selected_objects=parts, selected_editable_objects=parts):
    bpy.ops.object.join()
room = parts[0]
room.name = MODEL
room.data.name = room.name
me = room.data
print('joined: verts %d polys %d materials %d' % (len(me.vertices), len(me.polygons), len(me.materials)))
xs = [v.co.x for v in me.vertices]; ys = [v.co.y for v in me.vertices]; zs = [v.co.z for v in me.vertices]
print('room extents (m): x %.2f..%.2f  y %.2f..%.2f  z %.2f..%.2f' % (min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))

# --- 3. materials: palette for plain colours, resized images for the rest ----------------------
palette_mats = []
textured = {}
for i, mat in enumerate(me.materials):
    img = None
    if mat and mat.use_nodes:
        img = next((n.image for n in mat.node_tree.nodes if n.type == 'TEX_IMAGE' and n.image), None)
    if img is not None:
        textured[i] = img
    else:
        palette_mats.append(i)
assert len(palette_mats) <= 64, len(palette_mats)
PAL, SW = 128, 16  # 8x8 swatches of 16 px
pal = bpy.data.images.new('lr_palette', PAL, PAL, alpha=True)
px = [0.0] * (PAL * PAL * 4)
swatch_uv = {}
for k, mi in enumerate(palette_mats):
    mat = me.materials[mi]
    base = (0.8, 0.8, 0.8, 1.0)
    if mat and mat.use_nodes:
        bsdf = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
        if bsdf:
            base = tuple(bsdf.inputs['Base Color'].default_value)
        elif mat.node_tree.nodes.get('Emission'):
            base = tuple(mat.node_tree.nodes['Emission'].inputs['Color'].default_value)
    elif mat:
        base = tuple(mat.diffuse_color)
    col = [srgb(max(0.0, min(1.0, c))) for c in base[:3]] + [1.0]
    sx, sy = (k % 8) * SW, (k // 8) * SW  # sy counted from the TOP row of the texture
    for yy in range(SW):
        row = PAL - 1 - (sy + yy)  # Blender pixel rows are bottom-first
        for xx in range(SW):
            o = (row * PAL + sx + xx) * 4
            px[o:o + 4] = col
    # Blender UV: v=0 is the bottom row; the exporter writes v_file = 1 - v
    swatch_uv[mi] = ((sx + SW / 2.0) / PAL, 1.0 - (sy + SW / 2.0) / PAL)
    print('  swatch %2d <- %-18s rgb %s' % (k, mat.name if mat else None, tuple(round(c, 3) for c in col[:3])))
pal.pixels.foreach_set(px)
pal.pack()
for mi, img in textured.items():
    new = TEX_RENAME.get(img.name)
    if new is None:
        new = 'lr_' + ''.join(ch for ch in os.path.splitext(img.name)[0].lower() if ch.isalnum())[:12]
    if img.name != new and bpy.data.images.get(new) is None:
        w, h = img.size
        img.scale(pot(w), pot(h))
        img.name = new
        print('  texture %-12s -> %dx%d' % (new, *img.size))

# one material per KTMDL mesh: the palette material replaces every plain-colour slot
pal_mat = bpy.data.materials.new('lr_palette_mat')
pal_mat.use_nodes = True
tex = pal_mat.node_tree.nodes.new('ShaderNodeTexImage'); tex.image = pal; tex.interpolation = 'Closest'
pal_mat.node_tree.links.new(tex.outputs['Color'], pal_mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
uv = me.uv_layers.active or me.uv_layers.new(name='UVMap')
for poly in me.polygons:
    if poly.material_index in swatch_uv:
        u, v = swatch_uv[poly.material_index]
        for li in poly.loop_indices:
            uv.data[li].uv = (u, v)
pal_slot = palette_mats[0]
me.materials[pal_slot] = pal_mat
for poly in me.polygons:
    if poly.material_index in swatch_uv:
        poly.material_index = pal_slot
for mi in textured:
    mat = me.materials[mi]
    mat.use_nodes = True
    nt = mat.node_tree
    tex = next(n for n in nt.nodes if n.type == 'TEX_IMAGE' and n.image)
    tex.interpolation = 'Closest' if tex.image.size[0] <= 16 else 'Linear'
# remove now-unused slots (keeps indices consistent by rebuilding)
used = sorted({p.material_index for p in me.polygons})
remap = {old: new for new, old in enumerate(used)}
mats = [me.materials[i] for i in used]
new_idx = [remap[p.material_index] for p in me.polygons]
me.materials.clear()          # NOTE: clear() resets every polygon's material_index to 0 ...
for m in mats:
    me.materials.append(m)
me.polygons.foreach_set('material_index', new_idx)   # ... so re-apply the remapped indices afterwards
for m in me.materials:
    m.surface_render_method = 'DITHERED'
    m.use_backface_culling = False   # two-sided: interior walls / single-sided furniture stay visible
    if m.name.lower().startswith('ceiling'):
        m.use_backface_culling = True   # see-through from above (camera hack)
print('final material slots', [m.name for m in me.materials])
for ca in list(me.color_attributes):
    me.color_attributes.remove(ca)
colattr = me.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
colattr.data.foreach_set('color', [1.0] * (4 * len(colattr.data)))

# drop everything else from the scene (lights, cameras, originals) and save
for o in list(bpy.data.objects):
    if o is not room:
        bpy.data.objects.remove(o, do_unlink=True)
room.matrix_world = Matrix.Identity(4)
bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, MODEL + '.blend'))

# --- 4. export ---------------------------------------------------------------------------------
written, spec = export_model.export_model(os.path.join(EXPORT, MODEL + '.model'), None, [room], write_textures=True)
print('EXPORT written', [os.path.basename(w) for w in written])
data = open(os.path.join(EXPORT, MODEL + '.model'), 'rb').read()
m = K.parse_model(data)
print('MODEL bones', len(m['bones']), 'meshes', len(m['meshes']), 'bytes', len(data),
      'tris', sum(me_['index_buffers'][0]['count'] // 3 for me_ in m['meshes']),
      'shaders', sorted({mt['shader'] for mt in m['materials']}), 'textures', [t['name'] for t in m['textures']])
assert K.write_model(K.model_to_spec(m)) == data
