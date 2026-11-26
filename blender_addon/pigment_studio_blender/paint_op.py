"""3D Viewport modal raycast painting operator and Principled BSDF wiring."""

from __future__ import annotations

import math
import time
from typing import List, Optional, Tuple

import bpy
from bpy_extras import view3d_utils
from mathutils import Vector, geometry

from . import bridge

SEAM_BREAK_PX = 128.0
LIVE_SYNC_INTERVAL_S = 1.0 / 30.0


def _barycentric_weights(p: Vector, a: Vector, b: Vector, c: Vector) -> Tuple[float, float, float]:
    v0 = b - a
    v1 = c - a
    v2 = p - a
    d00 = v0.dot(v0)
    d01 = v0.dot(v1)
    d11 = v1.dot(v1)
    d20 = v2.dot(v0)
    d21 = v2.dot(v1)
    denom = d00 * d11 - d01 * d01
    if abs(denom) < 1e-12:
        return (1.0, 0.0, 0.0)
    v = (d11 * d20 - d01 * d21) / denom
    w = (d00 * d21 - d01 * d20) / denom
    u = 1.0 - v - w
    return (u, v, w)


def raycast_canvas_uv(
    context: bpy.types.Context,
    event: bpy.types.Event,
    obj: bpy.types.Object,
) -> Optional[Tuple[float, float]]:
    """Raycast from the 3D viewport cursor onto `obj` and return (x, y) in 0..1024 canvas space."""
    region = context.region
    rv3d = context.region_data
    if region is None or rv3d is None or obj.type != "MESH":
        return None

    mesh: bpy.types.Mesh = obj.data
    uv_layer = mesh.uv_layers.active
    if uv_layer is None:
        return None

    coord = (event.mouse_region_x, event.mouse_region_y)
    ray_origin_world = view3d_utils.region_2d_to_origin_3d(region, rv3d, coord)
    ray_dir_world = view3d_utils.region_2d_to_vector_3d(region, rv3d, coord)

    inv_world = obj.matrix_world.inverted()
    ray_origin = inv_world @ ray_origin_world
    ray_dir = (inv_world.to_3x3() @ ray_dir_world).normalized()

    hit, loc, _norm, face_idx = obj.ray_cast(ray_origin, ray_dir)
    if not hit or face_idx < 0:
        return None

    if not mesh.loop_triangles:
        mesh.calc_loop_triangles()

    verts = mesh.vertices
    uv_data = uv_layer.data

    for tri in mesh.loop_triangles:
        if tri.polygon_index != face_idx:
            continue
        v0 = verts[tri.vertices[0]].co
        v1 = verts[tri.vertices[1]].co
        v2 = verts[tri.vertices[2]].co
        hit_tri = geometry.intersect_ray_tri(v0, v1, v2, ray_dir, ray_origin, True)
        if hit_tri is None:
            continue
        w0, w1, w2 = _barycentric_weights(loc, v0, v1, v2)
        uv0 = uv_data[tri.loops[0]].uv
        uv1 = uv_data[tri.loops[1]].uv
        uv2 = uv_data[tri.loops[2]].uv
        u = (w0 * uv0.x + w1 * uv1.x + w2 * uv2.x) % 1.0
        v = (w0 * uv0.y + w1 * uv1.y + w2 * uv2.y) % 1.0
        return (u * bridge.SIDE, (1.0 - v) * bridge.SIDE)

    return None


class PIGMENT_OT_paint_3d(bpy.types.Operator):
    """Paint directly onto the active mesh's UV map using Pigment Studio GPU physics"""

    bl_idname = "pigment.paint_3d"
    bl_label = "Paint 3D Stroke"
    bl_options = {"REGISTER"}

    _painting: bool = False
    _points: List[Tuple[float, float, float]] = []
    _last_sync: float = 0.0

    @classmethod
    def poll(cls, context: bpy.types.Context) -> bool:
        obj = context.active_object
        return (
            context.area is not None
            and context.area.type == "VIEW_3D"
            and obj is not None
            and obj.type == "MESH"
            and obj.data.uv_layers.active is not None
        )

    def _apply_settings(self, context: bpy.types.Context, session: bridge.PigmentBridge) -> None:
        settings = context.scene.pigment_studio
        session.set_mix(
            int(settings.brush_type),
            settings.recipe_tuple(),
            clean=settings.clean_brush,
        )

    def _flush_stroke(self, context: bpy.types.Context, session: bridge.PigmentBridge, sync: bool) -> None:
        settings = context.scene.pigment_studio
        if self._points:
            session.stroke(
                int(settings.brush_type),
                settings.brush_diameter,
                math.radians(settings.brush_angle),
                settings.auto_follow,
                self._points,
            )
            self._points = [self._points[-1]]
        if sync:
            session.sync_pbr_images(
                gl_normals=settings.gl_normals,
                smooth_height=settings.smooth_height,
            )
            self._last_sync = time.monotonic()

    def invoke(self, context: bpy.types.Context, event: bpy.types.Event):
        session = bridge.get_session()
        self._apply_settings(context, session)
        self._painting = False
        self._points = []
        self._last_sync = 0.0

        if event.type == "LEFTMOUSE" and event.value == "PRESS":
            pt = raycast_canvas_uv(context, event, context.active_object)
            if pt is not None:
                self._painting = True
                pressure = event.pressure if event.pressure > 0.0 else 1.0
                self._points.append((pt[0], pt[1], pressure))

        context.window.cursor_modal_set("PAINT_BRUSH")
        context.window_manager.modal_handler_add(self)
        return {"RUNNING_MODAL"}

    def modal(self, context: bpy.types.Context, event: bpy.types.Event):
        if event.type in {"RIGHTMOUSE", "ESC"}:
            if self._points:
                session = bridge.get_session()
                self._flush_stroke(context, session, sync=True)
            context.window.cursor_modal_restore()
            return {"FINISHED"}

        if event.type == "LEFTMOUSE":
            session = bridge.get_session()
            if event.value == "PRESS":
                self._apply_settings(context, session)
                self._painting = True
                self._points.clear()
                pt = raycast_canvas_uv(context, event, context.active_object)
                if pt is not None:
                    pressure = event.pressure if event.pressure > 0.0 else 1.0
                    self._points.append((pt[0], pt[1], pressure))
                return {"RUNNING_MODAL"}
            if event.value == "RELEASE" and self._painting:
                self._painting = False
                self._flush_stroke(context, session, sync=True)
                self._points.clear()
                return {"RUNNING_MODAL"}

        if event.type == "MOUSEMOVE" and self._painting:
            pt = raycast_canvas_uv(context, event, context.active_object)
            session = bridge.get_session()
            if pt is None:
                if self._points:
                    self._flush_stroke(context, session, sync=False)
                    self._points.clear()
                return {"RUNNING_MODAL"}

            pressure = event.pressure if event.pressure > 0.0 else 1.0
            if self._points:
                lx, ly, _ = self._points[-1]
                if math.hypot(pt[0] - lx, pt[1] - ly) > SEAM_BREAK_PX:
                    self._flush_stroke(context, session, sync=False)
                    self._points = [(pt[0], pt[1], pressure)]
                else:
                    self._points.append((pt[0], pt[1], pressure))
            else:
                self._points.append((pt[0], pt[1], pressure))

            now = time.monotonic()
            if len(self._points) >= 4 and (now - self._last_sync) >= LIVE_SYNC_INTERVAL_S:
                self._flush_stroke(context, session, sync=True)
            return {"RUNNING_MODAL"}

        if event.type in {"MIDDLEMOUSE", "WHEELUPMOUSE", "WHEELDOWNMOUSE"}:
            return {"PASS_THROUGH"}

        return {"RUNNING_MODAL"}


class PIGMENT_OT_setup_material(bpy.types.Operator):
    """Create or update a Principled BSDF material wired to Pigment Studio PBR maps"""

    bl_idname = "pigment.setup_material"
    bl_label = "Setup Pigment Material"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context: bpy.types.Context) -> bool:
        return context.active_object is not None and context.active_object.type == "MESH"

    def execute(self, context: bpy.types.Context):
        settings = context.scene.pigment_studio
        session = bridge.get_session()
        session.sync_pbr_images(
            gl_normals=settings.gl_normals,
            smooth_height=settings.smooth_height,
        )

        mat = bpy.data.materials.get("Pigment_Material")
        if mat is None:
            mat = bpy.data.materials.new(name="Pigment_Material")
        mat.use_nodes = True
        nt = mat.node_tree
        nt.nodes.clear()

        out_node = nt.nodes.new("ShaderNodeOutputMaterial")
        out_node.location = (520, 0)

        bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
        bsdf.location = (220, 0)
        bsdf.inputs["Roughness"].default_value = 0.42
        nt.links.new(bsdf.outputs["BSDF"], out_node.inputs["Surface"])

        tex_color = nt.nodes.new("ShaderNodeTexImage")
        tex_color.location = (-320, 260)
        tex_color.image = bpy.data.images.get("Pigment_Color")
        nt.links.new(tex_color.outputs["Color"], bsdf.inputs["Base Color"])

        tex_normal = nt.nodes.new("ShaderNodeTexImage")
        tex_normal.location = (-320, -20)
        tex_normal.image = bpy.data.images.get("Pigment_Normal")

        normal_map = nt.nodes.new("ShaderNodeNormalMap")
        normal_map.location = (-20, -20)
        normal_map.inputs["Strength"].default_value = 1.0
        nt.links.new(tex_normal.outputs["Color"], normal_map.inputs["Color"])
        nt.links.new(normal_map.outputs["Normal"], bsdf.inputs["Normal"])

        tex_height = nt.nodes.new("ShaderNodeTexImage")
        tex_height.location = (-320, -300)
        tex_height.image = bpy.data.images.get("Pigment_Height")

        disp = nt.nodes.new("ShaderNodeDisplacement")
        disp.location = (220, -280)
        disp.inputs["Scale"].default_value = 0.02
        disp.inputs["Midlevel"].default_value = 0.0
        nt.links.new(tex_height.outputs["Color"], disp.inputs["Height"])
        nt.links.new(disp.outputs["Displacement"], out_node.inputs["Displacement"])

        obj = context.active_object
        if len(obj.data.materials) == 0:
            obj.data.materials.append(mat)
        else:
            obj.data.materials[0] = mat

        self.report({"INFO"}, "Wired Pigment_Color, Pigment_Normal, and Pigment_Height")
        return {"FINISHED"}
