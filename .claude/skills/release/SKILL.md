---
name: release
description: Semantic versioning contract, the end-of-session version bump + local make app, and the release procedure (tag, CI dev DMG, promote to prod) that runs only when Kai asks. Use when bumping the version, deciding whether a change is a patch/minor/major bump, or when Kai asks for a release. Also read at the end of a session that changed code.
---

# Ordnung releases (semver + procedure)

The single source of truth for the version is `[workspace.package] version` in
the root `Cargo.toml`. Everything else derives from it or from the git tag:
`tools/build-app.sh` stamps it into `Info.plist`, and the `Release` GitHub
workflow (`.github/workflows/release.yml`) builds the universal DMG and
publishes a GitHub Release whenever a `v*` tag is pushed.

## Channels: dev and prod

Every pushed tag replaces the rolling **`dev` prerelease**: the dev channel.
There is only ever one dev release (its tag is moved to the newest build), so
dev builds don't pile up on the Releases page. GitHub never makes a
prerelease `releases/latest`, and everything users install from follows
`latest` (the website download button in `docs/index.html`, the README link,
the in-app update check in `ordnung-core/src/update.rs`). So dev builds ship
as often as we like and users stay on the last prod build.

Prod is a promotion, not a rebuild: `make promote` runs
`.github/workflows/promote.yml`, which copies the DMGs from `dev` into a new
`vX.Y.Z` release (on the already-pushed tag) marked `latest`. Only the newest
dev build can be promoted; `make promote V=X.Y.Z` fails if `dev` holds a
different version. Only Kai decides when a build goes prod; never promote
unasked.

A prod release carries notes: after the promote, `gh release edit vX.Y.Z
--notes-file` with a `## What's new in X.Y.Z` section, then a `---` line,
then the install boilerplate (keep it: `gh release view vX.Y.Z --json body`
gives it). Bullets for what a DJ would notice since the previous prod
release, `**Lead.** sentence` each, newest work first, fixes last. The
landing page's "What's new" section reads that section from
`releases/latest` through the API, and its markup carries a copy of the
current notes as the no-script fallback, so refresh `docs/index.html`'s
`#rel` article with the same bullets.

## Semver contract

`MAJOR.MINOR.PATCH`, judged by what a DJ using the app would notice:

- **PATCH** — bug fixes, visual polish, performance work, docs/skills only.
  No new capability. `0.4.0 → 0.4.1`
- **MINOR** — a new user-visible feature or capability (new button, new
  analysis output, new export option). `0.4.0 → 0.5.0`
- **MAJOR** — reserved; only when the user explicitly asks (e.g. `1.0.0`),
  or a breaking change to on-disk data (catalog schema that can't migrate,
  cache invalidation that forces a full re-analysis).

When one release rolls up several commits, the highest-ranking change wins.
If the user names a version explicitly, use exactly that.

## Every session: bump and install locally

Work must already be verified in the running app and committed (see
CLAUDE.md). When the session's code changes are done:

1. Edit `[workspace.package] version` in the root `Cargo.toml` to the new
   `X.Y.Z`.
2. `cargo check -p ordnung-gui` — refreshes `Cargo.lock` with the new
   version and proves the workspace still builds.
3. Commit exactly these two files with the message
   `release: bump workspace version to X.Y.Z`. Working in a per-session
   worktree (see CLAUDE.md), first `git fetch && git rebase origin/main`,
   compile the rebased head, then fast-forward push; if the rebase pulled
   in a peer's feature, re-judge the bump (the highest-ranking change in
   the roll-up wins).
4. From that commit run `make app`, which builds, signs and installs the
   bundle to `/Applications` and relaunches it. The installed app must
   never lag `main`; also run it after fast-forwarding `main` at session
   start.

No tag and no CI run: a session publishes nothing on its own. Kai set
this on 2026-10-26 after a session's `v0.192.0` tag push started the
`Release` workflow and had to be cancelled (the tag stays; its version is
simply never built).

## Only when Kai asks: release

1. Tag the bumped commit and push the tag:
   `git tag vX.Y.Z && git push origin vX.Y.Z`. The `Release` workflow
   builds the universal DMG (`Ord-X.Y.Z-macos-universal.dmg`) and
   publishes it as the `dev` prerelease. Confirm it started with
   `gh run list --workflow=release.yml --limit 1` and report the outcome;
   if it fails, read the log with `gh run view <id> --log-failed` and fix
   before re-tagging. A manual `workflow_dispatch` run publishes nothing,
   so a version whose tag was pushed without a build needs a new patch
   version and tag.
2. Once the dev build is up, `make promote V=X.Y.Z` makes it `latest`,
   then write the release notes (see Channels).

Never re-use or move a tag that has already been pushed; a botched release
gets a new patch version.

## When to release

Only when Kai asks. Sessions never tag or publish on their own; a session
that changed code ends with the bump and `make app` above, so the
installed app keeps matching `main` (Kai, 2026-10-13: no CI build per
merged commit; 2026-10-26: none per session either, local builds only
until a prod release is requested). A session that changed only docs,
skills, tests or suite baselines needs no bump.
