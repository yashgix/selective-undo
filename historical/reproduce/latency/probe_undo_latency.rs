#![cfg(feature = "apps")]
#![allow(clippy::unwrap_used, reason = "scratch measurement")]

//! Scratch measurement, never committed: how long one `Store::undo` takes, today and through the
//! revert hook, on a document of 100, 1k and 10k items written as one change or as many.

use std::time::{Duration, Instant};

use serde_json::json;
use tilo_resource_store::wire::{self, Command, Edit};
use tilo_resource_store::{Resource, Stamp, Store};
use tilo_workspace_service::workspaces::content_undo::RevertContentUndo;

const ROUNDS: usize = 31;

fn stamp(invocation: String) -> Stamp {
    Stamp {
        invocation,
        ..Stamp::default()
    }
}

fn put(key: String) -> Edit {
    Edit::MapPut {
        field: "items".to_owned(),
        key,
        value: json!(true),
    }
}

fn edit(store: &mut Store, invocation: String, edits: Vec<Edit>) {
    let command = Command::Edit {
        kind: "note".to_owned(),
        id: "n".to_owned(),
        edits,
        stamp: stamp(invocation),
    };
    wire::write(store, command).unwrap();
}

fn seeded(hook: bool, items: usize, many_changes: bool) -> Store {
    let mut store = Store::new();
    if hook {
        store.set_content_undo(Box::new(RevertContentUndo));
    }
    let resource = Resource {
        readers: "*".to_owned(),
        ..Resource::default()
    };
    store.put("note", "n", resource, &stamp("made".to_owned())).unwrap();
    if many_changes {
        for i in 0..items {
            edit(&mut store, format!("seed-{i}"), vec![put(format!("k{i}"))]);
        }
    } else {
        let edits = (0..items).map(|i| put(format!("k{i}"))).collect();
        edit(&mut store, "seed".to_owned(), edits);
    }
    store
}

fn undo_once(store: &mut Store, round: usize) -> Duration {
    let target = format!("target-{round}");
    edit(store, target.clone(), vec![put(format!("t{round}"))]);
    edit(store, format!("later-{round}"), vec![put(format!("l{round}"))]);
    let undo = Stamp {
        verb: "undo".to_owned(),
        invocation: format!("undo-{round}"),
        undoes: Some(target.clone()),
        ..Stamp::default()
    };
    let command = Command::Undo {
        invocation: target,
        stamp: undo,
    };
    let start = Instant::now();
    wire::write(store, command).unwrap();
    start.elapsed()
}

fn report(hook: bool, items: usize, many_changes: bool) {
    let setup = Instant::now();
    let mut store = seeded(hook, items, many_changes);
    let setup = setup.elapsed();
    let mut times: Vec<Duration> = (0..ROUNDS).map(|round| undo_once(&mut store, round)).collect();
    times.sort();
    let micros = |at: usize| times[at].as_micros();
    println!(
        "LATENCY items={items} shape={} store={} rounds={ROUNDS} median_us={} p90_us={} max_us={} setup_ms={}",
        if many_changes { "many_changes" } else { "one_change" },
        if hook { "hook" } else { "today" },
        micros(ROUNDS / 2),
        micros(ROUNDS * 9 / 10),
        micros(ROUNDS - 1),
        setup.as_millis(),
    );
}

#[test]
fn undo_latency() {
    for items in [100, 1_000, 10_000] {
        for many_changes in [false, true] {
            for hook in [false, true] {
                report(hook, items, many_changes);
            }
        }
    }
}
