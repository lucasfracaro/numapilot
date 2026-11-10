# entrada no X de teste: xin.py move X Y | click X Y | scroll X Y N (N<0 sobe) | key NOME
import sys, time
from Xlib import X, display
from Xlib.ext import xtest
d = display.Display()
a = sys.argv[1:]
i = 0
while i < len(a):
    c = a[i]
    if c == 'move':
        xtest.fake_input(d, X.MotionNotify, x=int(a[i+1]), y=int(a[i+2])); i += 3
    elif c == 'click':
        xtest.fake_input(d, X.MotionNotify, x=int(a[i+1]), y=int(a[i+2])); d.sync(); time.sleep(0.15)
        xtest.fake_input(d, X.ButtonPress, 1); d.sync(); time.sleep(0.05); xtest.fake_input(d, X.ButtonRelease, 1); i += 3
    elif c == 'scroll':
        xtest.fake_input(d, X.MotionNotify, x=int(a[i+1]), y=int(a[i+2])); d.sync()
        n = int(a[i+3]); b = 5 if n > 0 else 4
        for _ in range(abs(n)):
            xtest.fake_input(d, X.ButtonPress, b); d.sync(); xtest.fake_input(d, X.ButtonRelease, b); d.sync(); time.sleep(0.03)
        i += 4
    elif c == 'sleep':
        d.sync(); time.sleep(float(a[i+1])); i += 2
    else:
        i += 1
    d.sync()
