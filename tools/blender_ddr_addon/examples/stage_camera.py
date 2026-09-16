"""EXAMPLE: compose an exported stage + an exported character + the stock foot panel, render the
candidate camera shots, and (with --export-cam) write the song camera as music_<song>.camanm
(frames 0..6238 = the HOW TO PLAY lesson length; a hard cut = two keys one frame apart).
Inputs (environment): DDR_3D_DATA, STAGE_MODEL (the exported gm_*.model), CHARA_MODEL (the exported
pl_*.model), OUT_DIR. Written for the Griffin living room; edit SHOTS for another room.
Run: Blender -b --python examples/stage_camera.py -- --export-cam"""
import os, sys, math, bpy
from mathutils import Vector, Matrix
OUT = os.environ['OUT_DIR']
ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
sys.path.insert(0, os.path.dirname(ADDON_DIR)); addon = __import__(os.path.basename(ADDON_DIR)); addon.register()
from blender_ddr_addon import import_model, import_character, import_anm, export_anm
DATA = os.environ['DDR_3D_DATA']
EXPORT_CAM = '--export-cam' in sys.argv
POSE_FRAME = int(os.environ.get('POSE_FRAME', '2000'))

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
import_model.load_model(os.environ['STAGE_MODEL'], import_textures=True, with_armature=False)
import_model.load_model(os.path.join(DATA, 'map', 'gm_boom00_footpanel', 'gm_boom00_footpanel.model'), import_textures=True, with_armature=False)
arm, body, parts, info = import_character.load_character(os.environ['CHARA_MODEL'], import_textures=True)
bpy.context.view_layer.objects.active = arm
import_anm.load_anm(os.path.join(DATA, 'chara', 'mc_male_lesa', 'mc_male_lesa_lesa_exec.anm'), arm, frame_step=40)
sc.frame_set(POSE_FRAME)

sc.render.engine = 'BLENDER_WORKBENCH'; sc.display.shading.light = 'FLAT'; sc.display.shading.color_type = 'TEXTURE'
sc.display.shading.show_backface_culling = True
sc.render.resolution_x, sc.render.resolution_y = 1280, 720
cam_data = bpy.data.cameras.new('cam'); cam = bpy.data.objects.new('cam', cam_data); sc.collection.objects.link(cam); sc.camera = cam
cam_data.sensor_fit = 'HORIZONTAL'; cam_data.sensor_width = 36.0; cam_data.clip_start = 0.05; cam_data.clip_end = 200.0

# shots: (name, start_frame, end_frame, (pos_start, look_start), (pos_end, look_end), lens_mm)
SHOTS = [
    ('tvpov',   0,    899,  ((0.0, -1.25, 2.00), (0.0, 0.5, 0.70)),    ((0.15, -1.30, 1.95), (0.0, 0.4, 0.70)),  20.0),
    ('side34',  900,  2399, ((-2.6, 1.3, 1.55), (0.0, 0.0, 0.85)),     ((-2.3, 1.9, 1.65), (0.0, -0.2, 0.85)),   30.0),
    ('behind',  2400, 5399, ((-0.7, 2.5, 2.0), (0.0, -0.4, 0.8)),      ((0.6, 2.6, 2.05), (0.0, -0.4, 0.8)),    28.0),
    ('side34b', 5400, 6238, ((2.4, 1.6, 1.5), (0.0, 0.0, 0.85)),       ((2.0, 2.2, 1.7), (0.0, -0.2, 0.85)),    30.0),
]


def aim(pos, look):
    cam.location = Vector(pos); cam.rotation_mode = 'QUATERNION'
    cam.rotation_quaternion = (Vector(look) - Vector(pos)).to_track_quat('-Z', 'Y')


for name, f0, f1, a, b, lens in SHOTS:
    cam_data.lens = lens
    aim(*a)
    sc.render.filepath = os.path.join(OUT, 'shot_%s.png' % name); bpy.ops.render.render(write_still=True)

if EXPORT_CAM:
    # keyframe the camera: lerp position / slerp rotation inside a shot, hard cut between shots
    cam.animation_data_clear(); cam_data.animation_data_clear()
    bpy.context.preferences.edit.keyframe_new_interpolation_type = 'LINEAR'   # 5.x: no action.fcurves (layered actions)
    for name, f0, f1, a, b, lens in SHOTS:
        for f, (pos, look) in ((f0, a), (f1, b)):
            sc.frame_set(f); cam_data.lens = lens; aim(pos, look)
            cam.keyframe_insert('location', frame=f); cam.keyframe_insert('rotation_quaternion', frame=f)
            cam_data.keyframe_insert('lens', frame=f)
    # export at full frame rate (the game slerps between our per-frame keys anyway)
    sc.frame_start, sc.frame_end = 0, 6238
    path = os.path.join(OUT, 'music_lesa.camanm')
    data, spec = export_anm.export_camanm(path, cam, frame_start=0, frame_end=6238, fps=60)
    print('CAMANM written', len(data), 'bytes; fov keys', len(spec['camera'][2]['keys']))
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, 'stage_camera_scene.blend'))
print('DONE')
