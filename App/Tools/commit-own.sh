#!/bin/zsh
# Commits only the files named, in a checkout other agents share: a private index from HEAD plus these files as they
# are on disk (their modes kept), a commit on it, and the branch moved only if nobody moved it meanwhile. Others'
# staged and unstaged work stays as it was. Afterwards the shared index forgets these paths' staged state.
#   Tools/commit-own.sh <message file> <file>…      paths relative to the repository's root or absolute
# A file another agent also changed: commit your hunks only — build the blob yourself (git show HEAD:f, apply your
# hunks, git hash-object -w) and git update-index --cacheinfo into a private index; this script takes whole files.
emulate -L zsh
setopt err_exit
[[ $# -ge 2 && -f $1 ]] || { sed -n '2,7p' $0 | sed 's/^# //'; exit 2 }
msg=${1:A}; shift
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
cd $(git -C ${0:A:h} rev-parse --show-toplevel)
files=(${@:A})
files=(${files#$PWD/})
ref=$(git symbolic-ref HEAD)
base=$(git rev-parse HEAD)
idx=$(mktemp -t commit-own); trap "rm -f $idx" EXIT
GIT_INDEX_FILE=$idx git read-tree $base
GIT_INDEX_FILE=$idx git add -- $files
[[ -n $(GIT_INDEX_FILE=$idx git diff --cached --name-only $base) ]] || { echo "nothing changed in these files" >&2; exit 1 }
tree=$(GIT_INDEX_FILE=$idx git write-tree)
new=$(git commit-tree $tree -p $base -F $msg)
git update-ref $ref $new $base || { echo "$ref moved meanwhile: run again on the new HEAD" >&2; exit 1 }
git reset -q HEAD -- $files
git --no-pager log --oneline --stat -1 $new
