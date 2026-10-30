#!/usr/bin/env python3
"""Generate Coban-mod's code-native icon with Python's standard library."""
from pathlib import Path
import struct, zlib
ROOT = Path(__file__).resolve().parent.parent

def png(size):
    def rounded(x,y,left,top,right,bottom,radius):
        if not left<=x<=right or not top<=y<=bottom: return False
        dx=max(left+radius-x,0,x-(right-radius))
        dy=max(top+radius-y,0,y-(bottom-radius))
        return dx*dx+dy*dy<=radius*radius
    rows=[]
    for y in range(size):
        row=bytearray(b'\0')
        for x in range(size):
            px,py=(x+.5)*128/size,(y+.5)*128/size
            color=(199,243,107,255) if rounded(px,py,4,4,124,124,28) else (0,0,0,0)
            shapes=any((px-cx)**2+(py-37)**2<=12**2 for cx in (39,89))
            shapes |= any(rounded(px,py,kx,ky,kx+20,ky+18,4) for kx in (26,54,82) for ky in (61,87))
            if shapes: color=(24,34,26,255)
            row.extend(color)
        rows.append(row)
    def chunk(kind,data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',size,size,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(rows)))+chunk(b'IEND',b'')

def main():
    assets=ROOT/'assets';iconset=assets/'MK.iconset';iconset.mkdir(exist_ok=True)
    cache={s:png(s) for s in (16,32,48,64,128,256,512,1024)}
    for size in (16,32,128,256,512):
        (iconset/f'icon_{size}x{size}.png').write_bytes(cache[size])
        (iconset/f'icon_{size}x{size}@2x.png').write_bytes(cache[size*2])
    sizes=(16,32,48,256);offset=6+16*len(sizes);entries=[];images=[]
    for size in sizes:
        data=cache[size];entries.append(struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(data),offset));images.append(data);offset+=len(data)
    (assets/'mk.ico').write_bytes(struct.pack('<HHH',0,1,len(sizes))+b''.join(entries)+b''.join(images))
if __name__=='__main__':main()
