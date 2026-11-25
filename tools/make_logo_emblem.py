import numpy as np, cv2
from PIL import Image, ImageDraw, ImageFilter
exec(open('make_logo.py').read().split("def gold(")[0])   # S, logo, clean()

CX=517   # מרכז אופטי: מרכז הסוגריים הוא 520, מוזז מעט שמאלה כי קיר הצד והצל נוספים מימין (כמו בחץ המקורי)

# רמפת צבע הפנים: נדגמה מהחץ בסמל "מסייע ההורדה" (קואורדינטות 1024)
RAMP_Y=[300,360,408,480,536,592,640]
RAMP_C=np.array([(238,203,128),(243,217,141),(247,231,155),(252,238,163),(250,230,152),(249,226,150),(247,222,146)],float)
RIM=np.array([253,243,175.])
# קיר הצד (מהחלק התחתון של הפס): מהפנים החוצה
WALL_T=[0,0.12,0.45,0.85,1.0]
WALL_C=np.array([(196,148,84),(146,98,52),(132,86,42),(108,64,26),(66,28,6)],float)

def shapes():
    import math
    k=2; W=S*k
    m=Image.new('L',(W,W),0); d=ImageDraw.Draw(m)
    th=math.radians(40); L=200; hl=88; hw=70; sw=58      # אורך חץ, אורך ראש, חצי-רוחב ראש, רוחב גזע
    tipy=505; tipdx=7
    def arrow(sign):
        T=(CX+sign*-tipdx, tipy)                                  # קצה החץ (שני החצים נפגשים במרכז)
        dv=(-sign*-math.sin(th), math.cos(th))                    # כיוון: מטה, פנימה
        dv=(sign*math.sin(th)*-1*-1, math.cos(th))
        dv=(-sign*math.sin(th)*-1, math.cos(th)) if False else (sign*math.sin(th), math.cos(th))
        pv=(math.cos(th), -sign*math.sin(th)*-1) if False else (math.cos(th)*(-sign), math.sin(th)*1)
        def P(xl,yl): return ((T[0]+xl*pv[0]+yl*dv[0])*k,(T[1]+xl*pv[1]+yl*dv[1])*k)
        pts=[P(0,0),P(hw,-hl),P(sw/2,-hl),P(sw/2,-L),P(-sw/2,-L),P(-sw/2,-hl),P(-hw,-hl)]
        d.polygon(pts,fill=255)
    arrow(-1); arrow(1)
    # עיגול פינות: טשטוש וחיתוך סף
    a=np.array(m.resize((S,S),Image.LANCZOS)).astype(np.float32)/255
    a=cv2.GaussianBlur(a,(0,0),3.2); a=np.clip((a-0.5)*6+0.5,0,1)
    out=Image.fromarray((a*255).astype(np.uint8)).resize((W,W),Image.LANCZOS)
    dd=ImageDraw.Draw(out)
    # הקובץ המאוחד: פס שטוח כמו במסייע
    dd.rounded_rectangle(((CX-118)*k,(tipy+34)*k,(CX+118)*k,(tipy+34+28)*k),radius=8*k,fill=255)
    return np.array(out.resize((S,S),Image.LANCZOS)).astype(float)/255

def render(m, T=20):
    face=(m>0.5).astype(np.uint8)
    # קיר: הזזה מטה-ימינה
    t_idx=np.zeros((S,S),np.float32); cov=face.copy().astype(bool)
    for t in range(1,T+1):
        dx=int(round(t*0.45)); sh=np.zeros_like(face); sh[t:,dx:]=face[:S-t,:S-dx]
        new=(sh>0)&(~cov); t_idx[new]=t/T; cov|=new
    wall=cov&(face==0)
    # צבע הפנים
    TR=[0,.12,.38,.7,.95,1.0]
    TC=np.array([(238,203,128),(242,208,132),(245,228,152),(251,237,162),(250,230,152),(249,226,150)],float)
    n,lab,stats,_=cv2.connectedComponentsWithStats(face,connectivity=8)
    tt=np.full((S,S),0.9,np.float32)
    for i in range(1,n):
        y0,hh=stats[i,cv2.CC_STAT_TOP],stats[i,cv2.CC_STAT_HEIGHT]
        if hh>80:
            sel=lab==i; ys=np.nonzero(sel)[0]; tt[sel]=(ys-y0)/max(hh-1,1)
    fc=np.stack([np.interp(tt,TR,TC[:,i]) for i in range(3)],-1)
    dist=cv2.distanceTransform(face,cv2.DIST_L2,3)
    rim=np.clip(1-(dist-0.5)/4.5,0,1)*(face>0)
    fc=fc*(1-0.7*rim[...,None])+RIM*0.7*rim[...,None]
    # צבע הקיר
    wc=np.stack([np.interp(t_idx,WALL_T,WALL_C[:,i]) for i in range(3)],-1)
    alpha=np.clip(cv2.GaussianBlur(cov.astype(np.float32),(0,0),0.8),0,1)
    rgb=np.where(face[...,None]>0,fc,wc)
    # החלקת קצוות המסכה המקורית
    a=np.maximum(m,np.clip(cv2.GaussianBlur(wall.astype(np.float32),(0,0),0.8),0,1))
    img=np.dstack([np.clip(rgb,0,255),a*255]).astype(np.uint8)
    return Image.fromarray(img,'RGBA'), cov

def build():
    base=clean(logo)
    m=shapes(); g,cov=render(m)
    sh=Image.new('RGBA',(S,S),(80,62,40,0)); sh.putalpha(Image.fromarray((cov*255*0.55).astype(np.uint8)))
    sh=sh.transform((S,S),Image.AFFINE,(1,0,-6,0,1,-14)).filter(ImageFilter.GaussianBlur(9))
    out=Image.alpha_composite(base,sh); out=Image.alpha_composite(out,g)
    return out

if __name__=='__main__':
    o=build(); o.save('logo_merge3.png')
    ref1=Image.open('/tmp/ref/Otzaria-Download-Assistant-windows.png').convert('RGBA').resize((330,330),Image.LANCZOS)
    ref2=Image.open('/tmp/ref/Otzaria-Updates.png').convert('RGBA').resize((330,330),Image.LANCZOS)
    sheet=Image.new('RGBA',(1500,440),(240,232,222,255))
    sheet.alpha_composite(o.resize((420,420),Image.LANCZOS),(10,10))
    sheet.alpha_composite(ref1,(450,55)); sheet.alpha_composite(ref2,(800,55))
    x=1160
    for sz in (96,48,32,16): sheet.alpha_composite(o.resize((sz,sz),Image.LANCZOS),(x,60)); x+=sz+16
    sheet.save('compare3.png')
