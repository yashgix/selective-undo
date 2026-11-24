#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test assertions"
)]

//! Revert scenarios on plain Automerge documents: counters (including increments by another
//! author, concurrent or later), list counters, shared maps and blocks, concurrent writes, and
//! replaced maps. Each states what an undo must take back and what it must leave to others.

use automerge::transaction::Transactable as _;
use automerge::{
    ActorId, AutoCommit, AutomergeError, ChangeHash, ObjId, ObjType, ROOT, ReadDoc as _,
    ScalarValue, Value,
};
use serde_json::Value as JsonValue;
use tilo_automerge::ApplyIncoming as _;
use tilo_resource_store::history::authors::{HistoryAuthor, ResolveAuthor};
use tilo_resource_store::history::revert::revert_changes;

/// Names every actor by its hex.
struct ActorNamed;

impl ResolveAuthor for ActorNamed {
    fn resolve(&self, actor: &str) -> HistoryAuthor {
        HistoryAuthor::new(actor, 0.0, actor)
    }
}

fn render(doc: &AutoCommit, obj: &ObjId) -> JsonValue {
    match doc.object_type(obj) {
        Ok(ObjType::Map | ObjType::Table) => JsonValue::Object(
            doc.keys(obj)
                .map(|key| {
                    let found = doc.get(obj, key.as_str());
                    (key, render_entry(doc, found))
                })
                .collect(),
        ),
        Ok(ObjType::List) => JsonValue::Array(
            (0..doc.length(obj))
                .map(|index| render_entry(doc, doc.get(obj, index)))
                .collect(),
        ),
        Ok(ObjType::Text) => JsonValue::String(doc.text(obj).unwrap_or_default()),
        Err(_) => JsonValue::Null,
    }
}

fn render_entry(
    doc: &AutoCommit,
    found: Result<Option<(Value<'_>, ObjId)>, AutomergeError>,
) -> JsonValue {
    match found {
        Ok(Some((Value::Object(_), id))) => render(doc, &id),
        Ok(Some((Value::Scalar(scalar), _))) => match scalar.as_ref() {
            ScalarValue::Str(text) => JsonValue::String(text.to_string()),
            _ => JsonValue::Null,
        },
        _ => JsonValue::Null,
    }
}

/// Commits what `write` does under the actor named `name`, with the actor's hex.
fn commit_as(
    doc: &mut AutoCommit,
    name: &str,
    write: impl FnOnce(&mut AutoCommit),
) -> (ChangeHash, String) {
    let actor = ActorId::from(name.as_bytes());
    let hex = actor.to_hex_string();
    doc.set_actor(actor);
    write(doc);
    (doc.commit().expect("a change"), hex)
}

fn count_of(doc: &AutoCommit) -> i64 {
    match doc.get(ROOT, "count").unwrap().unwrap().0 {
        Value::Scalar(scalar) => match scalar.as_ref() {
            ScalarValue::Counter(counter) => i64::from(counter),
            other => panic!("not a counter: {other:?}"),
        },
        Value::Object(_) => panic!("not a counter"),
    }
}

fn counter_doc() -> AutoCommit {
    let mut doc = AutoCommit::new();
    commit_as(&mut doc, "base", |doc| {
        doc.put(ROOT, "count", ScalarValue::counter(0)).unwrap();
    });
    doc
}

fn increment_by(by: i64) -> impl FnOnce(&mut AutoCommit) {
    move |doc| doc.increment(ROOT, "count", by).unwrap()
}

/// Reverts the run as `revert_target` does and returns what it kept and whether it wrote.
fn revert_run_wrote(doc: &mut AutoCommit, run: (ChangeHash, String)) -> (usize, bool) {
    doc.set_actor(ActorId::from(b"undo".as_slice()));
    let outcomes = revert_changes(doc, &[run], &ActorNamed).unwrap();
    doc.commit();
    let kept = outcomes.iter().map(|outcome| outcome.kept.len()).sum();
    (kept, outcomes.iter().any(|outcome| outcome.wrote))
}

/// Reverts the run as `revert_target` does and returns what it kept.
fn revert_run(doc: &mut AutoCommit, run: (ChangeHash, String)) -> usize {
    revert_run_wrote(doc, run).0
}

#[test]
fn an_increment_is_taken_back_beside_another_authors_increment() {
    let mut doc = counter_doc();
    let run = commit_as(&mut doc, "a", increment_by(5));
    commit_as(&mut doc, "b", increment_by(-2));
    assert_eq!(revert_run(&mut doc, run), 0);
    assert_eq!(count_of(&doc), -2);
}

#[test]
fn a_concurrent_increment_by_another_author_is_kept() {
    let mut doc = counter_doc();
    let mut peer = doc.fork();
    let run = commit_as(&mut doc, "a", increment_by(5));
    commit_as(&mut peer, "b", increment_by(-2));
    doc.merge_incoming(&mut peer).unwrap();
    assert_eq!(revert_run(&mut doc, run), 0);
    assert_eq!(count_of(&doc), -2);
}

#[test]
fn a_counter_made_by_the_run_stays_when_another_author_incremented_it() {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "a", |doc| {
        doc.put(ROOT, "count", ScalarValue::counter(0)).unwrap();
        doc.increment(ROOT, "count", 5).unwrap();
    });
    commit_as(&mut doc, "b", increment_by(-2));
    assert_eq!(revert_run(&mut doc, run), 1);
    assert_eq!(count_of(&doc), 3);
}

/// A list holding one counter at index 0, starting at zero.
fn list_counter_doc() -> (AutoCommit, ObjId) {
    let mut doc = AutoCommit::new().with_actor(ActorId::from(b"base".as_slice()));
    let list = doc.put_object(ROOT, "list", ObjType::List).unwrap();
    doc.insert(&list, 0, ScalarValue::counter(0)).unwrap();
    doc.commit();
    (doc, list)
}

fn list_counter_at(doc: &AutoCommit, list: &ObjId, index: usize) -> i64 {
    match doc.get(list, index).unwrap().unwrap().0 {
        Value::Scalar(scalar) => match scalar.as_ref() {
            ScalarValue::Counter(counter) => i64::from(counter),
            other => panic!("not a counter: {other:?}"),
        },
        Value::Object(_) => panic!("not a counter"),
    }
}

fn list_str_at(doc: &AutoCommit, list: &ObjId, index: usize) -> Option<String> {
    let (value, _) = doc.get(list, index).unwrap()?;
    value.as_str().map(str::to_owned)
}

#[test]
fn a_list_counter_is_taken_back_beside_another_authors_increment() {
    let (mut doc, list) = list_counter_doc();
    let run = commit_as(&mut doc, "a", |doc| doc.increment(&list, 0, 5).unwrap());
    commit_as(&mut doc, "b", |doc| doc.increment(&list, 0, -2).unwrap());
    assert_eq!(revert_run(&mut doc, run), 0);
    assert_eq!(list_counter_at(&doc, &list, 0), -2);
}

#[test]
fn a_list_counter_is_taken_back_after_an_insert_before_it() {
    let (mut doc, list) = list_counter_doc();
    let run = commit_as(&mut doc, "a", |doc| doc.increment(&list, 0, 5).unwrap());
    commit_as(&mut doc, "b", |doc| doc.insert(&list, 0, "theirs").unwrap());
    assert_eq!(revert_run(&mut doc, run), 0);
    assert_eq!(list_str_at(&doc, &list, 0).as_deref(), Some("theirs"));
    assert_eq!(list_counter_at(&doc, &list, 1), 0);
}

#[test]
fn a_list_counter_another_author_replaced_is_kept() {
    let (mut doc, list) = list_counter_doc();
    let run = commit_as(&mut doc, "a", |doc| doc.increment(&list, 0, 5).unwrap());
    commit_as(&mut doc, "b", |doc| doc.put(&list, 0, "theirs").unwrap());
    assert_eq!(revert_run_wrote(&mut doc, run).0, 1);
    assert_eq!(list_str_at(&doc, &list, 0).as_deref(), Some("theirs"));
}

#[test]
fn a_list_counter_another_author_deleted_is_kept_and_nothing_is_written() {
    let (mut doc, list) = list_counter_doc();
    let run = commit_as(&mut doc, "a", |doc| doc.increment(&list, 0, 5).unwrap());
    commit_as(&mut doc, "b", |doc| doc.delete(&list, 0).unwrap());
    assert_eq!(revert_run_wrote(&mut doc, run), (1, false));
    assert_eq!(doc.length(&list), 0);
}

fn put_strs(pairs: &'static [(&'static str, &'static str)]) -> impl FnOnce(&mut AutoCommit) {
    move |doc| {
        for (key, value) in pairs {
            doc.put(ROOT, *key, *value).unwrap();
        }
    }
}

/// The target writes `k0` and `k1`, a later invocation writes `k2`; the target is reverted.
fn revert_target_siblings(target_actor: &str, later_actor: &str) -> JsonValue {
    let mut doc = AutoCommit::new();
    let run = commit_as(
        &mut doc,
        target_actor,
        put_strs(&[("k0", "v0"), ("k1", "v1")]),
    );
    commit_as(&mut doc, later_actor, put_strs(&[("k2", "v2")]));
    assert_eq!(revert_run(&mut doc, run), 0);
    render(&doc, &ROOT)
}

#[test]
fn revert_keeps_later_sibling_edit_by_other_actor() {
    let after = revert_target_siblings("target", "later");

    assert_eq!(after, serde_json::json!({"k2": "v2"}));
}

#[test]
fn revert_keeps_later_sibling_edit_same_actor() {
    let after = revert_target_siblings("writer", "writer");

    assert_eq!(after, serde_json::json!({"k2": "v2"}));
}

/// `first` creates the map `m` with the key `a`, `second` then writes `second_key` into it; `first` is reverted.
fn revert_creator_of_shared_map(second_key: &str) -> JsonValue {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&map, "a", "first").unwrap();
    });
    let key = second_key.to_owned();
    commit_as(&mut doc, "second", move |doc| {
        let (_, map) = doc.get(ROOT, "m").unwrap().unwrap();
        doc.put(&map, key, "second").unwrap();
    });
    revert_run(&mut doc, run);
    render(&doc, &ROOT)
}

#[test]
fn a_map_the_run_made_stays_whole_when_another_author_wrote_in_it() {
    let after = revert_creator_of_shared_map("b");

    assert_eq!(
        after,
        serde_json::json!({"m": {"a": "first", "b": "second"}})
    );
}

#[test]
fn a_key_another_author_set_again_in_a_shared_map_is_kept() {
    let after = revert_creator_of_shared_map("a");

    assert_eq!(after, serde_json::json!({"m": {"a": "second"}}));
}

#[test]
fn a_key_the_run_wrote_twice_stays_in_a_map_another_author_wrote_in() {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&map, "a", "one").unwrap();
        doc.put(&map, "a", "two").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let (_, map) = doc.get(ROOT, "m").unwrap().unwrap();
        doc.put(&map, "b", "second").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"m": {"a": "two", "b": "second"}})
    );
}

#[test]
fn an_object_another_author_put_over_the_runs_key_is_kept() {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&map, "a", "one").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let (_, map) = doc.get(ROOT, "m").unwrap().unwrap();
        let inner = doc.put_object(&map, "a", ObjType::Map).unwrap();
        doc.put(&inner, "x", "kept").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"m": {"a": {"x": "kept"}}})
    );
}

fn shared(doc: &AutoCommit, key: &str) -> ObjId {
    doc.get(ROOT, key).unwrap().unwrap().1
}

#[test]
fn an_object_the_run_put_over_its_own_key_keeps_another_authors_edit_inside_it() {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&map, "a", "one").unwrap();
        doc.put_object(&map, "a", ObjType::Map).unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let map = shared(doc, "m");
        let inner = shared_in(doc, &map, "a");
        doc.put(&inner, "x", "kept").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"m": {"a": {"x": "kept"}}})
    );
}

fn shared_in(doc: &AutoCommit, map: &ObjId, key: &str) -> ObjId {
    doc.get(map, key).unwrap().unwrap().1
}

#[test]
fn a_map_in_a_list_stays_whole_when_another_author_wrote_in_it() {
    let mut doc = AutoCommit::new();
    commit_as(&mut doc, "base", |doc| {
        doc.put_object(ROOT, "l", ObjType::List).unwrap();
    });
    let run = commit_as(&mut doc, "first", |doc| {
        let list = shared(doc, "l");
        let map = doc.insert_object(&list, 0, ObjType::Map).unwrap();
        doc.put(&map, "a", "one").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let list = shared(doc, "l");
        let map = shared_at(doc, &list, 0);
        doc.put(&map, "b", "second").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"l": [{"a": "one", "b": "second"}]})
    );
}

fn shared_at(doc: &AutoCommit, list: &ObjId, index: usize) -> ObjId {
    doc.get(list, index).unwrap().unwrap().1
}

fn base_with_map() -> AutoCommit {
    let mut doc = AutoCommit::new();
    commit_as(&mut doc, "base", |doc| {
        doc.put_object(ROOT, "m", ObjType::Map).unwrap();
    });
    doc
}

/// Two authors write the shared map at the same time and merge in either order; `first` is reverted.
fn concurrent_in_shared_map(
    first_key: &str,
    second_key: &str,
    first_merges_second: bool,
) -> AutoCommit {
    let mut left = base_with_map();
    let mut right = left.fork();
    let (first_key, second_key) = (first_key.to_owned(), second_key.to_owned());
    let run = commit_as(&mut left, "first", move |doc| {
        let map = shared(doc, "m");
        doc.put(&map, first_key, "first").unwrap();
    });
    commit_as(&mut right, "second", move |doc| {
        let map = shared(doc, "m");
        doc.put(&map, second_key, "second").unwrap();
    });
    if first_merges_second {
        left.merge_incoming(&mut right).unwrap();
        revert_run(&mut left, run);
        left
    } else {
        right.merge_incoming(&mut left).unwrap();
        revert_run(&mut right, run);
        right
    }
}

#[test]
fn concurrent_sibling_keys_in_a_shared_map_lose_only_the_reverted_one_in_either_merge_order() {
    for first_merges_second in [true, false] {
        let doc = concurrent_in_shared_map("a", "b", first_merges_second);

        assert_eq!(
            render(&doc, &ROOT),
            serde_json::json!({"m": {"b": "second"}})
        );
    }
}

#[test]
fn a_concurrent_write_to_the_same_key_is_never_deleted_in_either_merge_order() {
    for first_merges_second in [true, false] {
        let doc = concurrent_in_shared_map("a", "a", first_merges_second);
        let map = shared(&doc, "m");

        assert!(doc.get(&map, "a").unwrap().is_some());
    }
}

#[test]
fn a_map_the_run_put_over_an_older_one_keeps_its_replacement_free_of_the_older_keys() {
    let mut doc = AutoCommit::new();
    commit_as(&mut doc, "base", |doc| {
        let old = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&old, "k", "old").unwrap();
    });
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put(&map, "a", "one").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let map = shared(doc, "m");
        doc.put(&map, "b", "second").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"m": {"a": "one", "b": "second"}})
    );
}

/// `first` creates `m` with a nested map `n`, `second` writes `second_key` into `m`, `third` optionally writes inside `n`.
fn revert_creator_of_nested_shared_map(third_writes_inside_n: bool) -> JsonValue {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        let nested = doc.put_object(&map, "n", ObjType::Map).unwrap();
        doc.put(&nested, "x", "one").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let map = shared(doc, "m");
        doc.put(&map, "b", "second").unwrap();
    });
    if third_writes_inside_n {
        commit_as(&mut doc, "third", |doc| {
            let map = shared(doc, "m");
            let nested = shared_in(doc, &map, "n");
            doc.put(&nested, "y", "third").unwrap();
        });
    }
    revert_run(&mut doc, run);
    render(&doc, &ROOT)
}

#[test]
fn a_nested_map_the_run_made_stays_with_its_parent_when_another_author_wrote_in_the_parent() {
    let after = revert_creator_of_nested_shared_map(false);

    assert_eq!(
        after,
        serde_json::json!({"m": {"b": "second", "n": {"x": "one"}}})
    );
}

#[test]
fn a_nested_map_the_run_made_stays_when_another_author_wrote_inside_it() {
    let after = revert_creator_of_nested_shared_map(true);

    assert_eq!(
        after,
        serde_json::json!({"m": {"b": "second", "n": {"x": "one", "y": "third"}}})
    );
}

#[test]
fn an_object_the_run_replaced_in_its_own_change_keeps_another_authors_edit_inside_the_new_one() {
    let mut doc = AutoCommit::new();
    let run = commit_as(&mut doc, "first", |doc| {
        let map = doc.put_object(ROOT, "m", ObjType::Map).unwrap();
        doc.put_object(&map, "n", ObjType::Map).unwrap();
        let second = doc.put_object(&map, "n", ObjType::Map).unwrap();
        doc.put(&second, "x", "one").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let map = shared(doc, "m");
        doc.put(&map, "b", "second").unwrap();
        let nested = shared_in(doc, &map, "n");
        doc.put(&nested, "y", "third").unwrap();
    });
    revert_run(&mut doc, run);

    assert_eq!(
        render(&doc, &ROOT),
        serde_json::json!({"m": {"b": "second", "n": {"x": "one", "y": "third"}}})
    );
}

/// A block (a map holding a type and a text) in a list, made by `first`; `second` changes it as `edit` says.
fn revert_creator_of_block(edit: fn(&mut AutoCommit, &ObjId)) -> JsonValue {
    let mut doc = AutoCommit::new();
    commit_as(&mut doc, "base", |doc| {
        doc.put_object(ROOT, "blocks", ObjType::List).unwrap();
    });
    let run = commit_as(&mut doc, "first", |doc| {
        let list = shared(doc, "blocks");
        let block = doc.insert_object(&list, 0, ObjType::Map).unwrap();
        doc.put(&block, "type", "paragraph").unwrap();
        let text = doc.put_object(&block, "text", ObjType::Text).unwrap();
        doc.splice_text(&text, 0, 0, "Figure 1").unwrap();
    });
    commit_as(&mut doc, "second", |doc| {
        let list = shared(doc, "blocks");
        edit(doc, &shared_at(doc, &list, 0));
    });
    revert_run(&mut doc, run);
    render(&doc, &ROOT)
}

#[test]
fn a_block_another_author_edited_the_text_of_stays_whole() {
    let after = revert_creator_of_block(|doc, block| {
        let text = shared_in(doc, block, "text");
        doc.splice_text(&text, 0, 0, "Revised. ").unwrap();
    });

    assert_eq!(
        after,
        serde_json::json!({"blocks": [{"type": "paragraph", "text": "Revised. Figure 1"}]})
    );
}

#[test]
fn a_block_another_author_added_an_attribute_to_stays_whole() {
    let after = revert_creator_of_block(|doc, block| {
        doc.put(block, "align", "center").unwrap();
    });

    assert_eq!(
        after,
        serde_json::json!({"blocks": [{"type": "paragraph", "text": "Figure 1", "align": "center"}]})
    );
}
