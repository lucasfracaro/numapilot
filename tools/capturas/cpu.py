import time, math, random, os
busy=[0]*88; tot=[0]*88; t=0
def load(c,t):
    s=(c//22)%2; core=c%22; ht=c>=44
    if s==0 and core<16: return 0.88+0.1*math.sin(t/7+c)      # llama-server no socket 0
    if s==1 and core<6: return 0.35+0.25*math.sin(t/5+c)       # jogo no socket 1
    return max(0,0.04+0.04*math.sin(t/3+c*1.7)+(0.25 if random.random()<0.03 else 0))
while os.path.exists('/tmp/vary.on'):
    t+=1
    for c in range(88):
        l=min(1,max(0,load(c,t))); busy[c]+=int(100*l); tot[c]+=100
    lines=["cpu  %d 0 0 %d 0 0 0 0 0 0"%(sum(busy),sum(tot)-sum(busy))]
    for c in range(88):
        lines.append("cpu%d %d 0 0 %d 0 0 0 0 0 0"%(c,busy[c],tot[c]-busy[c]))
    open('/tmp/fakeproc/stat.tmp','w').write("\n".join(lines)+"\n"); os.replace('/tmp/fakeproc/stat.tmp','/tmp/fakeproc/stat')
    time.sleep(1)
