"""Build the kvm product icon for macOS and Windows from one scalable design.
Requires Pillow. Output paths are build products; no binary fonts or assets needed.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
from math import sin,cos,pi

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/"dist"/"branding"
OUT.mkdir(parents=True,exist_ok=True)
N=1024

def icon():
    im=Image.new("RGBA",(N,N),(0,0,0,0))
    clip=Image.new("L",(N,N),0)
    ImageDraw.Draw(clip).rounded_rectangle((28,28,N-28,N-28),radius=220,fill=255)
    backdrop=Image.new("RGBA",(N,N),(0,0,0,0))
    d=ImageDraw.Draw(backdrop)
    for y in range(N):
        t=y/(N-1)
        # cobalt-blue upper rim, deep navy bottom
        rgb=(int(5+17*(1-t)),int(35+64*(1-t)),int(91+75*(1-t)))
        d.line((0,y,N,y),fill=(*rgb,255))
    backdrop.putalpha(clip)
    im.alpha_composite(backdrop)
    d=ImageDraw.Draw(im,"RGBA")
    d.rounded_rectangle((31,31,N-31,N-31),radius=210,outline=(44,164,255,210),width=11)
    # two screens, offset in depth
    d.rounded_rectangle((103,214,556,672),radius=44,fill=(14,35,85,255),outline=(173,218,255,245),width=13)
    d.rounded_rectangle((135,247,523,631),radius=24,fill=(20,150,246,255))
    d.polygon([(135,420),(269,352),(410,478),(523,410),(523,631),(135,631)],fill=(7,77,185,205))
    d.polygon([(135,548),(270,464),(446,610),(523,555),(523,631),(135,631)],fill=(22,85,212,235))
    d.rounded_rectangle((541,295,933,673),radius=40,fill=(14,35,88,255),outline=(187,212,255,255),width=12)
    d.rounded_rectangle((571,328,905,633),radius=22,fill=(42,92,223,255))
    d.polygon([(571,483),(713,412),(812,515),(905,450),(905,633),(571,633)],fill=(24,63,190,215))
    # sleek stands
    d.polygon([(285,669),(376,669),(388,734),(411,750),(251,750),(278,735)],fill=(121,162,216,255))
    d.polygon([(692,670),(770,670),(782,737),(796,752),(669,752),(685,736)],fill=(115,156,212,255))
    d.rounded_rectangle((232,739,432,764),radius=10,fill=(170,202,243,255))
    d.rounded_rectangle((655,742,814,763),radius=10,fill=(166,193,238,255))
    # cursor movement trail using cubic Bezier
    points=[]
    p0=(350,490);p1=(493,338);p2=(628,680);p3=(771,458)
    for i in range(101):
        t=i/100;u=1-t
        x=u**3*p0[0]+3*u*u*t*p1[0]+3*u*t*t*p2[0]+t**3*p3[0]
        y=u**3*p0[1]+3*u*u*t*p1[1]+3*u*t*t*p2[1]+t**3*p3[1]
        points.append((int(x),int(y)))
    glow=Image.new("RGBA",im.size,(0,0,0,0))
    ImageDraw.Draw(glow).line(points,fill=(67,232,255,255),width=23,joint="curve")
    glow=glow.filter(ImageFilter.GaussianBlur(20))
    im=Image.alpha_composite(im,glow)
    d=ImageDraw.Draw(im)
    d.line(points,fill=(237,255,255,255),width=15,joint="curve")
    d.line(points,fill=(35,214,255,255),width=5,joint="curve")
    d.polygon([(752,426),(795,538),(772,524),(756,561),(736,550),(754,512),(727,510)],fill=(250,255,255,255))
    # keyboard symbol near bottom -- clean bars instead of tiny legends
    d.rounded_rectangle((199,821,628,896),radius=22,fill=(111,162,222,255),outline=(187,226,255,255),width=4)
    for row in range(3):
        for col in range(9):
            x=220+col*43
            y=831+row*18
            d.rounded_rectangle((x,y,x+35,y+11),radius=4,fill=(25,57,110,255))
    # mouse silhouette
    d.ellipse((684,801,804,914),fill=(127,166,219,255),outline=(191,222,255,255),width=4)
    d.arc((709,815,786,900),198,325,fill=(58,221,255,255),width=7)
    return im

im=icon()
im.save(OUT/"kvm.png",optimize=True)
im.save(OUT/"kvm.ico",sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
iconset=OUT/"kvm.iconset"
iconset.mkdir(exist_ok=True)
for size in (16,32,128,256,512):
    im.resize((size,size),Image.Resampling.LANCZOS).save(iconset/f"icon_{size}x{size}.png")
    im.resize((size*2,size*2),Image.Resampling.LANCZOS).save(iconset/f"icon_{size}x{size}@2x.png")
print(f"Created icon PNG/ICO and macOS iconset in {OUT}")
