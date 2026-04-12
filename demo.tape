# omnifs demo: GitHub as a filesystem
# Run: vhs demo.tape

Output demo.mp4

Set Shell "bash"
Set FontFamily "Iosevka Term"
Set FontSize 14
Set Width 1920
Set Height 1080
Set Padding 20
Set TypingSpeed 35ms
Set Theme "GruvboxDarkHard"
Set Framerate 60

# enter the container
Hide
Type "docker exec -it omnifs-shell /bin/zsh"
Enter
Sleep 2s
Show

Sleep 500ms
Type "echo '>>> entering the github universe'"
Enter
Sleep 500ms

Type "cd /github"
Enter
Sleep 1s

Type "ll"
Enter
Sleep 1s

Type "echo '>>> let s go to a repo!'"
Enter
Sleep 500ms

Type "cd NousResearch"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 2s

Type "cd hermes-agent"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

# issues

Type "echo '>>> let s explore issues!'"
Enter
Sleep 500ms

Type "cd _issues"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

Type "cd _open"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 2s

# read an issue

Type "echo '>>> reading an issue...'"
Enter
Sleep 500ms

Type "cd 3926"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

Type "cat title"
Enter
Sleep 1s

Type "cat body"
Enter
Sleep 2s

Type "cd .."
Enter
Sleep 500ms

# rg search

Type "echo '>>> find all open issues mentioning memory'"
Enter
Sleep 500ms

Type 'rg -in "memory" */title */body --heading --color=always'
Enter
Sleep 3s

# PRs

Type "echo '>>> now let s look at PRs!'"
Enter
Sleep 500ms

Type "cd ../../_prs"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

Type "cd _open"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 2s

Type "echo '>>> reading a PR...'"
Enter
Sleep 500ms

Type "cd 7226"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

Type "cat title"
Enter
Sleep 1s

Type "cat body"
Enter
Sleep 2s

Type "cat state"
Enter
Sleep 1s

Type "cat diff | head -40"
Enter
Sleep 3s

# CI runs

Type "echo '>>> and finally, CI runs!'"
Enter
Sleep 500ms

Type "cd /github/NousResearch/hermes-agent/_actions/runs"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 2s

Type "echo '>>> checking a run...'"
Enter
Sleep 500ms

Type "cd 24264068866"
Enter
Sleep 500ms

Type "ll"
Enter
Sleep 1s

Type "cat status"
Enter
Sleep 1s

Type "cat conclusion"
Enter
Sleep 1s

Type "cat log | head -40"
Enter
Sleep 3s

# outro

Type "echo '>>> no API scripts. no SDK. just cd and cat.'"
Enter
Sleep 3s
