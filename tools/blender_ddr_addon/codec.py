"""Locate the format codecs (scripts/ktmdl_dump.py, scripts/anm_dump.py).

The add-on has no format code of its own: the parsers/decoders that were
validated against the whole DDR A3 data set live in the repo's scripts/ directory
(see docs/3d_model_format_research.md). In a source checkout they are loaded from
../../scripts; a packaged build (scripts/build_blender_addon.sh) vendors copies
into ./vendor.
"""
import importlib.util
import os

_HERE = os.path.dirname(os.path.abspath(__file__))
_SEARCH = (
    os.path.join(_HERE, "vendor"),
    os.path.normpath(os.path.join(_HERE, "..", "..", "scripts")),
)


def _load(name):
    for d in _SEARCH:
        path = os.path.join(d, name + ".py")
        if os.path.exists(path):
            spec = importlib.util.spec_from_file_location("ddr_codec_" + name, path)
            if spec is None or spec.loader is None:
                raise ImportError("cannot build a module spec for %s" % path)
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
            return mod
    raise ImportError("%s.py not found in %s" % (name, " or ".join(_SEARCH)))


ktmdl = _load("ktmdl_dump")
anm = _load("anm_dump")
