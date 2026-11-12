"""CodeBridge NSIS 설치 마법사 이미지 생성 (PIL) — 다크 브랜드."""
from PIL import Image, ImageDraw, ImageFont, ImageFilter
import math, os

BUILD = os.path.dirname(os.path.abspath(__file__))
ICON = os.path.join(BUILD, 'icon.png')
# 포인트 그라데이션 (gradA #58d6ff → gradB #7ee787)
CYAN = (88, 214, 255)
MINT = (126, 231, 135)
BG0 = (11, 16, 27)        # 최심해 네이비
BG1 = (20, 30, 51)        # 네이비
BG2 = (15, 22, 38)        # 중간 톤
TEXT = (230, 237, 246)
DIM = (139, 149, 169)

def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))

def gradient_bar(size, c0=CYAN, c1=MINT):
    w, h = size
    im = Image.new('RGB', (w, h))
    px = im.load()
    for x in range(w):
        t = x / max(w - 1, 1)
        col = lerp(c0, c1, t)
        for y in range(h):
            px[x, y] = col
    return im

def gradient_text(im, xy, text, font):
    mask = Image.new('L', im.size, 0)
    mdraw = ImageDraw.Draw(mask)
    mdraw.text(xy, text, font=font, fill=255)
    bbox = mask.getbbox()
    if not bbox:
        return
    grad = Image.new('RGB', (bbox[2] - bbox[0], bbox[3] - bbox[1]))
    gp = grad.load()
    gw, gh = grad.size
    for x in range(gw):
        t = x / max(gw - 1, 1)
        col = lerp(CYAN, MINT, t)
        for y in range(gh):
            gp[x, y] = col
    im.paste(grad, bbox[:2], mask.crop(bbox))

def font(name, size):
    for p in [
        r'C:\Windows\Fonts\segoeuib.ttf',
        r'C:\Windows\Fonts\segoeui.ttf',
        r'C:\Windows\Fonts\malgunbd.ttf',
        r'C:\Windows\Fonts\malgun.ttf',
    ]:
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                pass
    return ImageFont.load_default()

def kfont(size):
    for p in [r'C:\Windows\Fonts\malgunbd.ttf', r'C:\Windows\Fonts\malgun.ttf']:
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    return font('segoeui', size)

def vgradient(size, c0, c1):
    w, h = size
    im = Image.new('RGB', (w, h))
    px = im.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        col = lerp(c0, c1, t)
        for x in range(w):
            px[x, y] = col
    return im

def hgradient(size, c0, c1):
    w, h = size
    im = Image.new('RGB', (w, h))
    px = im.load()
    for x in range(w):
        t = x / max(w - 1, 1)
        col = lerp(c0, c1, t)
        for y in range(h):
            px[x, y] = col
    return im

def add_glow(im, center, radius, color, strength=255):
    layer = Image.new('L', im.size, 0)
    d = ImageDraw.Draw(layer)
    d.ellipse([center[0]-radius, center[1]-radius, center[0]+radius, center[1]+radius], fill=strength)
    layer = layer.filter(ImageFilter.GaussianBlur(radius // 2))
    tint = Image.new('RGB', im.size, color)
    im.paste(tint, (0, 0), layer)

logo = Image.open(ICON).convert('RGBA')

# ---------- installerHeader.bmp 150x57 ----------
w, h = 150, 57
im = hgradient((w, h), BG0, BG2)
# 배경에 얇은 그라데이션 라인(하단 강조)
bar = gradient_bar((w, 4), CYAN, MINT).filter(ImageFilter.GaussianBlur(0.5))
im.paste(bar, (0, h - 4))
d = ImageDraw.Draw(im)
# 왼쪽: 단어마크
f_brand = font('segoeuib', 15)
gradient_text(im, (10, 15), 'CodeBridge', f_brand)
# 오른쪽: 로고
lg = logo.resize((36, 36), Image.LANCZOS)
im.paste(lg, (w - 46, 8), lg)
im.save(os.path.join(BUILD, 'installer-header.bmp'), 'BMP')

# ---------- installerSidebar.bmp 164x314 ----------
w, h = 164, 314
im = vgradient((w, h), BG1, BG0)
# 중앙 광원 2개: 상단 시안 / 하단 민트
glow1 = Image.new('L', (w, h), 0)
d1 = ImageDraw.Draw(glow1)
d1.ellipse([-40, 10, 130, 150], fill=110)
glow1 = glow1.filter(ImageFilter.GaussianBlur(34))
cyan_tint = Image.new('RGB', (w, h), (34, 120, 160))
im.paste(cyan_tint, (0, 0), glow1)
glow2 = Image.new('L', (w, h), 0)
d2 = ImageDraw.Draw(glow2)
d2.ellipse([20, 190, 210, 330], fill=90)
glow2 = glow2.filter(ImageFilter.GaussianBlur(40))
mint_tint = Image.new('RGB', (w, h), (28, 110, 66))
im.paste(mint_tint, (0, 0), glow2)
# 로고
lg = logo.resize((96, 96), Image.LANCZOS)
im.paste(lg, ((w - 96) // 2, 44), lg)
# 단어마크 그라데이션
f_big = font('segoeuib', 22)
label = 'CodeBridge'
tw = d.textlength(label, font=f_big) if False else ImageDraw.Draw(im).textlength(label, font=f_big)
gradient_text(im, ((w - tw) / 2, 152), label, f_big)
# 태그라인 (한국어)
d = ImageDraw.Draw(im)
f_tag = kfont(10)
tag = '무료 AI 코딩 에이전트'
tw = d.textlength(tag, font=f_tag)
d.text(((w - tw) / 2, 188), tag, font=f_tag, fill=DIM)
# 얇은 그라데이션 바
bar = gradient_bar((96, 3), CYAN, MINT)
im.paste(bar, ((w - 96) // 2, 214))
# 하단 소글 (저장소 안내)
f_foot = font('segoeui', 7)
foot = 'github.com/skyb85993/codebridge'
tw = d.textlength(foot, font=f_foot)
d.text(((w - tw) / 2, h - 14), foot, font=f_foot, fill=(90, 100, 124))
im.save(os.path.join(BUILD, 'installer-sidebar.bmp'), 'BMP')

# ---------- installer/uninstaller ICO (48x48) ----------
for name in ('installer.ico', 'uninstaller.ico'):
    lg = logo.resize((48, 48), Image.LANCZOS)
    lg.save(os.path.join(BUILD, name), 'ICO', sizes=[(48, 48)])

print('done:', [f for f in os.listdir(BUILD) if f.endswith(('.bmp', '.ico'))])
