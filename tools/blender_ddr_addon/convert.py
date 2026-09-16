"""Coordinate conventions shared by the importers (and, later, the exporters).

Game side (docs/3d_model_format_research.md §3.2, §5.1, §6):
  * right-handed, Y-up, metres (camera tracks: centimetres)
  * 4x4 matrices are ROW-vector: p_world = p_local @ M, translation in row 3
  * bones, animation keys and camera quaternions are (x, y, z, w)

Blender side: right-handed, Z-up, column-vector matrices, quaternions (w, x, y, z).

Conversion: one rigid rotation of +90 deg about X (game Y -> Blender Z, game Z ->
Blender -Y), applied to every world-space quantity. A game local->world matrix
M (row form) becomes the Blender local->world matrix  C @ M^T  when the local
frame is kept as-is (bones keep their Maya joint frames; cameras already share
Blender's -Z-forward/+Y-up convention).
"""
from mathutils import Matrix, Quaternion, Vector

# game (x, y, z) -> blender (x, -z, y)
GAME_TO_BLENDER = Matrix.Rotation(1.5707963267948966, 4, "X")
BLENDER_TO_GAME = GAME_TO_BLENDER.inverted()


def vec_to_blender(v):
    """Game-space position/direction (x, y, z) -> Blender Vector."""
    return Vector((v[0], -v[2], v[1]))


def vec_to_game(v):
    return (v[0], v[2], -v[1])


def rowmat_to_blender(m16):
    """16 floats, game row-vector matrix (local->world) -> Blender column matrix
    expressing the SAME local frame in Blender world space (local axes untouched)."""
    rows = [m16[0:4], m16[4:8], m16[8:12], m16[12:16]]
    col = Matrix(rows).transposed()  # row-vector -> column-vector
    return GAME_TO_BLENDER @ col


def rowmat_from_blender(mat):
    """Inverse of rowmat_to_blender -> list of 16 floats (row-vector form)."""
    col = BLENDER_TO_GAME @ mat
    t = col.transposed()
    return [t[r][c] for r in range(4) for c in range(4)]


def quat_xyzw_to_blender_rowmat(q):
    """Game quaternion (x, y, z, w) -> the 3x3 row-vector rotation the game builds
    from it (anm_dump.quat_to_rowmat), as a Blender Matrix (still game-space rows)."""
    x, y, z, w = q
    return Matrix((
        (1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w)),
        (2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w)),
        (2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y)),
    ))


def game_frame_to_blender(rot_rowmat3, translation):
    """Rotation given as the game's row-vector 3x3 (rows = local axes in game
    world) + game translation -> Blender 4x4 local->world matrix."""
    col = rot_rowmat3.transposed().to_4x4()
    col.translation = Vector(translation)
    return GAME_TO_BLENDER @ col


def blender_quat_from_game_xyzw(q):
    """Convenience: (x, y, z, w) -> mathutils.Quaternion (w, x, y, z) in game axes."""
    return Quaternion((q[3], q[0], q[1], q[2]))
