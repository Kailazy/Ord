# Handoff — next session

Read first: `CLAUDE.md`, the `ordnung-roadmap` skill (section "Backlog
(2026-10-15 review)"), `ordnung-architecture`, `audio-analysis`, and the
memories `beatgrid-roadmap-state`, `one-worktree-per-session`-style rules in
CLAUDE.md. Work in your own worktree with its own CARGO_TARGET_DIR.

Do the backlog in order, one pushed commit per verified item, one release at
the end (`release` skill):

1. **Inspector edit wipe.** Reproduce: edit a field in the inspector without
   saving, let any job finish (e.g. a cover fetch), watch the edit vanish.
   Root cause is `refresh_selected` running after every job; fix in the one
   shared place (skip refresh while the inspector holds dirty state, or merge
   instead of replace). Verify with `make run` in an isolated HOME over
   `testdata/seeker-sample` (memory `isolated-gui-environment`).

2. **Kick-attack grid refinement** in `ordnung-core/src/analysis/tempo.rs`.
   Spec is in the roadmap backlog item 2. Prototype as a post-pass after the
   tempo is fixed; evaluate with `grid_eval` against `testdata/rekordbox-grids`
   (release build, ~8 s/run). Target: lift the 12/17 in-phase count without
   losing any of the 12; the 5 misses are domina, Monolense, metapattern,
   RB208, tekra. Bump analyzer version only if the shipped output changes.
   Do not tune on the 17 alone beyond this structural change.

3. **Catalog backup/restore** in core (`ordnung-core`), a menu entry in the
   GUI. Zip db+wal+shm, analysis, artwork; embed `restore.sh`; restore parks
   replaced files in a dated folder next to the db. Test on a catalog copy,
   never `~/.ordnung` live.

4. **Harmonic key view** if time remains: reuse `song_rows` and the existing
   Camelot colour code; a filter chip on the current song's neighbours, not a
   new table.

Report what shipped, what was measured (grid counts before/after), and what
you left out and why.
