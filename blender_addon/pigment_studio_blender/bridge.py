"""C-ABI bridge between Blender (ctypes) and native pigment_blender.dll."""

from __future__ import annotations

import ctypes
from pathlib import Path
from typing import Iterable, Optional, Tuple

import bpy
import numpy as np

SIDE = 1024
PIXEL_COUNT = SIDE * SIDE
RGBA_F32_COUNT = PIXEL_COUNT * 4

BRUSH_ITEMS = [
    ("0", "Round", "Classic pointed round brush"),
    ("1", "Flat", "Chisel-edge flat bristle brush"),
    ("2", "Filbert", "Oval-tip filbert brush"),
    ("3", "Fan", "Wide soft fan blender"),
    ("4", "Palette Knife", "Impasto palette knife"),
    ("5", "Soft Mop", "Soft glazing mop brush"),
]

TUBE_NAMES = (
    ("PY3", "Hansa Yellow"),
    ("PB15:2", "Phthalo Blue"),
    ("PR122", "Quinacridone Magenta"),
    ("PW6", "Titanium White"),
    ("PB29", "Ultramarine Blue"),
    ("PBr7", "Burnt Umber"),
    ("PR108", "Cadmium Red"),
    ("PBk9", "Ivory Black"),
)

_F32_ARRAY_8 = ctypes.c_float * 8
_U8_ARRAY_3 = ctypes.c_uint8 * 3


def _find_library() -> Path:
    here = Path(__file__).resolve().parent
    bundled = here / "pigment_blender.dll"
    if bundled.is_file():
        return bundled
    # Development fallback when running directly from the git repository.
    repo_release = here.parent.parent / "target" / "release" / "pigment_blender.dll"
    if repo_release.is_file():
        return repo_release
    repo_debug = here.parent.parent / "target" / "debug" / "pigment_blender.dll"
    if repo_debug.is_file():
        return repo_debug
    raise FileNotFoundError(
        f"Could not find pigment_blender.dll next to {here} or in target/release."
    )


def _load_cdll() -> ctypes.CDLL:
    lib_path = _find_library()
    lib = ctypes.CDLL(str(lib_path))

    lib.pigment_session_create.argtypes = []
    lib.pigment_session_create.restype = ctypes.c_void_p

    lib.pigment_session_destroy.argtypes = [ctypes.c_void_p]
    lib.pigment_session_destroy.restype = None

    lib.pigment_session_clear.argtypes = [ctypes.c_void_p]
    lib.pigment_session_clear.restype = None

    lib.pigment_session_dry_canvas.argtypes = [ctypes.c_void_p]
    lib.pigment_session_dry_canvas.restype = None

    lib.pigment_session_set_mix.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.POINTER(ctypes.c_float),
        ctypes.c_bool,
    ]
    lib.pigment_session_set_mix.restype = None

    lib.pigment_session_wipe.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.c_float,
    ]
    lib.pigment_session_wipe.restype = None

    lib.pigment_session_stroke.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.c_float,
        ctypes.c_float,
        ctypes.c_bool,
        ctypes.POINTER(ctypes.c_float),
        ctypes.c_uint32,
    ]
    lib.pigment_session_stroke.restype = None

    lib.pigment_session_readback_pbr.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_float),
        ctypes.POINTER(ctypes.c_float),
        ctypes.POINTER(ctypes.c_float),
        ctypes.c_bool,
        ctypes.c_bool,
    ]
    lib.pigment_session_readback_pbr.restype = ctypes.c_bool

    lib.pigment_mix_preview_srgb.argtypes = [
        ctypes.POINTER(ctypes.c_float),
        ctypes.POINTER(ctypes.c_uint8),
    ]
    lib.pigment_mix_preview_srgb.restype = None

    return lib


def _ensure_image(name: str, colorspace: str) -> bpy.types.Image:
    img = bpy.data.images.get(name)
    if img is None or img.size[0] != SIDE or img.size[1] != SIDE:
        if img is not None:
            bpy.data.images.remove(img)
        img = bpy.data.images.new(name=name, width=SIDE, height=SIDE, alpha=True, float_buffer=True)
    img.colorspace_settings.name = colorspace
    return img


class PigmentBridge:
    """Owns a native GPU PigmentSession and reusable PBR readback buffers."""

    def __init__(self) -> None:
        self._lib = _load_cdll()
        self._handle = self._lib.pigment_session_create()
        if not self._handle:
            raise RuntimeError("Failed to initialize native GPU PigmentSession (wgpu)")
        self._color_buf = np.empty(RGBA_F32_COUNT, dtype=np.float32)
        self._normal_buf = np.empty(RGBA_F32_COUNT, dtype=np.float32)
        self._height_buf = np.empty(RGBA_F32_COUNT, dtype=np.float32)
        self._color_ptr = self._color_buf.ctypes.data_as(ctypes.POINTER(ctypes.c_float))
        self._normal_ptr = self._normal_buf.ctypes.data_as(ctypes.POINTER(ctypes.c_float))
        self._height_ptr = self._height_buf.ctypes.data_as(ctypes.POINTER(ctypes.c_float))

    def close(self) -> None:
        if getattr(self, "_handle", None):
            self._lib.pigment_session_destroy(self._handle)
            self._handle = None

    def __del__(self) -> None:
        self.close()

    def clear(self) -> None:
        self._lib.pigment_session_clear(self._handle)

    def dry_canvas(self) -> None:
        self._lib.pigment_session_dry_canvas(self._handle)

    def set_mix(self, brush_id: int, recipe: Iterable[float], clean: bool = False) -> None:
        arr = _F32_ARRAY_8(*[float(v) for v in recipe])
        self._lib.pigment_session_set_mix(self._handle, int(brush_id), arr, bool(clean))

    def wipe(self, brush_id: int, fraction: float) -> None:
        self._lib.pigment_session_wipe(self._handle, int(brush_id), float(fraction))

    def stroke(
        self,
        brush_id: int,
        diameter: float,
        angle_rad: float,
        auto_follow: bool,
        points: Iterable[Tuple[float, float, float]],
    ) -> None:
        flat: list[float] = []
        count = 0
        for x, y, p in points:
            flat.extend((float(x), float(y), float(p)))
            count += 1
        if count == 0:
            return
        buf = (ctypes.c_float * (count * 3))(*flat)
        self._lib.pigment_session_stroke(
            self._handle,
            int(brush_id),
            float(diameter),
            float(angle_rad),
            bool(auto_follow),
            buf,
            count,
        )

    def mix_preview_srgb(self, recipe: Iterable[float]) -> Tuple[int, int, int]:
        arr = _F32_ARRAY_8(*[float(v) for v in recipe])
        out = _U8_ARRAY_3()
        self._lib.pigment_mix_preview_srgb(arr, out)
        return (int(out[0]), int(out[1]), int(out[2]))

    def sync_pbr_images(self, gl_normals: bool = True, smooth_height: bool = True) -> None:
        ok = self._lib.pigment_session_readback_pbr(
            self._handle,
            self._color_ptr,
            self._normal_ptr,
            self._height_ptr,
            bool(gl_normals),
            bool(smooth_height),
        )
        if not ok:
            raise RuntimeError("pigment_session_readback_pbr failed")

        color_img = _ensure_image("Pigment_Color", "sRGB")
        color_img.pixels.foreach_set(self._color_buf)
        color_img.update()

        normal_img = _ensure_image("Pigment_Normal", "Non-Color")
        normal_img.pixels.foreach_set(self._normal_buf)
        normal_img.update()

        height_img = _ensure_image("Pigment_Height", "Non-Color")
        height_img.pixels.foreach_set(self._height_buf)
        height_img.update()


_SESSION: Optional[PigmentBridge] = None


def get_session() -> PigmentBridge:
    global _SESSION
    if _SESSION is None:
        _SESSION = PigmentBridge()
    return _SESSION


def shutdown_session() -> None:
    global _SESSION
    if _SESSION is not None:
        _SESSION.close()
        _SESSION = None
