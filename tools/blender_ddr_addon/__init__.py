"""DDR 3D Formats — Blender add-on for DanceDanceRevolution KTMDL / ANM files.

Import:  File > Import > DDR Model (.model)      — armature + skinned meshes + DDS materials
         File > Import > DDR Character (.model)  — body + face/head/chest/forearm/hips parts
                                                    attached to their bones + rlist scale
         File > Import > DDR Animation (.anm / .camanm)
Export:  File > Export > DDR Model / DDR Character (folder layout + rlist row) / DDR Animation
Every value the exporters need is kept as a ``ddr_*`` custom property on the imported data.

Works both as a Blender 4.2+ extension (blender_manifest.toml) and as a legacy
add-on; the format codecs are the repo's scripts/ktmdl_dump.py / anm_dump.py.
"""
bl_info = {
    "name": "DDR 3D Formats (KTMDL / ANM)",
    "author": "ddr-world-universal-modpack",
    "version": (0, 2, 0),
    "blender": (4, 2, 0),
    "location": "File > Import/Export",
    "description": "Import/export DanceDanceRevolution .model / .anm / .camanm (+ whole characters)",
    "category": "Import-Export",
}

import importlib
import os

import bpy
from bpy.props import BoolProperty, EnumProperty, IntProperty, StringProperty
from bpy_extras.io_utils import ExportHelper, ImportHelper

from . import (codec, convert, export_anm, export_character, export_model, import_anm, import_character, import_model,
               import_stage)

for _m in (codec, convert, import_model, import_character, import_stage, import_anm, export_model, export_character,
           export_anm):
    importlib.reload(_m)


class DDR_OT_import_model(bpy.types.Operator, ImportHelper):
    """Import a DDR KTMDL .model (with its sibling .b2it bone names and .dds textures)"""
    bl_idname = "import_scene.ddr_model"
    bl_label = "Import DDR Model (.model)"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".model"
    filter_glob: StringProperty(default="*.model", options={"HIDDEN"})
    import_textures: BoolProperty(name="Load DDS textures", default=True)

    def execute(self, context):
        try:
            arm, objs = import_model.load_model(self.filepath, self.import_textures)
        except Exception as e:  # noqa: BLE001 — surface parser errors in the UI
            self.report({"ERROR"}, "DDR model import failed: %s" % e)
            return {"CANCELLED"}
        for o in bpy.context.selected_objects:
            o.select_set(False)
        for o in objs:
            o.select_set(True)
        if arm is not None:
            arm.select_set(True)
            context.view_layer.objects.active = arm
        self.report({"INFO"}, "Imported %d mesh(es)%s" % (len(objs), " + armature" if arm else ""))
        return {"FINISHED"}


class DDR_OT_import_character(bpy.types.Operator, ImportHelper):
    """Import a dancer BODY .model with its face/head/chest/forearm/hips part models attached to the
    right bones (as the game does) and the per-character scale from chara_resources.rlist"""
    bl_idname = "import_scene.ddr_character"
    bl_label = "Import DDR Character (body .model + parts)"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".model"
    filter_glob: StringProperty(default="*.model", options={"HIDDEN"})
    import_textures: BoolProperty(name="Load DDS textures", default=True)
    import_parts: BoolProperty(name="Attach part models", default=True,
                               description="Sibling pl_<name>_face01/_head00/_chest00/_forearm00/_hips00 models")
    all_faces: BoolProperty(name="All face variants", default=True,
                            description="Import face02/face03 too (hidden — the game shows face01)")
    apply_rlist_scale: BoolProperty(name="Apply chara_resources.rlist scale", default=True,
                                    description="Uniform scale on the armature object (field 3 of the character's row)")
    rlist_path: StringProperty(name="rlist file", default="", subtype="FILE_PATH",
                               description="chara_resources.rlist (auto-detected next to the pl_* folders or under startup/data/chara when empty)")

    def execute(self, context):
        try:
            arm, body, parts, info = import_character.load_character(
                self.filepath, self.import_textures, self.import_parts, self.all_faces,
                self.apply_rlist_scale, self.rlist_path or None)
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR character import failed: %s" % e)
            return {"CANCELLED"}
        for o in bpy.context.selected_objects:
            o.select_set(False)
        for o in body + parts:
            o.select_set(True)
        arm.select_set(True)
        context.view_layer.objects.active = arm
        msg = "Imported %s: %d body mesh(es), parts %s" % (info["body"], len(body), info["parts"] or "none")
        if info.get("skipped"):
            msg += ", skipped %s" % info["skipped"]
        if info.get("rlist"):
            msg += ", rlist scale %g" % info["scale"] if not info.get("rlist_missing_row") else ", no rlist row for %s" % info["key"]
        else:
            msg += ", no chara_resources.rlist found"
        self.report({"INFO"}, msg)
        return {"FINISHED"}


class DDR_OT_import_stage(bpy.types.Operator, ImportHelper):
    """Import a whole stage set: pick any gm_<stage>_<part>.model and every part listed for that stage in
    map_resources.rlist is imported (with its play-loop animation), optionally with the stage's camera set"""
    bl_idname = "import_scene.ddr_stage"
    bl_label = "Import DDR Stage (gm_<stage>_*.model set)"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".model"
    filter_glob: StringProperty(default="gm_*.model", options={"HIDDEN"})
    import_textures: BoolProperty(name="Load DDS textures", default=True)
    import_anims: BoolProperty(name="Bake play-loop animations", default=True)
    import_cameras: BoolProperty(name="Import the stage camera set", default=True,
                                 description="stage_camera_resources.rlist -> camera/long/<set>/*.camanm")
    frame_step: IntProperty(name="Bake every Nth frame", default=1, min=1, max=60)

    def execute(self, context):
        try:
            rep = import_stage.load_stage(self.filepath, import_textures=self.import_textures,
                                          import_anims=self.import_anims, import_cameras=self.import_cameras,
                                          frame_step=self.frame_step)
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR stage import failed: %s" % e)
            return {"CANCELLED"}
        msg = "Imported stage %s: %d part(s), %d camera(s)" % (rep["stage"], len(rep["parts"]), len(rep["cameras"]))
        if rep["skipped"]:
            msg += "; skipped %s" % rep["skipped"]
        self.report({"INFO"}, msg)
        return {"FINISHED"}


class DDR_OT_import_anm(bpy.types.Operator, ImportHelper):
    """Import a DDR .anm onto the active armature, or a .camanm as an animated camera"""
    bl_idname = "import_scene.ddr_anm"
    bl_label = "Import DDR Animation (.anm / .camanm)"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".anm"
    filter_glob: StringProperty(default="*.anm;*.camanm", options={"HIDDEN"})
    frame_step: IntProperty(name="Bake every Nth frame", default=1, min=1, max=60)
    apply_game_fov: BoolProperty(
        name="Use the game's 16:9 re-projection for the lens",
        description="Off: use the Maya camera's own field of view instead of what the game renders",
        default=True,
    )

    def execute(self, context):
        ext = os.path.splitext(self.filepath)[1].lower()
        try:
            if ext == ".camanm":
                cam = import_anm.load_camanm(self.filepath, self.frame_step, self.apply_game_fov)
                context.view_layer.objects.active = cam
                self.report({"INFO"}, "Imported camera %s" % cam.name)
            else:
                arm = context.active_object
                if arm is None or arm.type != "ARMATURE":
                    self.report({"ERROR"}, "Select the target armature (imported from a .model) first")
                    return {"CANCELLED"}
                action = import_anm.load_anm(self.filepath, arm, self.frame_step)
                self.report({"INFO"}, "Imported action %s (%d frames)" % (action.name, action["ddr_frame_count"]))
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR animation import failed: %s" % e)
            return {"CANCELLED"}
        return {"FINISHED"}


def _selected_armature_and_meshes(context):
    """Resolve the export set: the active/selected armature + its mesh children (EXCLUDING
    bone-parented character parts — those export through DDR Character or on their own), or
    the selected meshes when there is no armature (static stage prop / a single part)."""
    sel = list(context.selected_objects)
    arm = context.active_object if context.active_object and context.active_object.type == "ARMATURE" else None
    if arm is None:
        arm = next((o for o in sel if o.type == "ARMATURE"), None)
    if arm is None and sel and all(export_model.is_part_object(o) for o in sel if o.type == "MESH"):
        return None, [o for o in sel if o.type == "MESH"]  # a selected part on its own -> raw-axes part model
    if arm is not None:
        meshes = [o for o in arm.children_recursive if o.type == "MESH" and not export_model.is_part_object(o)]
        if not meshes:
            meshes = [o for o in sel if o.type == "MESH" and not export_model.is_part_object(o)]
    else:
        meshes = [o for o in sel if o.type == "MESH"]
    return arm, meshes


class DDR_OT_export_model(bpy.types.Operator, ExportHelper):
    """Export the selected armature (with its child meshes) or meshes as a DDR .model (+ .b2it, .grp2it, .dds)"""
    bl_idname = "export_scene.ddr_model"
    bl_label = "Export DDR Model (.model)"
    bl_options = {"REGISTER"}

    filename_ext = ".model"
    filter_glob: StringProperty(default="*.model", options={"HIDDEN"})
    write_textures: BoolProperty(name="Write DDS textures", default=True,
                                 description="Copy source .dds files or write uncompressed A8R8G8B8 .dds next to the model")

    def execute(self, context):
        arm, meshes = _selected_armature_and_meshes(context)
        if not meshes:
            self.report({"ERROR"}, "Select an armature with mesh children, or mesh objects")
            return {"CANCELLED"}
        try:
            written, spec = export_model.export_model(self.filepath, arm, meshes, self.write_textures)
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR model export failed: %s" % e)
            return {"CANCELLED"}
        self.report({"INFO"}, "Wrote %d file(s): %d bones, %d mesh(es), %d material(s)"
                    % (len(written), len(spec["bones"]), len(spec["meshes"]), len(spec["materials"])))
        return {"FINISHED"}


class DDR_OT_export_character(bpy.types.Operator):
    """Export the active armature as a whole dancer in the game's data/chara layout: pl_<key>/ body,
    pl_<key>_<part>/ per attached part, plus a chara_resources.rlist with this character's row"""
    bl_idname = "export_scene.ddr_character"
    bl_label = "Export DDR Character (folder)"
    bl_options = {"REGISTER"}

    directory: StringProperty(name="Output folder", subtype="DIR_PATH")
    key: StringProperty(name="Character key", default="",
                        description="The pl_<key> name, e.g. emi00 (default: the imported character's key)")
    write_textures: BoolProperty(name="Write DDS textures", default=True)
    write_rlist: BoolProperty(name="Write chara_resources.rlist row", default=True,
                              description="Upsert this character's row into a copy of the source rlist (or a single-row list)")
    rlist_source: StringProperty(name="Source rlist", default="", subtype="FILE_PATH",
                                 description="Existing chara_resources.rlist to copy + update (default: the one seen at import)")

    def invoke(self, context, event):
        arm = context.active_object
        if arm is None or arm.type != "ARMATURE":
            self.report({"ERROR"}, "Select the character's armature first")
            return {"CANCELLED"}
        if not self.key:
            self.key = str(arm.get("ddr_chara_key") or export_character.body_key(
                os.path.splitext(str(arm.get("ddr_source", arm.name)))[0]))
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        arm = context.active_object
        if arm is None or arm.type != "ARMATURE":
            self.report({"ERROR"}, "Select the character's armature first")
            return {"CANCELLED"}
        try:
            rep = export_character.export_character(
                self.directory, arm, key=self.key or None, write_textures=self.write_textures,
                write_rlist=self.write_rlist, rlist_source=self.rlist_source or None)
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR character export failed: %s" % e)
            return {"CANCELLED"}
        msg = "Wrote %s: %d file(s), parts %s" % (rep["body"], len(rep["written"]), [p for p, _ in rep["parts"]] or "none")
        if rep.get("rlist_row"):
            msg += ", rlist row %s -> %s (%s)" % (rep["rlist_row"][0], rep["rlist_row"][1], rep["rlist"])
        if rep["skipped"]:
            msg += "; skipped: " + "; ".join("%s (%s)" % s for s in rep["skipped"])
            self.report({"WARNING"}, msg)
        else:
            self.report({"INFO"}, msg)
        return {"FINISHED"}


class DDR_OT_export_anm(bpy.types.Operator, ExportHelper):
    """Export the active armature's animation (scene frame range) as .anm, or the active camera as .camanm"""
    bl_idname = "export_scene.ddr_anm"
    bl_label = "Export DDR Animation (.anm / .camanm)"
    bl_options = {"REGISTER"}

    filename_ext = ".anm"
    filter_glob: StringProperty(default="*.anm;*.camanm", options={"HIDDEN"})
    scale_tracks: EnumProperty(
        name="Scale tracks",
        items=[("auto", "Only when animated", "Write kind-10 scale tracks for bones whose scale leaves 1.0"),
               ("always", "Always", "Write a scale track for every bone"),
               ("never", "Never", "Rotation + translation only")],
        default="auto",
    )

    def execute(self, context):
        obj = context.active_object
        try:
            if obj is not None and obj.type == "CAMERA":
                path = self.filepath
                if not path.lower().endswith(".camanm"):
                    path = os.path.splitext(path)[0] + ".camanm"
                data, spec = export_anm.export_camanm(path, obj)
                self.report({"INFO"}, "Wrote %s (%d frames)" % (os.path.basename(path), spec["frame_count"] + 1))
            elif obj is not None and obj.type == "ARMATURE":
                inc = {"auto": "auto", "always": True, "never": False}[self.scale_tracks]
                data, spec = export_anm.export_anm(self.filepath, obj, include_scale=inc)
                self.report({"INFO"}, "Wrote %s (%d bones, %d frames, %d tracks)"
                            % (os.path.basename(self.filepath), len(spec["hierarchy"]), spec["frame_count"] + 1, len(spec["tracks"])))
            else:
                self.report({"ERROR"}, "Select an armature (for .anm) or a camera (for .camanm)")
                return {"CANCELLED"}
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, "DDR animation export failed: %s" % e)
            return {"CANCELLED"}
        return {"FINISHED"}


def _menu_import(self, context):
    self.layout.operator(DDR_OT_import_model.bl_idname, text="DDR Model (.model)")
    self.layout.operator(DDR_OT_import_character.bl_idname, text="DDR Character (body .model + parts)")
    self.layout.operator(DDR_OT_import_stage.bl_idname, text="DDR Stage (gm_<stage>_*.model set)")
    self.layout.operator(DDR_OT_import_anm.bl_idname, text="DDR Animation (.anm / .camanm)")


def _menu_export(self, context):
    self.layout.operator(DDR_OT_export_model.bl_idname, text="DDR Model (.model)")
    self.layout.operator(DDR_OT_export_character.bl_idname, text="DDR Character (folder + rlist row)")
    self.layout.operator(DDR_OT_export_anm.bl_idname, text="DDR Animation (.anm / .camanm)")


_CLASSES = (DDR_OT_import_model, DDR_OT_import_character, DDR_OT_import_stage, DDR_OT_import_anm,
            DDR_OT_export_model, DDR_OT_export_character, DDR_OT_export_anm)


def register():
    for c in _CLASSES:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_import.append(_menu_import)
    bpy.types.TOPBAR_MT_file_export.append(_menu_export)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(_menu_export)
    bpy.types.TOPBAR_MT_file_import.remove(_menu_import)
    for c in reversed(_CLASSES):
        bpy.utils.unregister_class(c)


if __name__ == "__main__":
    register()
