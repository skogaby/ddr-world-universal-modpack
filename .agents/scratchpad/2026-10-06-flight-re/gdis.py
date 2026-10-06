import sys, os, re, struct, capstone
p=os.environ.get('DLL') or os.path.expandvars('$DDR_WORLD_INSTALL/modules/gamemdx.dll')
b=open(p,'rb').read()
pe=struct.unpack_from('<I',b,0x3c)[0]
nsec=struct.unpack_from('<H',b,pe+6)[0]; opt=struct.unpack_from('<H',b,pe+20)[0]
base=struct.unpack_from('<Q',b,pe+24+24)[0]
secs=[]
for i in range(nsec):
    o=pe+24+opt+40*i
    name=b[o:o+8].rstrip(b'\0').decode(); vs,va,rs,ra=struct.unpack_from('<IIII',b,o+8)
    secs.append((name,va,vs,ra,rs))
def rva2off(r):
    for n,va,vs,ra,rs in secs:
        if va<=r<va+max(vs,rs): return r-va+ra
def off2rva(o):
    for n,va,vs,ra,rs in secs:
        if ra<=o<ra+rs: return o-ra+va
def find(pat):
    rx=re.compile(b''.join(b'.' if t=='??' else re.escape(bytes([int(t,16)])) for t in pat.split()), re.S)
    return [off2rva(m.start()) for m in rx.finditer(b)]
md=capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
def ins(rva, n):
    o=rva2off(rva); return list(md.disasm(b[o:o+n], base+rva))
def dis(rva, n):
    for i in ins(rva,n): print('%x: %-8s %s' % (i.address-base, i.mnemonic, i.op_str))
def strings(s):
    out=[]; i=b.find(s)
    while i>=0: out.append(off2rva(i)); i=b.find(s,i+1)
    return out
def xrefs_lea(target):
    # RIP-relative LEA / MOV to target
    res=[]
    for n,va,vs,ra,rs in secs:
        if n!='.text': continue
        for i in range(ra, ra+rs-7):
            if b[i] in (0x48,0x4c) and b[i+1]==0x8d and (b[i+2]&0xc7)==0x05:
                d=struct.unpack_from('<i',b,i+3)[0]; r=off2rva(i)
                if r+7+d==target: res.append(r)
    return res
