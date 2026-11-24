# Historical: the original work (20 to 23 Nov)

The investigation and implementation I did before upstream landed its own change on 24 Nov. It is kept in full and was never filed. It applies to Tilo `2fcb9946fe`, not to today's `main`.

- `patch/pr1-generated-histories.diff`: the seeded generator of edit histories and the benchmark (test code).
- `patch/pr2-selective-revert.diff`: fixes in the revert (increments, replaced maps, blocks built on by others, the quadratic lookup).
- `patch/pr3-content-undo-hook.diff`: the `ContentUndo` hook in the resource store, the workspace-service wiring, the store rule for flat map fields and the Chat reaction tests. It needs the first two.
- `reproduce/run.sh`: the correctness benchmark on that code (678 of 1,300 histories break before, 1 after). It does not reproduce the latency numbers.
- `reproduce/latency/probe_undo_latency.rs`: the latency probe those numbers came from, for reference.

Apply the three patches in order with `git apply` to a checkout of `2fcb9946fe`; they give the final tree of the original work.
