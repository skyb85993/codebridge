"""CodeBridge NSIS 설치 마법사 이미지 생성 (PIL)."""
from PIL import Image, ImageDraw, ImageFont, ImageFilter
import math, os

BUILD = os.path.dirname(os.path.abspath(__file__))
ICON = os.path.join(BUILD, 'icon.png')
# 색상 (앱의 gradA #58d6ff → gradB #7ee787)
CYAN = (88, 214, 255)
MINT = (126, 231, 135)
BG0 = (16, 22, 34)      # 패널
BG1 = (10, 14, 20)      # 배경
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
    # 그라디언트 텍스트: 텍스트 마스크 기반으로 cyan→mint 알파 합성
    mask = Image.new('L', im.size, 0)
    mdraw = ImageDraw.Draw(mask)
    mdraw.text(xy, text, font=font, fill=255)
    bbox = mask.getbbox()
    if not bbox:
        return
    grad = Image.new('RGB', (bbox[2]-bbox[0], bbox[3]-bbox[1]))
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

logo = Image.open(ICON).convert('RGBA')

# ---------- installerHeader.bmp 150x57 ----------
w, h = 150, 57
im = Image.new('RGB', (w, h), BG0)
d = ImageDraw.Draw(im)
# 좌측 로고
lg = logo.resize((40, 40), Image.LANCZOS)
im.paste(lg, (8, 9), lg)
# 상측 텍스트
f = font('segoeuib', 17)
d.text((56, 10), 'CodeBridge', font=f, fill=TEXT)
# 하단 그라디언트 바 (설치 진행 느낌)
bar = gradient_bar((w - 8, 5), CYAN, MINT).filter(ImageFilter.GaussianBlur(0.5))
im.paste(bar, (4, h - 7))
im.save(os.path.join(BUILD, 'installer-header.bmp'), 'BMP')

# ---------- installerSidebar.bmp 164x314 ----------
w, h = 164, 314
im = Image.new('RGB', (w, h))
px = im.load()
for y in range(h):
    t = y / h
    # BG1 -> BG0 세로 그라디언트 + 중앙 발광
    col = lerp(BG1, BG0, t)
    for x in range(w):
        cx = 1 - abs(x - w / 2) / (w / 2)
        glow = int(18 * cx * (1 - abs(t - 0.42) / 0.6))
        px[x, y] = tuple(min(255, c + glow) for c in col)
d = ImageDraw.Draw(im)
# 큰 로고
lg = logo.resize((110, 110), Image.LANCZOS)
im.paste(lg, ((w - 110) // 2, 52), lg)
# 워드마크 그라디언트
f_big = font('segoeuib', 25)
label = 'CodeBridge'
tw = d.textlength(label, font=f_big)
gradient_text(im, ((w - tw) / 2, 176), label, f_big)
# 태그라인
f_small = font('segoeui', 9)
tag = 'C O D I N G   A G E N T'
tw = d.textlength(tag, font=f_small)
d.text(((w - tw) / 2, 208), tag, font=f_small, fill=DIM)
# 하단 가는 그라디언트 바
bar = gradient_bar((w - 32, 3), CYAN, MINT)
im.paste(bar, (16, 260))
im.save(os.path.join(BUILD, 'installer-sidebar.bmp'), 'BMP')

# ---------- installer/uninstaller ICO (48x48) ----------
for name in ('installer.ico', 'uninstaller.ico'):
    lg = logo.resize((48, 48), Image.LANCZOS)
    lg.save(os.path.join(BUILD, name), 'ICO', sizes=[(48, 48)])

print('done:', [f for f in os.listdir(BUILD) if f.endswith(('.bmp', '.ico'))])
