# Technical notes

Architecture, design decisions, findings and the verification record behind the page and the patches. Hashes refer to commits of `code.tilo.so/tilo/tilo`.

## 1. How undo works in Tilo now
`Store::undo` (crate `tilo-resource-store`) takes back one invocation. Since `dd0f8bbc37` it calls `history::revert::revert_stamped` for each document the invocation changed. The history engine itself moved into the store crate in `a6758222e2`.

The revert works on a document's change history:
1. `first_index_after` locates the invocation in the history: it finds the prefix whose heads equal the heads after the invocation.
2. The revert computes the inverse of the invocation's patches.
3. It collects what later changes wrote (`Later`): which slots were written, by whom, and what text was typed.
4. `classify` decides per patch whether to write it or keep the other author's value (`takes_back`), and reports the slots it kept.
5. The patches that remain are written as one new change.

The property it aims for: undoing an invocation that nothing later depends on gives the same document as replaying the history without it.

## 2. The original design (20 to 23 Nov, kept in `historical/`)
Built on Tilo as of `2fcb9946fe`, where `Store::undo` restored the old value of every key an invocation changed. If someone else had edited a neighbouring part of the same value, their edit went too.
- An optional `ContentUndo` hook let the workspace service plug in `revert_changes`. The store sat below the collaboration crate then, so the hook was injected.
- Fixes in the revert itself: counters several people increment, replaced maps, blocks built on by others, and a linear-time lookup.
- A store rule for map fields held directly in a resource, which made the Chat reaction case exact.
- A seeded generator of edit histories (2,000, of which 1,300 have no later edit depending on the undone one) that checks the property above, and 86 tests.
- Results on that code: 678 of 1,300 histories broke before, 1 after (a list-order choice). One undo on 10,000 items and 10,000 small changes took 33 s in the first version and 0.19 s after the lookup became one pass, against 0.60 s for the store's undo then.

Tilo's own change removed the layering problem the hook worked around, so the hook became unnecessary.

## 3. Findings on Tilo main
**Speed (issue #5993).** `first_index_after` called `prefix_heads` for every prefix, and each call read all change metadata. One undo on a document written by 10,000 separate invocations took 26.0 s. The fix keeps the set of heads while walking the history once (`current/first-index-after-one-pass.diff`, 8 lines). The same lookup had been the main cost in the original work.

**Counter step beside another author's step.** Ada adds 5, Ben adds -2, Ada undoes: the counter stays 3, expected -2. The revert treated Ben's step as overwriting Ada's. Counter steps add up, so it should not. The failure needs a second author: with a single store where Ben's step is simply the next invocation, `Store::undo` already gives -2.

**Replaced map.** After an invocation replaced a map and another author wrote into the new one, undo kept the new map but wrote the older map's key into it.

**List counter, reporting only.** A list counter another author replaced or deleted was reported as 0 slots kept instead of 1. Nothing is lost.

The fix (`current/corr-pr.patch`) tracks increments apart from overwrites, names a list element by its cursor and holds back keys under a container that stays. It leaves `revert_stamped` and `takes_back` as they are.

## 4. Verification record
Tilo's tests and the checks below ran in an offline Docker copy of upstream with the original dependency cache, using plain `cargo`, not Tilo's `t` build server.

| Check | Result |
|---|---|
| 27 revert scenario tests (23 plain documents, 4 synced stores) on unmodified `b06ae3fe8e` | 6 fail: 3 counter, 1 replaced map, 2 list counter |
| The same tests with the correctness fix | 27 pass |
| Generator, 2,000 histories (1,300 independent), correctness fix alone and with the speed fix | 0 violations, 0 replay errors |
| `tilo-resource-store` full suite, speed fix | 22 library and 59 integration tests pass |
| `tilo-resource-store` full suite, correctness fix | passes (scratch timing test excluded) |
| `tilo-collaboration` document tests, `tilo-ui` history tests, `undo_the_agents_work` | 17, 38 and 1 pass |
| rustfmt, Clippy `-D warnings` on the store library and new test files | clean, both fixes |
| Latency, one undo, 10,000 items and 10,000 small changes | 26.0 s before, 33 ms after |

Rerun on current upstream main (`21936d4e81`, 24 Nov, 347 commits after `b06ae3fe8e`; `revert.rs` identical to the version the patches target), same offline copy and dependency cache:

| Check | Result |
|---|---|
| 27 scenario tests, unmodified | 6 fail |
| 27 scenario tests with the correctness fix, alone and with the speed fix | 27 pass |
| Generator (2,000 histories, 1,300 independent), unmodified, speed fix, correctness fix, both | 0 violations, 0 replay errors in each |
| `tilo-resource-store` full suite: speed fix, correctness fix, both | 22 library and 59 integration tests pass in each (the scenario tests fail only in the speed-fix-alone run, as expected) |
| rustfmt and Clippy `-D warnings` on the store library and new test files, both fixes together | clean (no allowance in `tilo-doc` needed on this commit) |

The `tilo-collaboration`, `tilo-ui` and `undo_the_agents_work` rows above ran on `b06ae3fe8e` and were not repeated on the newer commit. The latency numbers are from `b06ae3fe8e`.

### Notes on the environment
- Unmodified `b06ae3fe8e` fails Clippy in `crates/sdk/tilo-doc/src/table_source.rs` (3 `arithmetic_side_effects` errors). To lint the store, that one lint was allowed in `tilo-doc` in the scratch copy. The patches do not touch `tilo-doc`.
- The scratch copy has `tools/font-catalog.py` removed, because the font crate's build script downloads fonts and the build copy is offline.
- Timings come from a laptop under load (macOS storage services at 85 to 163% of a core, load average 5 to 7), release builds without LTO. They show orders of magnitude and scaling.
- `tilo-session-core` tests did not compile on `b06ae3fe8e` because of an upstream test type error and were not rerun. The UI and collaboration tests ran before a final Clippy-driven type alias edit in the correctness fix. The full workspace suite and CI belong to Tilo's own pipeline.

### Reproduction
See `current/README.md` for the commands, patches and tests.
