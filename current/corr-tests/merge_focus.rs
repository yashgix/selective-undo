#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]

//! `Store::undo` beside other people's changes: an invocation undone while a synced peer's change
//! (a key, a text insert or a counter step) sits next to it, and a later change in a single store.

use std::collections::BTreeMap;

use automerge::transaction::{Transactable as _, Transaction};
use automerge::{AutomergeError, ObjId, ObjType, ROOT, ReadDoc as _, ScalarValue, Value};
use tilo_resource_store::{Proposal, Received, Resource, Schema, Stamp, Store, Type, Written};

type Edit = fn(&mut Transaction<'_>, &ObjId) -> Result<(), AutomergeError>;

fn stamp(invocation: &str) -> Stamp {
    Stamp {
        invocation: invocation.to_owned(),
        ..Stamp::default()
    }
}

fn store() -> Store {
    let mut store = Store::new();
    store.declare(Schema {
        types: vec![Type {
            name: "note".to_owned(),
            fields: Vec::new(),
            nested: Vec::new(),
            live: None,
        }],
        ..Schema::default()
    });
    store
}

fn resource() -> Resource {
    Resource {
        readers: "*".to_owned(),
        ..Resource::default()
    }
}

/// Edits the content of the note `id`; the edit must change it.
fn write(store: &mut Store, id: &str, invocation: &str, edit: Edit) -> Written {
    let ((), written) = store
        .edit_content("note", id, &stamp(invocation), edit)
        .unwrap();
    written.unwrap()
}

/// Hands a peer's change to `store` the way the network does.
fn deliver(store: &mut Store, written: &Written) {
    let received = Received {
        change: written.change.clone(),
        author: [0; 32],
        claimed: None,
    };
    let refused = store
        .receive(
            &written.doc,
            &written.readers,
            vec![received],
            &|_: &Proposal<'_>| true,
        )
        .unwrap();
    assert!(refused.is_empty(), "{refused:?}");
}

/// The root of a content document, every value shown as text (a counter as its number).
fn observe(store: &Store, doc: &str) -> BTreeMap<String, String> {
    let doc = store.document(doc).unwrap();
    doc.keys(ROOT)
        .map(|key| {
            let shown = match doc.get(ROOT, key.as_str()).unwrap() {
                Some((Value::Object(ObjType::Text), id)) => doc.text(&id).unwrap(),
                Some((Value::Scalar(scalar), _)) => match scalar.as_ref() {
                    ScalarValue::Counter(counter) => i64::from(counter).to_string(),
                    other => format!("{other:?}"),
                },
                other => format!("{other:?}"),
            };
            (key, shown)
        })
        .collect()
}

fn body(tx: &Transaction<'_>, root: &ObjId) -> Result<ObjId, AutomergeError> {
    tx.get(root, "body")?
        .map(|(_, id)| id)
        .ok_or_else(|| AutomergeError::InvalidObjId("body".to_owned()))
}

/// Two stores share a base. `mine` (i2) is made in A, `theirs` (i3) in B without seeing i2, and A
/// receives i3. A undoes i2; B then receives i2 and A's undo.
struct Merged {
    a: Store,
    b: Store,
    doc: String,
    /// B's content before it saw i2: a replay of the history without i2.
    without_i2: BTreeMap<String, String>,
}

fn merged(base: Edit, mine: Edit, theirs: Edit) -> Merged {
    let mut a = store();
    let made = a.put("note", "n1", resource(), &stamp("i1")).unwrap();
    let base = write(&mut a, "n1", "i1", base);
    let mut b = store();
    for written in made.iter().chain([&base]) {
        deliver(&mut b, written);
    }
    let own = write(&mut a, "n1", "i2", mine);
    let other = write(&mut b, "n1", "i3", theirs);
    let without_i2 = observe(&b, &base.doc);
    deliver(&mut a, &other);
    // B needs i2 to take A's undo, which depends on it.
    deliver(&mut b, &own);
    for undo in a.undo("i2", &stamp("undo-i2")).unwrap() {
        deliver(&mut b, &undo);
    }
    Merged {
        a,
        b,
        doc: base.doc,
        without_i2,
    }
}

/// P1 in A, and A and B converge on it; `key` must hold `want` so the oracle is not vacuous.
fn assert_p1_and_converged(run: &Merged, key: &str, want: &str) {
    assert_eq!(run.without_i2.get(key).map(String::as_str), Some(want));
    assert_eq!(observe(&run.a, &run.doc), run.without_i2);
    assert_eq!(observe(&run.b, &run.doc), observe(&run.a, &run.doc));
}

#[test]
fn undoing_a_key_beside_a_peers_key_restores_it_and_keeps_theirs() {
    let run = merged(
        |tx, root| tx.put(root, "x", "base"),
        |tx, root| tx.put(root, "x", "two"),
        |tx, root| tx.put(root, "y", "three"),
    );

    assert_p1_and_converged(&run, "x", "Str(\"base\")");
}

#[test]
fn undoing_a_text_insert_beside_a_peers_insert_keeps_theirs() {
    let run = merged(
        |tx, root| {
            let text = tx.put_object(root, "body", ObjType::Text)?;
            tx.splice_text(&text, 0, 0, "one two three")
        },
        |tx, root| {
            let text = body(tx, root)?;
            tx.splice_text(&text, 3, 0, " mid")
        },
        |tx, root| {
            let text = body(tx, root)?;
            let end = tx.length(&text);
            tx.splice_text(&text, end, 0, " end")
        },
    );

    assert_p1_and_converged(&run, "body", "one two three end");
}

#[test]
fn undoing_a_counter_step_beside_a_peers_step_keeps_theirs() {
    let run = merged(
        |tx, root| tx.put(root, "count", ScalarValue::counter(0)),
        |tx, root| tx.increment(root, "count", 5),
        |tx, root| tx.increment(root, "count", -2),
    );

    assert_p1_and_converged(&run, "count", "-2");
}

#[test]
fn undoing_a_counter_step_in_one_store_keeps_a_later_step() {
    let mut store = store();
    store.put("note", "n1", resource(), &stamp("i1")).unwrap();
    let base = write(&mut store, "n1", "i1", |tx, root| {
        tx.put(root, "count", ScalarValue::counter(0))
    });
    write(&mut store, "n1", "i2", |tx, root| {
        tx.increment(root, "count", 5)
    });
    write(&mut store, "n1", "i3", |tx, root| {
        tx.increment(root, "count", -2)
    });

    store.undo("i2", &stamp("undo-i2")).unwrap();

    assert_eq!(
        observe(&store, &base.doc).get("count").map(String::as_str),
        Some("-2")
    );
}
