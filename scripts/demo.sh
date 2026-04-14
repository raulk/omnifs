#!/usr/bin/env zsh
# omnifs demo: the image bakes this script to `/tmp/demo.sh`.
# In the local dev container, the repo copy is also bind-mounted at `/tmp/demo.sh`
# so you can iterate on the script without rebuilding the image.

alias ls="ls --color=always"

bat() {
    batcat --style=plain --paging=never "$@"
    local rc=$?
    print
    return $rc
}

C_AQUA=$'\033[1;38;2;131;192;146m'
C_RESET=$'\033[0m'

show_prompt() { print -nP "%F{yellow}%/%f\n%F{cyan}>%f " }

trace_command() {
    local cmd=$1
    local trace_file=${OMNIFS_CMD_TRACE:-/tmp/omnifs-cmd.trace}
    local start_ns=$(date +%s%N)
    print -r -- "START ${start_ns} ${PWD} ${cmd}" >> "$trace_file"
    eval $cmd
    local rc=$?
    local end_ns=$(date +%s%N)
    print -r -- "END ${end_ns} ${rc} $(( (end_ns - start_ns) / 1000000 ))ms ${cmd}" >> "$trace_file"
    return $rc
}

type_and_run() {
    local cmd=$1
    local prev=""
    for (( i=1; i<=${#cmd}; i++ )); do
        local c=${cmd[$i]}
        print -rn -- "$c"
        if [[ $c == " " || $c == "|" || $c == "/" ]]; then
            sleep 0.$(( RANDOM % 8 + 8 ))
        elif [[ $prev == " " ]]; then
            sleep 0.0$(( RANDOM % 30 + 50 ))
        elif [[ $c == $prev ]]; then
            sleep 0.0$(( RANDOM % 10 + 15 ))
        else
            sleep 0.0$(( RANDOM % 20 + 25 ))
        fi
        prev=$c
    done
    sleep 0.2
    print
    trace_command "$cmd"
    show_prompt
    sleep ${2:-0.6}
}

type_and_run_fast() {
    local cmd=$1
    for (( i=1; i<=${#cmd}; i++ )); do
        print -rn -- "${cmd[$i]}"
        sleep 0.0$(( RANDOM % 5 + 8 ))
    done
    sleep 1.5
    print
    trace_command "$cmd"
    show_prompt
    sleep ${2:-0.6}
}

act() {
    clear
    print
    local lines=("${(@f)$(gum style --bold --border rounded --margin "1 2" --padding "2 4" --border-foreground 142 --foreground 142 "$1")}")
    for line in "${lines[@]}"; do
        print -r -- "$line"
        sleep 0.05
    done
    print
    show_prompt
    sleep 1
}

run_smoke_demo() {
    set -euo pipefail

    local owner=${OMNIFS_DEMO_OWNER:-raulk}
    local requested_repo=${OMNIFS_DEMO_REPO:-omnifs}
    local repo_root="/github/${owner}"

    print -r -- "omnifs smoke demo: /github/${owner}/${requested_repo}"

    cd "/github/${owner}"
    ls

    if [[ -d $requested_repo ]]; then
        cd "$requested_repo"
        repo_root=$PWD
    elif [[ -d _issues && -d _prs ]]; then
        repo_root=$PWD
    else
        local discovered_repo=""
        local candidate
        for candidate in *; do
            if [[ -d $candidate && $candidate != _* ]]; then
                discovered_repo=$candidate
                break
            fi
        done

        [[ -n $discovered_repo ]]
        cd "$discovered_repo"
        repo_root=$PWD
    fi

    ls

    cd "${repo_root}/_issues/_open"
    ls
    local first_issue
    first_issue=$(command ls -1 | head -n 1)
    [[ -n $first_issue ]]
    cd "$first_issue"
    bat title
    [[ -f body ]] && bat -l md body

    cd "${repo_root}/_prs/_open"
    ls
    local first_pr
    first_pr=$(command ls -1 | head -n 1)
    [[ -n $first_pr ]]
    cd "$first_pr"
    bat title
    bat state

    if cd "${repo_root}/_actions/runs" 2>/dev/null; then
        ls
        local first_run
        first_run=$(command ls -1 | head -n 1)
        if [[ -n $first_run ]]; then
            cd "$first_run"
            [[ -f status ]] && bat status
            [[ -f conclusion ]] && bat conclusion
        fi
    fi
}

if [[ ${OMNIFS_DEMO_MODE:-full} == smoke ]]; then
    run_smoke_demo
    exit 0
fi

clear
sleep 1

# act 1: navigation

act "omnifs: the universe, mounted on your filesystem

Plan 9 was right. the filesystem was always the right abstraction.
they just didn't have the APIs worth mounting yet.

every service on Earth, mounted as files.
starting with GitHub. cd into any repo.

for humans. for agents."

type_and_run "cd /github" 0.3
type_and_run "ls -lrt" 0
type_and_run "# nothing here... yet!" 0.5
type_and_run "cd NousResearch" 0.3
type_and_run "ls" 2

# act 2: source code

act "source code, automatically checked out
the repo is cloned lazily on first access. git just works."

type_and_run "cd hermes-agent" 0.1
type_and_run "ls"
type_and_run "cd _repo" 0.3
type_and_run "ls" 0.8
type_and_run "git --no-pager log --oneline -n 1" 1
type_and_run_fast "cd .."

# act 3: issues

act "issues are just files
cat a title. grep a thousand bodies. ripgrep across everything."

type_and_run "cd _issues" 0.5
type_and_run "ls" 0.6
type_and_run "cd _open" 0.3
type_and_run "ls" 0.8
type_and_run "cd 3926" 0.3
type_and_run "bat title" 0.5
type_and_run "bat -l md body" 1.5
type_and_run "cd .." 0.3
type_and_run_fast 'rg -in memory */title */body --heading --color=always' 2
type_and_run_fast 'cd ../..'

# act 4: pull requests

act "PRs are just files too
read the diff. check the state. it is all just text."

type_and_run "cd _prs" 0.5
type_and_run "ls" 0.5
type_and_run "cd _open" 0.3
type_and_run "ls" 0.8
type_and_run "cd 7226" 0.3
type_and_run "bat title" 0.5
type_and_run "bat state" 0.3
type_and_run "bat diff" 2
type_and_run_fast "cd ../../.."

# act 5: CI

act "even GitHub Actions runs
why open a browser when you can cat a CI log directly?"

type_and_run "cd _actions/runs" 0.3
type_and_run "ls" 0.8
type_and_run "cd 24264068866" 0.3
type_and_run "ls" 0.8
type_and_run "bat status" 0.3
type_and_run "bat conclusion" 0.5
type_and_run "bat log | head -1000" 2

# outro

clear
gum style --bold --border double --padding "1 3" --border-foreground 142 --foreground 142 \
    "omnifs" \
    "" \
    "the universe, mounted on your filesystem." \
    "" \
    "no API. no SDK. no MCP. no CLI. just files." \
    "if your agent can read a file, it already speaks everything."
sleep 3
