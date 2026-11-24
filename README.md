# selective-undo

**Live portfolio: https://yashgix.github.io/selective-undo/**

Collaborative undo for Tilo's resource store: a design and benchmark, Tilo's own implementation of the same idea, and the fixes that came from testing it.

When several people edit the same thing, undo should take back your action and keep what others did since. In Chat, Ada reacts, Ben reacts, and Ada undoes hers. The old undo made both reactions disappear.

| | Result |
|---|---|
| One undo on a document with 10,000 small changes, Tilo main | **26.0 s**, **33 ms** with a one-function fix (issue [#5993](https://code.tilo.so/tilo/tilo/issues/5993)) |
| Revert scenario tests that fail on Tilo main | **6 of 27**, none with the revert fix |
| Original work: generated edit histories that broke the old undo (1,300 independent of 2,000) | 678 before, 1 with my undo |
| Tilo's own new undo on the same benchmark | 0 of 1,300 |

The page `docs/index.html` tells the story with interactive demos and latency explorers. Timings come from a laptop under load, so read them as orders of magnitude.

**Status.** Open on Tilo's Forgejo: issue [#5993](https://code.tilo.so/tilo/tilo/issues/5993) with pull request [#5994](https://code.tilo.so/tilo/tilo/pulls/5994) for the speed fix, and issue [#5995](https://code.tilo.so/tilo/tilo/issues/5995) with pull request [#5996](https://code.tilo.so/tilo/tilo/pulls/5996) for the correctness fix. Nothing has been accepted or merged upstream yet.

## Engineering approach
**Exploring selective undo.** The project began with an investigation into selective undo in collaborative documents, focusing on how individual changes can be reversed while preserving other contributors' work.

**Building on Tilo's architecture.** As Tilo's undo implementation evolved, I adapted my research and regression tests to build on its new history-based architecture.

**Optimizing history traversal.** Performance profiling identified an opportunity to eliminate repeated history calculations. An incremental lookup reduced undo latency from 26 seconds to 33 milliseconds in the large-document benchmark.

**Strengthening correctness.** Additional collaboration scenarios helped identify edge cases involving concurrent counter updates, replaced maps and list counters. The resulting improvements preserve the existing architecture and pass all 27 regression tests.

Both contributions are now submitted for review.

## Contents
- `docs/index.html`: the write-up with two tabs (Overview, Technical details), a replay of the 26 s versus 33 ms undo, diagrams, interactive demos and a light and dark theme.
- `current/`: the latest contributions: patches, tests, the latency test, the generator, and how to run them on Tilo main (`current/README.md`).
- `historical/`: the original work in full: `patch/` (three stacked changes against Tilo `2fcb9946fe`) and `reproduce/` (the correctness benchmark script and the latency probe).
- `TECHNICAL.md`: architecture, design decisions, verification and environment notes.
- `data/results.json`: the numbers the page shows.
- `LICENSE`, `NOTICE.md`: MIT for the write-up, page and data. AGPL-3.0-only for the Tilo code the patches change and the tests that link it.

## Reproduce
- Latest findings on Tilo main: `current/README.md`.
- Original benchmark on the earlier code: `bash historical/reproduce/run.sh path/to/tilo-checkout` (a checkout at `2fcb9946fe` with the three patches applied). It reproduces the correctness results, not the latency numbers.

## Scope
- On Tilo main, if Ada's reaction created the reaction map and Ben reacts before she undoes, both reactions stay and her undo does nothing. Nothing is lost. My original work made this exact. It is documented, not proposed.
- Automerge records no operation when a value is written over an equal one, so undo has no later write to protect there.
- Verification details and what was left to Tilo's CI are in `TECHNICAL.md`.

## Author
Yash Gupta · [GitHub](https://github.com/yashgix) · [LinkedIn](https://www.linkedin.com/in/yash-guptaix/)
