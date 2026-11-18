# Shared settings; each tape sources this. Run docs/demo/setup.sh first.
Require rpt
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 18
Set Width 1440
Set Height 780
Set Padding 12
Set TypingSpeed 70ms
Set Theme "Catppuccin Mocha"
Env COLORTERM "truecolor"
Env XDG_CONFIG_HOME "/tmp/rpt-demo/config"
Env XDG_STATE_HOME "/tmp/rpt-demo/state"
Hide
Type "export PATH=/tmp/rpt-demo/bin:$PATH PS1='$ '; cd /tmp/rpt-demo/acme-external; clear"
Enter
Show
