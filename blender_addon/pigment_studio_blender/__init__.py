"""Pigment Studio Blender Extension — Native GPU Kubelka-Munk Painting."""

from __future__ import annotations

from typing import Tuple

import bpy

from . import bridge
from .paint_op import PIGMENT_OT_paint_3d, PIGMENT_OT_setup_material

bl_info = {
    "name": "Pigment Studio",
    "author": "PolyBird",
    "version": (0, 3, 1),
    "blender": (4, 2, 0),
    "location": "View3D > Sidebar > Pigment",
    "description": "Real-time GPU Kubelka-Munk spectral oil & pigment painting",
    "category": "Paint",
}

PRESETS: dict[str, Tuple[float, ...]] = {
    "PY3": (1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    "PB15": (0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    "PR122": (0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    "PW6": (0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0),
    "PB29": (0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0),
    "PBr7": (0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    "PR108": (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0),
    "PBk9": (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
}


class PigmentStudioSettings(bpy.types.PropertyGroup):
    brush_type: bpy.props.EnumProperty(
        name="Brush",
        items=bridge.BRUSH_ITEMS,
        default="0",
    )
    brush_diameter: bpy.props.FloatProperty(
        name="Diameter (px)",
        min=4.0,
        max=256.0,
        default=42.0,
    )
    brush_angle: bpy.props.FloatProperty(
        name="Angle (deg)",
        min=-180.0,
        max=180.0,
        default=0.0,
    )
    auto_follow: bpy.props.BoolProperty(
        name="Auto Follow Stroke",
        default=True,
    )
    clean_brush: bpy.props.BoolProperty(
        name="Clean Brush",
        default=False,
    )
    gl_normals: bpy.props.BoolProperty(
        name="OpenGL Normals (+Y)",
        default=True,
    )
    smooth_height: bpy.props.BoolProperty(
        name="Smooth Height",
        default=True,
    )
    wipe_fraction: bpy.props.FloatProperty(
        name="Wipe Fraction",
        min=0.05,
        max=1.0,
        default=0.35,
    )

    mix_py3: bpy.props.FloatProperty(name="PY3 Hansa Yellow", min=0.0, max=1.0, default=0.0)
    mix_pb15: bpy.props.FloatProperty(name="PB15:2 Phthalo Blue", min=0.0, max=1.0, default=0.0)
    mix_pr122: bpy.props.FloatProperty(name="PR122 Quin Magenta", min=0.0, max=1.0, default=0.0)
    mix_pw6: bpy.props.FloatProperty(name="PW6 Titanium White", min=0.0, max=1.0, default=0.25)
    mix_pb29: bpy.props.FloatProperty(name="PB29 Ultramarine", min=0.0, max=1.0, default=0.75)
    mix_pbr7: bpy.props.FloatProperty(name="PBr7 Burnt Umber", min=0.0, max=1.0, default=0.0)
    mix_pr108: bpy.props.FloatProperty(name="PR108 Cadmium Red", min=0.0, max=1.0, default=0.0)
    mix_pbk9: bpy.props.FloatProperty(name="PBk9 Ivory Black", min=0.0, max=1.0, default=0.0)

    def recipe_tuple(self) -> Tuple[float, ...]:
        return (
            self.mix_py3,
            self.mix_pb15,
            self.mix_pr122,
            self.mix_pw6,
            self.mix_pb29,
            self.mix_pbr7,
            self.mix_pr108,
            self.mix_pbk9,
        )

    def set_recipe(self, recipe: Tuple[float, ...]) -> None:
        (
            self.mix_py3,
            self.mix_pb15,
            self.mix_pr122,
            self.mix_pw6,
            self.mix_pb29,
            self.mix_pbr7,
            self.mix_pr108,
            self.mix_pbk9,
        ) = recipe


class PIGMENT_OT_apply_preset(bpy.types.Operator):
    """Load a pure Studio Essentials pigment tube into the mix"""

    bl_idname = "pigment.apply_preset"
    bl_label = "Load Pigment Tube"

    preset_key: bpy.props.StringProperty()

    def execute(self, context: bpy.types.Context):
        recipe = PRESETS.get(self.preset_key)
        if recipe is None:
            return {"CANCELLED"}
        context.scene.pigment_studio.set_recipe(recipe)
        return {"FINISHED"}


class PIGMENT_OT_dry_canvas(bpy.types.Operator):
    """Dry wet paint on the canvas into the dry oil layer"""

    bl_idname = "pigment.dry_canvas"
    bl_label = "Dry Canvas"

    def execute(self, context: bpy.types.Context):
        settings = context.scene.pigment_studio
        session = bridge.get_session()
        session.dry_canvas()
        session.sync_pbr_images(gl_normals=settings.gl_normals, smooth_height=settings.smooth_height)
        return {"FINISHED"}


class PIGMENT_OT_clear_canvas(bpy.types.Operator):
    """Clear the Pigment Studio canvas back to bare substrate"""

    bl_idname = "pigment.clear_canvas"
    bl_label = "Clear Canvas"

    def execute(self, context: bpy.types.Context):
        settings = context.scene.pigment_studio
        session = bridge.get_session()
        session.clear()
        session.sync_pbr_images(gl_normals=settings.gl_normals, smooth_height=settings.smooth_height)
        return {"FINISHED"}


class PIGMENT_OT_wipe_canvas(bpy.types.Operator):
    """Wipe wet paint across the canvas by the configured fraction"""

    bl_idname = "pigment.wipe_canvas"
    bl_label = "Wipe Wet Paint"

    def execute(self, context: bpy.types.Context):
        settings = context.scene.pigment_studio
        session = bridge.get_session()
        session.wipe(int(settings.brush_type), settings.wipe_fraction)
        session.sync_pbr_images(gl_normals=settings.gl_normals, smooth_height=settings.smooth_height)
        return {"FINISHED"}


class PIGMENT_OT_sync_maps(bpy.types.Operator):
    """Sync Pigment_Color, Pigment_Normal, and Pigment_Height images from GPU"""

    bl_idname = "pigment.sync_maps"
    bl_label = "Sync PBR Maps"

    def execute(self, context: bpy.types.Context):
        settings = context.scene.pigment_studio
        session = bridge.get_session()
        session.sync_pbr_images(gl_normals=settings.gl_normals, smooth_height=settings.smooth_height)
        return {"FINISHED"}


class VIEW3D_PT_pigment_studio(bpy.types.Panel):
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Pigment"
    bl_label = "Pigment Studio"

    def draw(self, context: bpy.types.Context) -> None:
        layout = self.layout
        settings = context.scene.pigment_studio

        col = layout.column(align=True)
        col.operator("pigment.setup_material", icon="MATERIAL")
        col.operator("pigment.paint_3d", icon="BRUSH_DATA")

        box = layout.box()
        box.label(text="Brush")
        box.prop(settings, "brush_type")
        box.prop(settings, "brush_diameter")
        box.prop(settings, "brush_angle")
        box.prop(settings, "auto_follow")
        box.prop(settings, "clean_brush")

        box = layout.box()
        box.label(text="Studio Essentials Mix")
        row = box.row(align=True)
        for key in ("PY3", "PB15", "PR122", "PW6"):
            op = row.operator("pigment.apply_preset", text=key)
            op.preset_key = key
        row = box.row(align=True)
        for key in ("PB29", "PBr7", "PR108", "PBk9"):
            op = row.operator("pigment.apply_preset", text=key)
            op.preset_key = key

        for prop_name in (
            "mix_py3",
            "mix_pb15",
            "mix_pr122",
            "mix_pw6",
            "mix_pb29",
            "mix_pbr7",
            "mix_pr108",
            "mix_pbk9",
        ):
            box.prop(settings, prop_name, slider=True)

        box = layout.box()
        box.label(text="Canvas & PBR Maps")
        row = box.row(align=True)
        row.operator("pigment.dry_canvas", icon="MOD_SOLIDIFY")
        row.operator("pigment.clear_canvas", icon="X")
        row = box.row(align=True)
        row.prop(settings, "wipe_fraction", slider=True)
        row.operator("pigment.wipe_canvas", text="Wipe")
        box.prop(settings, "gl_normals")
        box.prop(settings, "smooth_height")
        box.operator("pigment.sync_maps", icon="FILE_REFRESH")


_CLASSES = (
    PigmentStudioSettings,
    PIGMENT_OT_apply_preset,
    PIGMENT_OT_dry_canvas,
    PIGMENT_OT_clear_canvas,
    PIGMENT_OT_wipe_canvas,
    PIGMENT_OT_sync_maps,
    PIGMENT_OT_paint_3d,
    PIGMENT_OT_setup_material,
    VIEW3D_PT_pigment_studio,
)


def register() -> None:
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.pigment_studio = bpy.props.PointerProperty(type=PigmentStudioSettings)


def unregister() -> None:
    bridge.shutdown_session()
    if hasattr(bpy.types.Scene, "pigment_studio"):
        del bpy.types.Scene.pigment_studio
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
