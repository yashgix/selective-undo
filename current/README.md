# Latest contributions on Tilo main

Two changes to `crates/core/resource-store/src/history/revert.rs`, found by carrying the original benchmark and tests over to Tilo's own undo (`Store::undo` calling the history's revert).

- Speed: one undo took 26.0 s on a document with 10,000 small changes, and takes 33 ms with the fix. Issue [#5993](https://code.tilo.so/tilo/tilo/issues/5993), pull request [#5994](https://code.tilo.so/tilo/tilo/pulls/5994).
- Correctness: a counter step is not taken back beside another author's step, and a replaced map's old key comes back. 6 of 27 scenario tests fail on Tilo main, none with the fix. Issue [#5995](https://code.tilo.so/tilo/tilo/issues/5995), pull request [#5996](https://code.tilo.so/tilo/tilo/pulls/5996).

Both patches apply to `revert.rs` as of `f43996c7aa` and later. They were tested on `b06ae3fe8e` and `21936d4e81`. See `../TECHNICAL.md` for the verification record.

## Files
- `first-index-after-one-pass.diff`: the speed fix, 8 lines in `revert.rs`.
- `corr-pr.patch`: the correctness change as submitted in pull request #5996: `revert.rs`, two `mod` lines in `tests/it.rs` (the crate's existing `it` integration test) and the two new test files. No `Cargo.toml` change.
- `corr-revert-main.diff`: the `revert.rs` part of that patch. `corr-and-perf-revert-main.diff`: the same on top of the speed fix.
- `corr-tests/`: `revert_focus.rs` (23 tests on plain documents) and `merge_focus.rs` (4 tests with synced stores).
- `undo_latency.rs`: the latency test from the issue.
- `selective_undo.rs`: the seeded generator and correctness benchmark.

## Run the speed measurement
In a checkout of Tilo main (`SU` is the path to this repository):
1. Copy `undo_latency.rs` to `crates/core/resource-store/tests/`.
2. Register it in `crates/core/resource-store/Cargo.toml`:
   ```toml
   [[test]]
   name = "undo_latency"
   path = "tests/undo_latency.rs"
   ```
3. Run it without and with the fix (`git apply $SU/current/first-index-after-one-pass.diff`):
   ```bash
   cargo test --release -p tilo-resource-store --test undo_latency -- --exact undo_latency --nocapture --test-threads=1
   ```
   It prints one median per size and shape. On the laptop used here, 10,000 items written as 10,000 changes took 26.0 s on main and 33 ms with the fix. Timings depend on the machine.

## Run the correctness tests
```bash
git apply $SU/current/corr-pr.patch
cargo test -p tilo-resource-store --test it -- revert_focus:: merge_focus::
```
All 27 pass (the tests run inside the `it` target, the filters select them). To see the failures, undo only the `revert.rs` change (`git apply -R $SU/current/corr-revert-main.diff`): 6 fail, 3 for counters, 1 for the replaced map and 2 for list counters.

## Run the generator
Copy `selective_undo.rs` to `crates/core/resource-store/tests/`, register it like the latency test (`name = "selective_undo"`), then:
```bash
cargo test --release -p tilo-resource-store --test selective_undo -- --ignored --nocapture
```
Expected: 2,000 histories, 1,300 independent, 0 violations, with and without either fix. On the code the original work started from (`2fcb9946fe`) the same generator reports 678 violations, see `../historical/`.

The files here change or exercise Tilo code and stay AGPL-3.0-only (see `../NOTICE.md`).
