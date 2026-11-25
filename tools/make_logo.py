import numpy as np
from PIL import Image, ImageDraw, ImageFilter
S=1024
logo=Image.open('iconnew.png').convert('RGBA')

def clean(logo):
    import cv2
    a=np.array(logo.convert('RGB')).astype(float)
    top=np.array([231,233,226.]); bot=np.array([228,225,222.])
    t=np.clip((np.arange(S)[:,None]-240)/430,0,1)[...,None]
    fill=top*(1-t)+bot*t; fill=np.broadcast_to(fill,(S,S,3))
    mask=np.zeros((S,S),np.uint8); mask[294:654,376:668]=255; mask[242:294,396:650]=255
    m=(cv2.GaussianBlur(mask,(0,0),13)/255.0)[...,None]
    m=np.clip((m-0.12)*1.5,0,1)
    out=a*(1-m)+fill*m
    res=np.dstack([np.clip(out,0,255),np.array(logo)[...,3]]).astype(np.uint8)
    return Image.fromarray(res,'RGBA')

def gold(mask_img, bbox_top, bbox_bot, light=(-0.55,-0.83)):
    """צובע מסכה בזהב עם שיפוע, שפה משופעת והדגשה"""
    m=np.array(mask_img.resize((S,S),Image.LANCZOS)).astype(float)/255
    h=np.array(Image.fromarray((m*255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(7))).astype(float)/255
    gy,gx=np.gradient(h)
    s=-(gx*light[0]+gy*light[1])*9            # חיובי = פונה לאור
    s=np.clip(s,-1,1)
    yy=np.linspace(0,1,S)[:,None]*np.ones((1,S))
    t=np.clip((yy*S-bbox_top)/(bbox_bot-bbox_top),0,1)
    top=np.array([243,212,118.]); mid=np.array([212,163,58.]); bot=np.array([160,112,28.])
    base=np.where(t[...,None]<0.5, top*(1-t[...,None]*2)+mid*(t[...,None]*2), mid*(1-(t[...,None]-0.5)*2)+bot*((t[...,None]-0.5)*2))
    shade=1+0.80*s[...,None]
    col=base*shade
    hi=np.clip(s-0.45,0,1)[...,None]*1.5
    col=col+hi*np.array([255,246,205])*0.55
    edge=np.clip((0.55-h)/0.55,0,1)*(m>0.02)       # קרוב לשפה = כהה יותר
    col=col*(1-0.38*edge[...,None])
    col=np.clip(col,0,255)
    out=np.dstack([col,m*255]).astype(np.uint8)
    return Image.fromarray(out,'RGBA')

def shadow(mask_img, dy=9, blur=7, alpha=0.38):
    m=mask_img.resize((S,S),Image.LANCZOS)
    sh=Image.new('RGBA',(S,S),(70,45,10,0)); sh.putalpha(m.point(lambda v:int(v*alpha)))
    sh=sh.transform((S,S),Image.AFFINE,(1,0,0,0,1,-dy)).filter(ImageFilter.GaussianBlur(blur))
    return sh

def draw_emblem():
    k=2; W=S*k
    m=Image.new('L',(W,W),0); d=ImageDraw.Draw(m)
    def rr(x0,y0,x1,y1,r): d.rounded_rectangle((x0*k,y0*k,x1*k,y1*k),radius=r*k,fill=255)
    def poly(pts): d.polygon([(x*k,y*k) for x,y in pts],fill=255)
    def line(pts,w):
        d.line([(x*k,y*k) for x,y in pts],fill=255,width=int(w*k),joint='curve')
        for x,y in pts: d.ellipse(((x-w/2)*k,(y-w/2)*k,(x+w/2)*k,(y+w/2)*k),fill=255)
    # שני חלקים עליונים: כל אחד שתי שורות טקסט
    for x0,x1 in ((346,484),(540,678)):
        rr(x0,258,x1,288,12); rr(x0,306,x1-36,336,12)
    # מיזוג: שני קווים מתכנסים לחץ אחד
    line([(415,358),(415,402),(512,460)],38); line([(609,358),(609,402),(512,460)],38)
    line([(512,460),(512,514)],42)
    poly([(436,506),(588,506),(512,586)])
    # הקובץ המאוחד: שורות ארוכות יותר
    rr(362,602,662,634,12); rr(362,646,662,674,12)
    return m.resize((S,S),Image.LANCZOS)

def build():
    base=clean(logo)
    m=draw_emblem()
    g=gold(m,255,670)
    sh=shadow(m)
    out=Image.alpha_composite(base,sh); out=Image.alpha_composite(out,g)
    return out
if __name__=='__main__':
    o=build(); o.save('logo_merge.png'); print('ok')
    prev=Image.new('RGBA',(1100,560),(240,232,222,255)); prev.alpha_composite(o.resize((512,512),Image.LANCZOS),(20,20)); prev.alpha_composite(clean(logo).resize((512,512),Image.LANCZOS),(560,20)); prev.save('prev.png')
