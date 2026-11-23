#!/usr/bin/env python3
"""Make assets/fonts/GoogleSansFlex.ttf small: pin the axes dshell does not use (GRAD, ROND, slnt at 0, opsz at 18)
and drop the shaping tables ab_glyph does not read. Weight and width stay variable. 4 MB -> ~230 KB.
Usage: tools/reduce-font.py <GoogleSansFlex-VariableFont_GRAD,ROND,opsz,slnt,wdth,wght.ttf>   (needs fonttools)"""
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

f = TTFont(sys.argv[1])
out = instancer.instantiateVariableFont(f, {"GRAD": 0, "ROND": 0, "slnt": 0, "opsz": 18})
for t in ["GSUB", "GPOS", "GDEF", "STAT", "MVAR"]:
    if t in out:
        del out[t]
out.save("assets/fonts/GoogleSansFlex.ttf")
print("wrote assets/fonts/GoogleSansFlex.ttf")
