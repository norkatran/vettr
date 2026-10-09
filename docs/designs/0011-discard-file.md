# 0011 Discard file changes

Status: Fulfilled (not yet compiled or run: no Rust toolchain was available when written)

## Problem
The only way to throw changes away was the global "Discard all" command. A single file could not be reverted from the review.

## Decision
The 3-dot menu in each file header gets "Discard changes..." (hidden in read-only views). It opens a confirmation window; confirming runs `git_discard_files`: `git reset` for the paths (drops staged state), then `git checkout` for files tracked in HEAD or `git clean -fd` for new/untracked ones. Renames pass both paths. Staged and unstaged changes to the file are both discarded, whichever group the menu was opened from. It is irreversible, hence the confirmation.

Out of scope: discarding single hunks or lines, undo.

## To do
- [x] `discard_files` in host/git.rs with a test
- [x] Backend and model op
- [x] Menu item and confirmation window
- [ ] Compile and try it (cargo was unavailable)
