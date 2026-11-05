# 21: Versions

## Goal and prerequisites

Automatic local history and first-class Git, as in [the desktop briefing](../DESKTOP_BRIEFING.md#versions). Requires milestone 18. See V1 and V2 on [the third mockup board](../mockups/03-review-versions.html). That board shows the commit card on `Cmd+S`; it moved to `Cmd+Shift+S`.

## Deliverables

1. **Local history.** Snapshots per file on a pause, labelled before each agent turn and each outside change, outside the project in the app data folder. 30 days, thinned with age. It works with no repo and on single files, and survives a crash. Moving a book must not lose its history without saying so. Choose how files are identified, and record it.
2. **Commit card.** `Cmd+Shift+S`: changed chapters with word counts, a message, commit, and commit and push.
3. **Timeline.** `Cmd+Y`: commits as tall marks and local history as small ticks on one strip. Scrubbing shows the chapter at that point, differences word by word in the book's type. Restore a passage or the chapter, which is an ordinary edit that can be undone.
4. **Git panel.** `Ctrl+Shift+G`: changes, message, branches, and history. A right panel in Write and a rail tab in Publish.
5. **Branches.** Create, switch, delete, and merge. Switching with unsaved text is safe, and an overlapping merge uses the existing conflict line.
6. **Push and pull.** Through the installed `git`, with Synced, Ahead, or Behind shown. Without `git` installed, local work keeps working and the panel says what is missing.
7. **Repos.** New projects start as one, with a warning for folders in iCloud or Dropbox. An existing folder is asked once, on its first commit.

Check gitoxide's current writing and merging support against the installed version before building on it. As last checked, it has no push. Record the dependency, and update `THIRD_PARTY_NOTICES.md`.

## Completion criteria

On a copy of the owner's real book, which has 138 commits: its history shows, a commit made here reads correctly in plain `git log`, no commit happens without a request, and local history adds nothing to the project folder or `git status`. Branch switching and merging through the app match what the `git` CLI reports. An hour of scripted edits keeps typing under 16 ms median, and local history's disk use is recorded. A restore from a week-old snapshot is byte-exact.
