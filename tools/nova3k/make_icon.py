# Nova3K launcher icon: dark-blue disc, glowing cyan four-point "nova" star, orbit ring.
from PIL import Image, ImageDraw, ImageFilter, ImageChops
import math, sys
S = 4 * 512                      # draw at 4x, downscale for smooth edges
C = S / 2
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

# background disc: radial gradient deep navy -> midnight blue
bg = Image.new("RGBA", (S, S))
px = bg.load()
R = S * 0.48
for y in range(0, S, 2):
    for x in range(0, S, 2):
        d = math.hypot(x - C, y - C * 0.9) / R
        t = min(d, 1.0)
        col = (int(18 + 20 * (1 - t)), int(40 + 70 * (1 - t)), int(80 + 90 * (1 - t)), 255)
        for dy in (0, 1):
            for dx in (0, 1):
                px[x + dx, y + dy] = col
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).ellipse([C - R, C - R, C + R, C + R], fill=255)
img.paste(bg, (0, 0), mask)

# rim
ImageDraw.Draw(img).ellipse([C - R, C - R, C + R, C + R], outline=(90, 230, 255, 255), width=int(S * 0.012))

# tilted orbit ring (ellipse rotated), drawn on its own layer
ring = Image.new("RGBA", (S, S), (0, 0, 0, 0))
rd = ImageDraw.Draw(ring)
rw, rh = S * 0.38, S * 0.13
rd.ellipse([C - rw, C - rh, C + rw, C + rh], outline=(120, 200, 255, 200), width=int(S * 0.010))
ring = ring.rotate(-28, resample=Image.BICUBIC, center=(C, C))
ring_m = Image.new("L", (S, S), 0)
ImageDraw.Draw(ring_m).ellipse([C - R * 0.97, C - R * 0.97, C + R * 0.97, C + R * 0.97], fill=255)
ring.putalpha(ImageChops.multiply(ring.getchannel("A"), ring_m))
img.alpha_composite(ring)

def star(r_long, r_short, rot=0):
    pts = []
    for i in range(8):
        a = math.radians(rot + i * 45)
        r = r_long if i % 2 == 0 else r_short
        pts.append((C + r * math.sin(a), C - r * math.cos(a)))
    return pts

# glow
glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(glow)
gd.polygon(star(S * 0.36, S * 0.07), fill=(60, 220, 255, 255))
gd.ellipse([C - S * 0.12, C - S * 0.12, C + S * 0.12, C + S * 0.12], fill=(60, 220, 255, 255))
glow = glow.filter(ImageFilter.GaussianBlur(S * 0.035))
img.alpha_composite(glow)

# secondary small diagonal star
d = ImageDraw.Draw(img)
d.polygon(star(S * 0.17, S * 0.035, rot=45), fill=(150, 235, 255, 255))
# main star: cyan body, white core
d.polygon(star(S * 0.34, S * 0.065), fill=(70, 225, 255, 255))
d.polygon(star(S * 0.25, S * 0.04), fill=(235, 252, 255, 255))
d.ellipse([C - S * 0.035, C - S * 0.035, C + S * 0.035, C + S * 0.035], fill=(255, 255, 255, 255))

# small sparkles
for (fx, fy, fr) in [(0.27, 0.30, 0.012), (0.74, 0.70, 0.010), (0.70, 0.27, 0.008), (0.30, 0.73, 0.007)]:
    x, y, r = fx * S, fy * S, fr * S
    d.ellipse([x - r, y - r, x + r, y + r], fill=(200, 245, 255, 230))

out = img.resize((512, 512), Image.LANCZOS)
out.save(sys.argv[1], optimize=True)
print("saved", out.size)
