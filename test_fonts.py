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

print("\nItalic overhangs (check slash tips and f/j descenders):")
show("Slashes", "// CMD - base yellow", style=STYLES["italic"])
show("Letters", "ff jj fy fj / // ///", style=STYLES["italic"])
show("Style edges", f"|{STYLES['italic']}//fj{RESET}|{STYLES['italic']}f/j{RESET}|")
show("Color edges", "//fj", style=STYLES["italic"], color=COLORS["red"])
print(f"{STYLES['italic']}//{COLORS['red']}fj{COLORS['green']}//{RESET}")

print("\nBlock art (solid edges should meet without gaps):")
print("█▀▀█ █▀▀█ █▀▀▀ █▀▀█")
print("█  █ █▄▄█ █▀▀  █  █")
print("█▄▄█ █    █▄▄▄ █  █")
print("▁▂▃▄▅▆▇█ ▉▊▋▌▍▎▏ ▖▗▘▙▚▛▜▝▞▟")
print("Shades: ░░░▒▒▒▓▓▓███")
print("Sextants: " + "".join(chr(code) for code in range(0x1fb00, 0x1fb3c)))
print("Octants:")
for start in range(0x1cd00, 0x1cde6, 32):
    print("".join(chr(code) for code in range(start, min(start + 32, 0x1cde6))))

print("\nBox borders (connections should meet without gaps):")
print("┌────┬────┐  ┏━━━━┳━━━━┓")
print("│    │    │  ┃    ┃    ┃")
print("├────┼────┤  ┣━━━━╋━━━━┫")
print("│    │    │  ┃    ┃    ┃")
print("└────┴────┘  ┗━━━━┻━━━━┛")
print("╭─────────╮")
print("│ rounded │")
print("╰─────────╯")
print("╔════╦════╗  ╒════╤════╕  ╓────╥────╖")
print("║    ║    ║  │    │    │  ║    ║    ║")
print("╠════╬════╣  ╞════╪════╡  ╟────╫────╢")
print("╚════╩════╝  ╘════╧════╛  ╙────╨────╜")
print("Dashed: ┄┄┄ ┅┅┅ ┈┈┈ ┉┉┉ ╌╌╌ ╍╍╍ ┆┇┊┋╎╏")
print("Diagonals: ╱╲╳")
print("\nBraille indicators (dot columns should stay upright):")
show("Normal", "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ⣿ ⡇ ⢸")
show("Italic", "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ⣿ ⡇ ⢸", style=STYLES["italic"])
show("Italic border", "┌──┬──┐ │││ └──┴──┘", style=STYLES["italic"])

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
