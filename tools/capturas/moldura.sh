#!/usr/bin/env bash
# moldura.sh ENTRADA SAIDA [RAIO]: cantos arredondados e sombra
R=${3:-12}
convert "$1" \( +clone -alpha extract -draw "fill black polygon 0,0 0,$R $R,0 fill white circle $R,$R $R,0" \( +clone -flip \) -compose Multiply -composite \( +clone -flop \) -compose Multiply -composite \) -alpha off -compose CopyOpacity -composite /tmp/rounded.png
convert /tmp/rounded.png \( +clone -background black -shadow 50x18+0+10 \) +swap -background white -layers merge +repage "$2"
