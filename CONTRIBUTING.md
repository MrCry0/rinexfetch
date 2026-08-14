# Contributing to rinexfetch

Thanks for considering a contribution. This document describes the
expected workflow for proposing changes, and the standards a change
needs to meet before it can be merged.

## Workflow

1. Fork the repository.
2. Branch from `main` in your fork. Name the branch after what it
   does, not who's doing it, e.g. `fix-nav-fallback-panic` or
   `add-obs-retry-backoff`, not `patch-1` or your username.
3. If your change fixes a bug, make sure an issue for it already
   exists upstream (open one if it doesn't) before you start work.
4. Make your change as one or more atomic commits (see below), then
   push the branch to your fork.
5. Open a pull request from your branch to this repository's `main`
   branch. If the PR fixes a bug, link the upstream issue in the PR
   description (`Closes #123` or `Fixes #123`).
6. Keep your branch up to date with `main` by rebasing, not merging
   (see below). Never force-push over review comments without saying
   so.

## Commits

### Atomicity

Each commit should be a self-contained, working checkpoint: it builds,
its tests pass, and its own description matches what it actually
changes. Don't split a single logical change across multiple commits,
and don't bundle unrelated changes into one commit. If you find
yourself writing "and" in a commit subject, it's probably two commits.

### Message format

Follow the Linux kernel convention:

```
subsystem: short description of the change
```

- Imperative mood: "add", "fix", "remove", not "added", "fixes".
- 72 characters maximum for the subject line, no trailing period.
- The subsystem prefix names the part of the codebase most affected
  (e.g. `cddis:`, `rinex_merge:`, `ci:`, `docs:`).
- Separate the subject from the body with one blank line.
- The body explains *why* the change is needed, not what the diff
  already shows. Note any non-obvious tradeoffs or decisions.
- Wrap the body at 72 characters.

Example:

```
rinex_merge: fall through past malformed nav candidates, not just 404s

CDDIS occasionally serves a 200 response with a truncated or corrupt
RINEX file for a given day. Previously this aborted the whole nav
fallback chain instead of trying the next candidate, so a single bad
upstream file made fetching fail outright even though older data was
available.

Catch the parse failure and continue to the next fallback tier, the
same way an outright 404 is already handled.

Closes #12
```

### Sign-off

Every commit must include a `Signed-off-by` trailer (the Developer
Certificate of Origin). Use `git commit -s`, which adds it
automatically from your configured `user.name` and `user.email`.

### No merge commits

This project does not use merge commits, anywhere, including to bring
`main` into a feature branch. If your branch falls behind `main`,
update it with:

```
git fetch origin
git rebase origin/main
```

Resolve any conflicts as part of the rebase, then force-push your
branch (`git push --force-with-lease`).

## Before you open a pull request

Run the same checks CI runs, so review isn't spent on things a
machine can catch:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --all-targets
cargo test
```

All of CI must be green before a pull request can be merged:
`Lint`, `Build & test` on Linux/macOS/Windows, and packaging
(`Package (deb)`, `Package (rpm)`, `Package (msi)`,
`Package (macOS .pkg)`).

## Dependencies

This project is licensed GPL-3.0-only. Any new dependency must have a
license compatible with that before it can be added; check and state
the license in your PR description if you're introducing one.

## Code of conduct

Be respectful and constructive in issues, pull requests, and reviews.
Disagree with the change, not the person.
