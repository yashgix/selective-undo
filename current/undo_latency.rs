#![allow(clippy::unwrap_used, reason = "measurement")]

use std::time::{Duration, Instant};

use serde_json::json;
use tilo_resource_store::wire::{self, Command, Edit};
use tilo_resource_store::{Resource, Stamp, Store};

const ROUNDS: usize = 31;

fn stamp(invocation: String) -> Stamp {
    Stamp { invocation, ..Stamp::default() }
}

fn put(key: String) -> Edit {
    Edit::MapPut { field: "items".to_owned(), key, value: json!(true) }
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

fn seeded(items: usize, many_changes: bool) -> Store {
    let mut store = Store::new();
    let resource = Resource { readers: "*".to_owned(), ..Resource::default() };
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
    let start = Instant::now();
    wire::write(store, Command::Undo { invocation: target, stamp: undo }).unwrap();
    start.elapsed()
}

#[test]
fn undo_latency() {
    for items in [100, 1_000, 10_000] {
        for many_changes in [false, true] {
            let mut store = seeded(items, many_changes);
            let mut times: Vec<Duration> =
                (0..ROUNDS).map(|round| undo_once(&mut store, round)).collect();
            times.sort();
            println!(
                "items={items} many_changes={many_changes} median_us={}",
                times[ROUNDS / 2].as_micros()
            );
        }
    }
}
