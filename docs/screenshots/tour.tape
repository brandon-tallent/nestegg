# Takes the screenshots in the README from the built-in demo household.
# Nothing here reads a database. From the repository root:
#
#   cargo install --path tui --locked
#   vhs docs/screenshots/tour.tape
#   for shot in docs/screenshots/*.png; do
#     magick "$shot" -strip +dither -colors 128 -define png:compression-level=9 "PNG8:$shot"
#   done
#
# The last step is ImageMagick making each file about a third of the size.
# The data is the same on every run but follows the calendar, so the dates
# and the latest few weeks will have moved on.
#
# https://github.com/charmbracelet/vhs

# VHS always records a video as well; this one is ignored
Output docs/screenshots/tour.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 16
Set Width 1584
Set Height 880
Set Padding 16
Set Theme { "name": "nestegg", "black": "#0e1622", "red": "#f06469", "green": "#2ec486", "yellow": "#f0be50", "blue": "#5aa0fa", "magenta": "#a582f5", "cyan": "#3cbec8", "white": "#e2e8f0", "brightBlack": "#7d8a9c", "brightRed": "#f06469", "brightGreen": "#2ec486", "brightYellow": "#f0be50", "brightBlue": "#5aa0fa", "brightMagenta": "#a582f5", "brightCyan": "#3cbec8", "brightWhite": "#ffffff", "background": "#0e1622", "foreground": "#e2e8f0", "selection": "#263244", "cursor": "#0e1622" }

Hide
Type "nestegg --demo"
Enter
Sleep 2s
Show

# A screenshot is the next frame drawn, so each one is given a moment before
# the next key
Screenshot docs/screenshots/dashboard.png
Sleep 500ms

Type "2"
Sleep 500ms
Screenshot docs/screenshots/accounts.png
Sleep 500ms

Type "3"
Sleep 500ms
Screenshot docs/screenshots/transactions.png
Sleep 500ms

Type "/"
Type "groceries >100"
Sleep 500ms
Screenshot docs/screenshots/search.png
Sleep 500ms
Escape

# Last month: the current one is only part of the way through
Type "4"
Type "h"
Sleep 500ms
Screenshot docs/screenshots/cashflow.png
Sleep 500ms

Type "5"
Sleep 500ms
Screenshot docs/screenshots/reports.png
Sleep 500ms

Type "6"
Sleep 500ms
Screenshot docs/screenshots/budget.png
Sleep 500ms

Type "7"
Sleep 500ms
Screenshot docs/screenshots/recurring.png
Sleep 500ms

Type "9"
Sleep 500ms
Screenshot docs/screenshots/investments.png
Sleep 500ms

Type "0"
Sleep 500ms
Screenshot docs/screenshots/equity.png
Sleep 500ms

# The first grant, opened
Enter
Sleep 500ms
Screenshot docs/screenshots/equity-grant.png
Sleep 500ms

Escape
Type "q"
