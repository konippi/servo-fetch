# servo-fetch demo (Regenerate with: `vhs demo.tape`)

Output ./demo.gif

Require servo-fetch

# Visual setup
Set Shell "bash"
Set FontSize 24
Set LetterSpacing 0
Set LineHeight 1.3
Set Width 1600
Set Height 900
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

# Mac-style terminal window chrome
Set Margin 40
Set MarginFill "#181825"
Set BorderRadius 14
Set WindowBar Colorful
Set WindowBarSize 40

# Land the loop thumbnail on the rendered output
Set LoopOffset 70%

# Clean prompt and screen
Hide
Type `export PS1='$ '`
Enter
Type "clear"
Enter
Show

# Demo
Type "servo-fetch https://servo.org/blog/2026/04/13/servo-0.1.0-release/ | glow -"
Sleep 300ms
Enter
Sleep 3s
