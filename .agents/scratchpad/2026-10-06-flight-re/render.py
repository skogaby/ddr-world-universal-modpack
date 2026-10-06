# Offline raycast of ported stage parts: skinned pose from the .anm, .sanm UV offsets,
# vertex colour, opaque / additive / alpha blending, wrapped bilinear texture sampling.
import sys, os, glob, struct, numpy as np
sys.path.insert(0,'scripts')
import ktmdl_dump as K, anm_dump as A
from PIL import Image
OUT=os.path.join(os.environ.get('TMPDIR','/tmp'),'opencode/s6/')
STAGE=os.environ.get('STAGE','HOTTEST PARTY 3/Stage 201/mapset_hp3stage201'); KEY=os.environ.get('KEY','hp3stage201')
D='data_mods/custom_models/stages/%s/'%STAGE
def dds(p):
    b=open(p,'rb').read(); h=struct.unpack_from('<7I',b,4); H,W=h[2],h[3]
    return np.frombuffer(b,np.uint8,W*H*4,128).reshape(H,W,4)[...,[2,1,0,3]].astype(np.float32)/255
def load(part, frame):
    pd=D+'gm_%s_%s/'%(KEY,part)
    m=K.parse_model(open(pd+'gm_%s_%s.model'%(KEY,part),'rb').read())
    texs={}
    for t in m['texnames']:
        n=t['name']; f=pd+n[:7]+'_'+n[7:]+'.dds'
        texs[n]=dds(f) if os.path.exists(f) else None
    W=[np.eye(4)]*len(m['bones'])
    ap=pd+'gm_%s_%s_play_loop.anm'%(KEY,part)
    if os.path.exists(ap):
        an=A.parse_anm(open(ap,'rb').read())
        pose=A.evaluate_pose(an, frame % max(1,an['header']['frame_count']), [b['parent'] for b in m['bones']])
        W=[np.array(p['world']).reshape(4,4) for p in pose]
    inv=[np.array(b['inverse_bind']).reshape(4,4) for b in m['bones']]
    offs={}
    sp=pd+'gm_%s_%s_play_loop.sanm'%(KEY,part)
    if os.path.exists(sp):
        d=open(sp,'rb').read(); s=A.parse_anm(d); fc=s['header']['frame_count']
        tg=[c for c in s['chunks'] if c['name']=='material_targets'][0]['entries']
        ident={mt['identity']:i for i,mt in enumerate(m['materials'])}
        for c in s['chunks']:
            for T in c.get('tracks',[]):
                mi=ident.get(tg[T['target']]['identity'])
                offs.setdefault(mi,[0,0,0,0])[T['sub']]=A.sample_track(d,T,frame%fc)[0]
    out=[]
    for me in m['meshes']:
        vs=K.read_vertices(m,me); idx=np.array(K.read_indices(m,me)).reshape(-1,3)
        P=[]
        for v in vs:
            p=np.append(v['POSITION'],1.0); acc=np.zeros(4)
            w=v.get('WEIGHTS4',[1,0,0,0]); bi=v.get('BLENDINDICES',[0,0,0,0])
            for k in range(4):
                if w[k]>0: b=me['palette'][bi[k]]; acc+=w[k]*(p@inv[b]@W[b])
            P.append(acc[:3])
        P=np.array(P); UV=np.array([v['TEXCOORD0'] for v in vs])
        C=np.array([v['COLOR0'] for v in vs],np.float32)/255 if 'COLOR0' in vs[0] else np.ones((len(vs),4),np.float32)
        o=offs.get(me['material'],[0,0,0,0]); UV=UV+np.array([o[2],o[3]])
        fl=me['flags']; f2=me.get('flags2',0)
        blend='add' if f2&4 else ('alpha' if fl&0x40 else 'opaque')
        tex=texs[m['texnames'][me['texture_slots'][0]]['name']]
        out.append(dict(part=part,mesh=me['index'],P=P,UV=UV,C=C,T=idx,blend=blend,tex=tex))
    return out
def sample(tex,uv):
    H,W=tex.shape[:2]; x=np.mod(uv[:,0],1)*W-0.5; y=np.mod(uv[:,1],1)*H-0.5
    x0=np.floor(x).astype(int); y0=np.floor(y).astype(int); fx=(x-x0)[:,None]; fy=(y-y0)[:,None]
    g=lambda yy,xx: tex[yy%H, xx%W]
    return (g(y0,x0)*(1-fx)+g(y0,x0+1)*fx)*(1-fy)+(g(y0+1,x0)*(1-fx)+g(y0+1,x0+1)*fx)*fy
def render(meshes, eye, fwd, up=(0,1,0), fov=76.8, Wd=480, Hd=270, only=None):
    eye=np.array(eye,float); f=np.array(fwd,float); f/=np.linalg.norm(f); r=np.cross(f,np.array(up,float)); r/=np.linalg.norm(r); u=np.cross(r,f)
    t=np.tan(np.radians(fov)/2); xs=np.linspace(-t,t,Wd); ys=np.linspace(t*Hd/Wd,-t*Hd/Wd,Hd)
    X,Y=np.meshgrid(xs,ys); Dr=(f[None,None]+X[...,None]*r+Y[...,None]*u).reshape(-1,3)
    frags=[]  # (depth, rgba, blend)
    for M in meshes:
        if only and (M['part'],M['mesh']) not in only: continue
        P=M['P']-eye
        for tri in M['T']:
            a,b,c=P[tri]; e1=b-a; e2=c-a
            h=np.cross(Dr,e2); det=h@e1; ok=np.abs(det)>1e-12
            inv=np.where(ok,1/np.where(ok,det,1),0); s=-a
            uu=(h@s)*inv; q=np.cross(s,e1); vv=(Dr@q)*inv; tt=(q@e2)*inv
            mk=ok&(uu>=0)&(vv>=0)&(uu+vv<=1)&(tt>0)
            if not mk.any(): continue
            ii=np.nonzero(mk)[0]; bu=uu[ii,None]; bv=vv[ii,None]
            uv=M['UV'][tri[0]]+(M['UV'][tri[1]]-M['UV'][tri[0]])*bu+(M['UV'][tri[2]]-M['UV'][tri[0]])*bv
            col=M['C'][tri[0]]+(M['C'][tri[1]]-M['C'][tri[0]])*bu+(M['C'][tri[2]]-M['C'][tri[0]])*bv
            rgba=(sample(M['tex'],uv) if M['tex'] is not None else 1)*col
            frags.append((ii,tt[ii],rgba,M['blend']))
    img=np.zeros((len(Dr),3),np.float32); depth=np.full(len(Dr),np.inf)
    for ii,tt,rgba,bl in frags:
        if bl=='opaque':
            m=tt<depth[ii]; img[ii[m]]=rgba[m,:3]; depth[ii[m]]=tt[m]
    trans=[(ii,tt,rgba,bl) for ii,tt,rgba,bl in frags if bl!='opaque']
    # per-pixel back-to-front: flatten
    if trans:
        I=np.concatenate([x[0] for x in trans]); Tt=np.concatenate([x[1] for x in trans]); R=np.concatenate([x[2] for x in trans])
        Bl=np.concatenate([np.full(len(x[0]),x[3]=='add') for x in trans])
        o=np.lexsort((-Tt,I)); I,Tt,R,Bl=I[o],Tt[o],R[o],Bl[o]
        vis=Tt<depth[I]; I,R,Bl=I[vis],R[vis],Bl[vis]
        start=np.r_[0,np.nonzero(np.diff(I))[0]+1]; rank=np.arange(len(I))-np.repeat(start,np.diff(np.r_[start,len(I)]))
        for k in range(rank.max()+1 if len(rank) else 0):
            m=rank==k; p=I[m]; a=R[m,3:4]; c=R[m,:3]
            img[p]=np.where(Bl[m,None], img[p]+c*a, img[p]*(1-a)+c*a)
    return (np.clip(img,0,1)*255).astype(np.uint8).reshape(Hd,Wd,3)

def camera(path, frame):
    d=open(path,'rb').read(); a=A.parse_anm(d)
    slots={}
    for c in a['chunks']:
        for T in c.get('tracks',[]):
            slots[T['target']]=A.sample_track(d,T,frame)
    q=slots[0]; pos=np.array(slots[1])*0.01
    R=np.array(A.quat_to_rowmat(q))
    target=pos-10*R[2,:3]; up=R[1,:3]/np.linalg.norm(R[1,:3])
    fov=slots[2][0]; asp=slots[5][0]
    tp=np.tan(0.5*np.arctan2(2, 2*np.tan(fov*np.pi/360)*asp))
    return pos, target-pos, up, 2*np.degrees(np.arctan(tp)), a['header']['frame_count']

def render_cam(meshes, path, frame, Wd=480, Hd=270):
    eye,fwd,up,hfov,_=camera(path,frame)
    return render(meshes, eye, fwd, up, fov=hfov, Wd=Wd, Hd=Hd)
