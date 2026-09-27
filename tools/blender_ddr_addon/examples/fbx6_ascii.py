"""Minimal ASCII FBX 6.1 reader (the legacy text format 3ds Max / FBX SDK 2011 writes).

Blender's importer rejects ASCII FBX, so this reads just what a skinned-character port needs:
the node tree, the mesh (vertices, polygons, UVs, per-polygon materials), the skin clusters
(vertex indices + weights + bind matrices), the bind pose and the parent/child connections.
Pure Python (no bpy) so it can be sanity-checked outside Blender.
"""
import re

_TOK = re.compile(r'\s*(?:(;[^\n]*)|("[^"]*")|([A-Za-z_][A-Za-z0-9_]*):|([{},])|([^\s,{}"]+))')


class Node:
    __slots__ = ('name', 'props', 'children')

    def __init__(self, name, props):
        self.name = name
        self.props = props
        self.children = []

    def find(self, name):
        for c in self.children:
            if c.name == name:
                return c
        return None

    def find_all(self, name):
        return [c for c in self.children if c.name == name]

    def value(self, name, default=None):
        c = self.find(name)
        if c is None:
            return default
        return c.props

    def __repr__(self):
        return 'Node(%s, %r, %d children)' % (self.name, self.props[:4], len(self.children))


def _atom(s):
    try:
        if re.fullmatch(r'[-+]?\d+', s):
            return int(s)
        return float(s)
    except ValueError:
        return s


def tokenize(text):
    pos = 0
    n = len(text)
    out = []
    while pos < n:
        m = _TOK.match(text, pos)
        if not m or m.end() == pos:
            if text[pos:].strip() == '':
                break
            raise ValueError('FBX tokenize error at %d: %r' % (pos, text[pos:pos + 40]))
        pos = m.end()
        comment, string, key, punct, bare = m.groups()
        if comment is not None:
            continue
        if string is not None:
            out.append(('S', string[1:-1]))
        elif key is not None:
            out.append(('K', key))
        elif punct is not None:
            out.append((punct, punct))
        elif bare is not None:
            out.append(('V', _atom(bare)))
    return out


def parse(text):
    toks = tokenize(text)
    root = Node('__root__', [])
    stack = [root]
    i = 0
    n = len(toks)
    while i < n:
        kind, val = toks[i]
        if kind == 'K':
            node = Node(val, [])
            i += 1
            # values: value (',' value)*   until a key, '{' or '}'
            while i < n and toks[i][0] in ('S', 'V', ','):
                if toks[i][0] != ',':
                    node.props.append(toks[i][1])
                i += 1
            stack[-1].children.append(node)
            if i < n and toks[i][0] == '{':
                stack.append(node)
                i += 1
        elif kind == '}':
            stack.pop()
            i += 1
        else:
            raise ValueError('FBX parse error: unexpected %r at token %d' % (toks[i], i))
    return root


def mat4_from_list(v):
    """FBX matrices are 16 doubles, column-major for row vectors == rows of the transposed
    (translation in elements 12..14). Returns a row-major 4x4 nested list M such that
    p' = M @ p (column vector)."""
    assert len(v) == 16, len(v)
    # FBX stores m[0..3] = first column (x axis), m[12..14] = translation
    return [[v[0], v[4], v[8], v[12]],
            [v[1], v[5], v[9], v[13]],
            [v[2], v[6], v[10], v[14]],
            [v[3], v[7], v[11], v[15]]]


def load(path):
    text = open(path, 'r', encoding='latin-1').read()
    root = parse(text)
    objects = root.find('Objects')
    models = {}
    mesh_node = None
    materials = {}
    clusters = {}
    bind_pose = {}
    for c in objects.children:
        if c.name == 'Model':
            full, typ = c.props[0], c.props[1]
            nm = full.split('::', 1)[1]
            props = {}
            p60 = c.find('Properties60')
            if p60:
                for p in p60.find_all('Property'):
                    props[p.props[0]] = p.props[3:]
            models[nm] = {'type': typ, 'props': props, 'node': c}
            if typ == 'Mesh':
                mesh_node = (nm, c)
        elif c.name == 'Material':
            nm = c.props[0].split('::', 1)[1]
            materials[nm] = c
        elif c.name == 'Deformer' and len(c.props) > 1 and c.props[1] == 'Cluster':
            nm = c.props[0].split('::', 1)[1]
            idx = c.value('Indexes', [])
            w = c.value('Weights', [])
            t = c.value('Transform')
            tl = c.value('TransformLink')
            clusters[nm] = {'indexes': [int(x) for x in idx], 'weights': [float(x) for x in w],
                            'transform': mat4_from_list(t) if t else None,
                            'transform_link': mat4_from_list(tl) if tl else None}
        elif c.name == 'Pose':
            for pn in c.find_all('PoseNode'):
                node_name = pn.value('Node')[0].split('::', 1)[1]
                bind_pose[node_name] = mat4_from_list(pn.value('Matrix'))
    conns = []
    cn = root.find('Connections')
    if cn:
        for c in cn.find_all('Connect'):
            conns.append(tuple(c.props))
    # mesh
    mname, mnode = mesh_node
    verts = mnode.value('Vertices')
    pvi = mnode.value('PolygonVertexIndex')
    polys = []
    cur = []
    for x in pvi:
        x = int(x)
        if x < 0:
            cur.append(-x - 1)
            polys.append(cur)
            cur = []
        else:
            cur.append(x)
    uv = None
    le = mnode.find('LayerElementUV')
    if le:
        uv = {'mapping': le.value('MappingInformationType')[0], 'ref': le.value('ReferenceInformationType')[0],
              'uv': le.value('UV'), 'index': le.value('UVIndex')}
    mat_layer = None
    lm = mnode.find('LayerElementMaterial')
    if lm:
        mat_layer = {'mapping': lm.value('MappingInformationType')[0], 'ref': lm.value('ReferenceInformationType')[0],
                     'materials': [int(x) for x in lm.value('Materials')]}
    normals = None
    ln = mnode.find('LayerElementNormal')
    if ln:
        normals = {'mapping': ln.value('MappingInformationType')[0], 'ref': ln.value('ReferenceInformationType')[0],
                   'normals': ln.value('Normals')}
    # parent map + material order on the mesh (connection order = material index order)
    parent = {}
    mesh_mats = []
    cluster_bone = {}
    for c in conns:
        if c[0] != 'OO':
            continue
        child, par = c[1], c[2]
        ck, cn_ = child.split('::', 1)
        pk, pn_ = par.split('::', 1) if '::' in par else ('', par)
        if ck == 'Model' and pk == 'Model':
            parent[cn_] = pn_
        elif ck == 'Material' and pn_ == mname:
            mesh_mats.append(cn_)
        elif ck == 'Model' and pk == 'SubDeformer':
            cluster_bone[pn_] = cn_
    return {
        'models': models, 'mesh_name': mname, 'vertices': verts, 'polygons': polys, 'uv': uv,
        'material_layer': mat_layer, 'normals': normals, 'materials': mesh_mats, 'clusters': clusters,
        'cluster_bone': cluster_bone, 'parent': parent, 'bind_pose': bind_pose, 'connections': conns,
        'global_settings': root.find('Settings') or root.find('GlobalSettings'),
    }


if __name__ == '__main__':
    import sys
    d = load(sys.argv[1])
    v = d['vertices']
    xs, ys, zs = v[0::3], v[1::3], v[2::3]
    print('verts', len(v) // 3, 'polys', len(d['polygons']), 'poly sizes', sorted({len(p) for p in d['polygons']}))
    print('bbox x %.3f..%.3f y %.3f..%.3f z %.3f..%.3f' % (min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))
    print('materials', d['materials'])
    print('uv', d['uv']['mapping'], d['uv']['ref'], len(d['uv']['uv']) // 2, len(d['uv']['index'] or []))
    ml = d['material_layer']
    print('mat layer', ml['mapping'], ml['ref'], len(ml['materials']), sorted(set(ml['materials'])))
    if d['normals']:
        print('normals', d['normals']['mapping'], d['normals']['ref'], len(d['normals']['normals']) // 3)
    print('clusters', len(d['clusters']), 'bind pose nodes', len(d['bind_pose']))
    for nm, m in d['models'].items():
        bp = d['bind_pose'].get(nm)
        t = (bp[0][3], bp[1][3], bp[2][3]) if bp else None
        print('%-8s %-28s parent %-26s bind %s' % (m['type'], nm, d['parent'].get(nm), tuple(round(x, 3) for x in t) if t else None))
    for cnm, c in d['clusters'].items():
        tl = c['transform_link']
        t = c['transform']
        print('%-40s bone %-26s n %4d TL %s T %s' % (cnm, d['cluster_bone'].get(cnm), len(c['indexes']),
              tuple(round(tl[i][3], 3) for i in range(3)), tuple(round(t[i][3], 3) for i in range(3))))
