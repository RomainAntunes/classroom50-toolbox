#!/bin/zsh
# Records the running app's power use (macOS): CPU, energy impact, memory, idle wakeups and
# threads of the app and its three WebKit helpers every 2 s, the gh and git processes it
# starts, the log lines it writes, and a system-wide energy snapshot at the end. Read-only:
# it only runs ps, top, pmset and reads the app's log. Ends early if the app quits (an
# update restarts it under a new pid).
# Usage: pnpm profile:power [minutes=10] [out=profile-out/<date>]
# Then:  node scripts/profile-power/report.mjs <out> [--compare <older out>]
set -euo pipefail

minutes=${1:-10}
out=${2:-profile-out/$(date +%Y-%m-%d_%H-%M-%S)}
log_file="$HOME/Library/Logs/io.github.romainantunes.classroom50-toolbox/Classroom 50 Toolbox.log"

if [[ $(uname) != Darwin ]]; then
  echo "profile:power runs on macOS only (top's energy impact)." >&2
  exit 1
fi

# `[[dd-]hh:]mm:ss` to seconds.
elapsed() {
  local e=$1 days=0 h=0 m=0 s=0
  [[ $e == *-* ]] && { days=${e%%-*}; e=${e#*-}; }
  local parts=(${(s.:.)e})
  case ${#parts} in
    3) h=${parts[1]}; m=${parts[2]}; s=${parts[3]} ;;
    2) m=${parts[1]}; s=${parts[2]} ;;
  esac
  echo $(( 10#$days * 86400 + 10#$h * 3600 + 10#$m * 60 + 10#$s ))
}

app=$(pgrep -n -x classroom50-toolbox || true)
if [[ -z $app ]]; then
  echo "Classroom 50 Toolbox isn't running." >&2
  exit 1
fi
app_age=$(elapsed $(ps -o etime= -p $app | tr -d ' '))

# WebKit's helpers aren't the app's children (launchd starts them): the ones started within
# 3 s of the app are its own.
helper() {
  local name=$1 best="" best_gap=4 pid gap
  for pid in $(pgrep -f "com.apple.WebKit.$name.xpc" || true); do
    gap=$(( $(elapsed $(ps -o etime= -p $pid | tr -d ' ')) - app_age ))
    (( gap < 0 )) && gap=$(( -gap ))
    if (( gap < best_gap )); then best=$pid; best_gap=$gap; fi
  done
  echo $best
}
gpu=$(helper GPU)
net=$(helper Networking)
web=$(helper WebContent)
pids=($app $gpu $net $web)

binary=$(ps -o comm= -p $app)
plist="${binary%/MacOS/*}/Info.plist"
if [[ -f $plist ]]; then
  version=$(defaults read "$plist" CFBundleShortVersionString 2>/dev/null || echo unknown)
else
  version="$(node -p 'require("./package.json").version' 2>/dev/null || echo unknown) (dev)"
fi

mkdir -p $out
cat > $out/meta.json <<EOF
{ "version": "$version", "app": $app, "gpu": ${gpu:-null}, "networking": ${net:-null}, "webcontent": ${web:-null}, "minutes": $minutes }
EOF
echo "Profiling $version: app $app, GPU ${gpu:-?}, Networking ${net:-?}, WebContent ${web:-?}, for $minutes min into $out"
[[ -z $gpu || -z $net || -z $web ]] && echo "Some WebKit helpers weren't found; recording the rest." >&2

log_start=$( [[ -f $log_file ]] && stat -f %z "$log_file" || echo 0 )
ps -o pid=,time=,rss= -p ${(j:,:)pids} > $out/cputime_start.txt
pmset -g batt > $out/batt_start.txt

top_args=()
for p in $pids; do top_args+=(-pid $p); done
top -l $(( minutes * 30 + 1 )) -s 2 $top_args -stats pid,command,cpu,mem,power,idlew,threads,time \
  > $out/top.txt 2>&1 &
top_pid=$!

# The gh and git processes the app starts, every 0.5 s; stops top if the app goes away.
(
  while kill -0 $top_pid 2>/dev/null; do
    if ! kill -0 $app 2>/dev/null; then
      date +%s > $out/ended_early.txt
      kill $top_pid 2>/dev/null || true
      break
    fi
    ps -axo pid=,ppid=,command= | awk -v P=$app -v t=$(date +%s) '$2==P {print t, $0}'
    sleep 0.5
  done > $out/children.txt
) &
wait $top_pid || true
wait

top -l 2 -s 5 -o power -n 15 -stats pid,command,cpu,power,idlew > $out/context.txt 2>&1
live=()
for p in $pids; do kill -0 $p 2>/dev/null && live+=($p); done
if (( ${#live} )); then
  ps -o pid=,time=,rss= -p ${(j:,:)live} > $out/cputime_end.txt
else
  : > $out/cputime_end.txt
fi
pmset -g batt > $out/batt_end.txt
if [[ -f $log_file ]]; then
  # A log rotated meanwhile starts over: keep the new file whole.
  size=$(stat -f %z "$log_file")
  if (( size >= log_start )); then
    tail -c +$(( log_start + 1 )) "$log_file" > $out/app.log
  else
    cp "$log_file" $out/app.log
  fi
  grep -h " sync: " "${log_file%.log}"*.log > $out/sync-history.log || true
fi
[[ -f $out/ended_early.txt ]] && echo "The app quit during the capture (an update?); the report covers what was recorded." >&2
echo "Done. Report: node scripts/profile-power/report.mjs $out"
