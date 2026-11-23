#!/usr/bin/env python3

RESET = "\033[0m"
STYLES = {
    "normal": "",
    "bold": "\033[1m",
    "dim": "\033[2m",
    "italic": "\033[3m",
    "underline": "\033[4m",
    "reverse": "\033[7m",
}

COLORS = {
    "red": "\033[31m",
    "green": "\033[32m",
    "yellow": "\033[33m",
    "blue": "\033[34m",
    "magenta": "\033[35m",
    "cyan": "\033[36m",
}

def show(label, text, style="", color=""):
    print(f"{label:12} {style}{color}{text}{RESET}")

sample = "Hello, café! ✨"

print("Terminal text styles:")
for name, code in STYLES.items():
    show(name, sample, style=code)

print("\nDifferent colors:")
for name, code in COLORS.items():
    show(name, sample, color=code)

print("\nUnicode font-like characters:")
show("Script", "𝓗𝓮𝓵𝓵𝓸 𝓦𝓸𝓻𝓵𝓭")
show("Fraktur", "𝔉𝔯𝔞𝔨𝔱𝔲𝔯")
show("Double-struck", "𝕯𝕠𝕦𝕓𝕝𝕖 𝕊𝕥𝕣𝕦𝕔𝕜")
show("Monospace", "𝙼𝚘𝚗𝚘𝚜𝚙𝚊𝚌𝚎")

print("\nOther:")
show("SMALL", "➜abc")
show("BIG", "➜ abc")
show("NERD SMALL", "terst")
show("NERD BIG", " test")
