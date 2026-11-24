# Notice

The write-up, the page, `historical/reproduce/run.sh` and the data are original work under the MIT License in `LICENSE`.

The files in `historical/patch/` and `current/` change code from [Tilo](https://code.tilo.so/tilo/tilo) (`tilo-resource-store`, `collaboration` and `tilo-workspace-service`), which is licensed AGPL-3.0-only. That code and the patches to it remain under that license, reproduced in `historical/patch/LICENSE-AGPL-3.0-only`. Tilo's own `LICENSING.md` lists the license of each part.

This is proposed work. It is not part of Tilo, and no change here has been accepted upstream.

The test files that link Tilo crates (`current/undo_latency.rs`, `current/selective_undo.rs`, `historical/reproduce/latency/probe_undo_latency.rs`) are written to be contributed to that code base and stay AGPL-3.0-only for the same reason.
