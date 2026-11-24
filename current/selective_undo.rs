#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test assertions and a small generator"
)]

//! A seeded generator and checker for `Store::undo`: one invocation is undone, and the
//! checker compares the result with a replay of the history without that invocation.
//!
//! The generator builds a tiny content model first, then replays the generated operations
//! against the real store, so histories are data and can be replayed independently.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;

use automerge::transaction::Transactable as _;
use automerge::{ObjId, ObjType, Prop};
use serde_json::Value as JsonValue;
use tilo_resource_store::{Cell, Resource, Stamp, Store};

pub(crate) const KIND: &str = "Issue";
pub(crate) const ID: &str = "a";
pub(crate) const MEMBERS: &str = "*";
pub(crate) const ROOT_LABEL: &str = "i1:root";
pub(crate) const KEY_POOL: [&str; 6] = ["k0", "k1", "k2", "k3", "k4", "k5"];

pub(crate) fn stamped(invocation: &str) -> Stamp {
    Stamp {
        verb: "make".to_owned(),
        invoker: "ada".to_owned(),
        invocation: invocation.to_owned(),
        membership: vec!["m1".to_owned()],
        ..Stamp::default()
    }
}

pub(crate) fn undo_stamp(target: &str) -> Stamp {
    Stamp {
        verb: "undo".to_owned(),
        invoker: "ada".to_owned(),
        invocation: format!("u-{target}"),
        membership: vec!["m1".to_owned()],
        undoes: Some(target.to_owned()),
        ..Stamp::default()
    }
}

pub(crate) fn resource() -> Resource {
    Resource {
        readers: MEMBERS.to_owned(),
        parent: Some("board".to_owned()),
        order: Some("a0".to_owned()),
        fields: BTreeMap::from([("title".to_owned(), Cell::One("Draft".into()))]),
    }
}

pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % n as u64).unwrap()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct History {
    pub(crate) invocations: Vec<Invocation>,
}

#[derive(Clone, Debug)]
pub(crate) struct Invocation {
    pub(crate) id: String,
    pub(crate) ops: Vec<Op>,
}

#[derive(Clone, Debug)]
pub(crate) enum Op {
    PutKey {
        target: String,
        key: String,
        value: String,
    },
    DeleteKey {
        target: String,
        key: String,
    },
    Insert {
        target: String,
        after: Option<String>,
        value: String,
    },
    Delete {
        target: String,
        value: String,
    },
    CreateMap {
        parent: String,
        key: String,
        name: String,
    },
    CreateList {
        parent: String,
        key: String,
        name: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(crate) history: History,
    pub(crate) target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelKind {
    Map,
    List,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Scalar(String),
    Container(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub(crate) containers: BTreeMap<String, ModelKind>,
    pub(crate) maps: BTreeMap<String, BTreeMap<String, Entry>>,
    pub(crate) lists: BTreeMap<String, Vec<String>>,
    pub(crate) paths: BTreeMap<String, Vec<String>>,
}

pub(crate) fn base_model() -> Model {
    let mut model = Model::default();

    model
        .containers
        .insert(ROOT_LABEL.to_owned(), ModelKind::Map);
    model
        .containers
        .insert("i1:meta".to_owned(), ModelKind::Map);
    model
        .containers
        .insert("i1:props".to_owned(), ModelKind::Map);
    model
        .containers
        .insert("i1:items".to_owned(), ModelKind::List);

    model.maps.insert(
        ROOT_LABEL.to_owned(),
        BTreeMap::from([("meta".to_owned(), Entry::Container("i1:meta".to_owned()))]),
    );
    model.maps.insert(
        "i1:meta".to_owned(),
        BTreeMap::from([
            ("props".to_owned(), Entry::Container("i1:props".to_owned())),
            ("items".to_owned(), Entry::Container("i1:items".to_owned())),
        ]),
    );
    model.maps.insert("i1:props".to_owned(), BTreeMap::new());
    model.lists.insert("i1:items".to_owned(), Vec::new());

    model.paths.insert(ROOT_LABEL.to_owned(), Vec::new());
    model
        .paths
        .insert("i1:meta".to_owned(), vec!["meta".to_owned()]);
    model.paths.insert(
        "i1:props".to_owned(),
        vec!["meta".to_owned(), "props".to_owned()],
    );
    model.paths.insert(
        "i1:items".to_owned(),
        vec!["meta".to_owned(), "items".to_owned()],
    );

    model
}

#[derive(Clone, Debug)]
pub(crate) enum Choice {
    PutKey {
        target: String,
        key: String,
    },
    DeleteKey {
        target: String,
        key: String,
    },
    Insert {
        target: String,
        after: Option<String>,
    },
    Delete {
        target: String,
        value: String,
    },
    CreateMap {
        parent: String,
        key: String,
    },
    CreateList {
        parent: String,
        key: String,
    },
}

pub(crate) fn generate_case(seed: u64) -> Case {
    let mut rng = Rng::new(seed);
    let mut model = base_model();
    let mut values = 1_u64;
    let mut containers = 0_u64;
    let more = 3 + rng.below(6);

    let mut invocations = vec![Invocation {
        id: "i1".to_owned(),
        ops: Vec::new(),
    }];

    for index in 0..more {
        let id = format!("i{}", index + 2);
        let count = 1 + rng.below(3);
        let mut ops = Vec::new();
        for _ in 0..count {
            let choice = random_choice(&mut rng, &model);
            let op = make_op(choice, &id, &mut values, &mut containers);
            apply_model(&mut model, &op);
            ops.push(op);
        }
        invocations.push(Invocation { id, ops });
    }

    let target_index = 1 + rng.below(invocations.len() - 1);
    let target = invocations[target_index].id.clone();

    Case {
        history: History { invocations },
        target,
    }
}

pub(crate) fn random_choice(rng: &mut Rng, model: &Model) -> Choice {
    let mut choices = Vec::new();

    for (label, entries) in &model.maps {
        for key in KEY_POOL {
            if !entries.contains_key(key) {
                choices.push(Choice::PutKey {
                    target: label.clone(),
                    key: key.to_owned(),
                });
                choices.push(Choice::CreateMap {
                    parent: label.clone(),
                    key: key.to_owned(),
                });
                choices.push(Choice::CreateList {
                    parent: label.clone(),
                    key: key.to_owned(),
                });
            }
        }
        for (key, entry) in entries {
            if matches!(entry, Entry::Scalar(_)) {
                choices.push(Choice::DeleteKey {
                    target: label.clone(),
                    key: key.clone(),
                });
            }
        }
    }

    for (label, items) in &model.lists {
        choices.push(Choice::Insert {
            target: label.clone(),
            after: None,
        });
        for item in items {
            choices.push(Choice::Insert {
                target: label.clone(),
                after: Some(item.clone()),
            });
            choices.push(Choice::Delete {
                target: label.clone(),
                value: item.clone(),
            });
        }
    }

    let index = rng.below(choices.len());
    choices.into_iter().nth(index).expect("a choice exists")
}

pub(crate) fn make_op(
    choice: Choice,
    invocation: &str,
    values: &mut u64,
    containers: &mut u64,
) -> Op {
    match choice {
        Choice::PutKey { target, key } => {
            let value = format!("v{values}");
            *values += 1;
            Op::PutKey { target, key, value }
        }
        Choice::DeleteKey { target, key } => Op::DeleteKey { target, key },
        Choice::Insert { target, after } => {
            let value = format!("v{values}");
            *values += 1;
            Op::Insert {
                target,
                after,
                value,
            }
        }
        Choice::Delete { target, value } => Op::Delete { target, value },
        Choice::CreateMap { parent, key } => {
            let name = format!("{invocation}:c{containers}");
            *containers += 1;
            Op::CreateMap { parent, key, name }
        }
        Choice::CreateList { parent, key } => {
            let name = format!("{invocation}:c{containers}");
            *containers += 1;
            Op::CreateList { parent, key, name }
        }
    }
}

pub(crate) fn apply_model(model: &mut Model, op: &Op) {
    match op {
        Op::PutKey { target, key, value } => {
            model
                .maps
                .get_mut(target)
                .expect("target map exists")
                .insert(key.clone(), Entry::Scalar(value.clone()));
        }
        Op::DeleteKey { target, key } => {
            model
                .maps
                .get_mut(target)
                .expect("target map exists")
                .remove(key);
        }
        Op::Insert {
            target,
            after,
            value,
        } => {
            let list = model.lists.get_mut(target).expect("target list exists");
            let index = match after {
                Some(after_value) => {
                    list.iter()
                        .position(|item| item == after_value)
                        .expect("insert-after value exists")
                        + 1
                }
                None => 0,
            };
            list.insert(index, value.clone());
        }
        Op::Delete { target, value } => {
            let list = model.lists.get_mut(target).expect("target list exists");
            let index = list
                .iter()
                .position(|item| item == value)
                .expect("deleted value exists");
            list.remove(index);
        }
        Op::CreateMap { parent, key, name } => {
            model.containers.insert(name.clone(), ModelKind::Map);
            model.maps.insert(name.clone(), BTreeMap::new());
            let mut path = model.paths[parent].clone();
            path.push(key.clone());
            model.paths.insert(name.clone(), path);
            model
                .maps
                .get_mut(parent)
                .expect("parent map exists")
                .insert(key.clone(), Entry::Container(name.clone()));
        }
        Op::CreateList { parent, key, name } => {
            model.containers.insert(name.clone(), ModelKind::List);
            model.lists.insert(name.clone(), Vec::new());
            let mut path = model.paths[parent].clone();
            path.push(key.clone());
            model.paths.insert(name.clone(), path);
            model
                .maps
                .get_mut(parent)
                .expect("parent map exists")
                .insert(key.clone(), Entry::Container(name.clone()));
        }
    }
}

pub(crate) fn is_op_valid(model: &Model, op: &Op) -> bool {
    match op {
        Op::PutKey { target, key, .. } => model
            .maps
            .get(target)
            .is_some_and(|map| !map.contains_key(key)),
        Op::DeleteKey { target, key } => model
            .maps
            .get(target)
            .is_some_and(|map| matches!(map.get(key), Some(Entry::Scalar(_)))),
        Op::Insert { target, after, .. } => {
            model.lists.get(target).is_some_and(|list| match after {
                Some(after_value) => list.iter().any(|item| item == after_value),
                None => true,
            })
        }
        Op::Delete { target, value } => model
            .lists
            .get(target)
            .is_some_and(|list| list.iter().any(|item| item == value)),
        Op::CreateMap { parent, key, .. } | Op::CreateList { parent, key, .. } => model
            .maps
            .get(parent)
            .is_some_and(|map| !map.contains_key(key)),
    }
}

pub(crate) fn valid_history(history: &History) -> bool {
    let mut model = base_model();
    for invocation in &history.invocations {
        if invocation.id == "i1" {
            continue;
        }
        for op in &invocation.ops {
            if !is_op_valid(&model, op) {
                return false;
            }
            apply_model(&mut model, op);
        }
    }
    true
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SlotKey {
    Map { key: String },
    List { value: String },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Slot {
    pub(crate) container: String,
    pub(crate) key: SlotKey,
}

pub(crate) fn slots_of_op(op: &Op) -> BTreeSet<Slot> {
    let mut slots = BTreeSet::new();
    match op {
        Op::PutKey { target, key, .. } | Op::DeleteKey { target, key } => {
            slots.insert(Slot {
                container: target.clone(),
                key: SlotKey::Map { key: key.clone() },
            });
        }
        Op::Insert { target, value, .. } | Op::Delete { target, value } => {
            slots.insert(Slot {
                container: target.clone(),
                key: SlotKey::List {
                    value: value.clone(),
                },
            });
        }
        Op::CreateMap { parent, key, .. } | Op::CreateList { parent, key, .. } => {
            slots.insert(Slot {
                container: parent.clone(),
                key: SlotKey::Map { key: key.clone() },
            });
        }
    }
    slots
}

pub(crate) fn touched_containers(op: &Op) -> Vec<&str> {
    match op {
        Op::PutKey { target, .. }
        | Op::DeleteKey { target, .. }
        | Op::Insert { target, .. }
        | Op::Delete { target, .. } => vec![target.as_str()],
        Op::CreateMap { parent, .. } | Op::CreateList { parent, .. } => vec![parent.as_str()],
    }
}

pub(crate) struct Analysis {
    pub(crate) independent: bool,
}

pub(crate) fn analyse(history: &History, target: &str) -> Analysis {
    let target_index = history
        .invocations
        .iter()
        .position(|invocation| invocation.id == target)
        .expect("target exists");

    let mut model = base_model();
    let mut a_wrote = BTreeSet::new();
    let mut a_created = BTreeSet::new();
    let mut later_wrote = BTreeSet::new();
    let mut later_touched_created = false;

    for (index, invocation) in history.invocations.iter().enumerate() {
        if invocation.id == "i1" {
            continue;
        }

        for op in &invocation.ops {
            let slots = slots_of_op(op);

            if index == target_index {
                a_wrote.extend(slots);
                if let Op::CreateMap { name, .. } | Op::CreateList { name, .. } = op {
                    a_created.insert(name.clone());
                }
            } else if index > target_index {
                later_wrote.extend(slots);
                if !later_touched_created
                    && touched_containers(op)
                        .into_iter()
                        .any(|container| a_created.iter().any(|made| made.as_str() == container))
                {
                    later_touched_created = true;
                }
            }

            apply_model(&mut model, op);
        }
    }

    let independent = !later_touched_created && a_wrote.intersection(&later_wrote).next().is_none();

    Analysis { independent }
}

/// How `play_history_with` commits an invocation's ops to the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Commits {
    /// One change per invocation.
    One,
    /// One change per op, all stamped as the same invocation.
    #[allow(dead_code, reason = "only the store gate replays per op")]
    PerOp,
}

pub(crate) fn play_history(history: &History, skip: Option<&str>) -> Result<Store, String> {
    play_history_with(history, skip, Commits::One)
}

pub(crate) fn play_history_with(
    history: &History,
    skip: Option<&str>,
    commits: Commits,
) -> Result<Store, String> {
    let mut store = Store::new();
    let mut containers = HashMap::new();
    let mut list_values = HashMap::new();
    let mut full_model = base_model();
    let base_stamp = stamped("i1");

    store
        .put(KIND, ID, resource(), &base_stamp)
        .map_err(|error| error.to_string())?;
    store
        .edit_content(KIND, ID, &base_stamp, |tx, root| {
            containers.insert(ROOT_LABEL.to_owned(), root.clone());
            let meta = tx.put_object(root, "meta", ObjType::Map)?;
            containers.insert("i1:meta".to_owned(), meta.clone());
            let props = tx.put_object(&meta, "props", ObjType::Map)?;
            containers.insert("i1:props".to_owned(), props);
            let items = tx.put_object(&meta, "items", ObjType::List)?;
            containers.insert("i1:items".to_owned(), items);
            Ok(())
        })
        .map_err(|error| error.to_string())?;

    for invocation in &history.invocations {
        if invocation.id == "i1" {
            continue;
        }
        if Some(invocation.id.as_str()) == skip {
            for op in &invocation.ops {
                apply_model(&mut full_model, op);
            }
            continue;
        }
        let stamp = stamped(&invocation.id);
        let commit_ops: Vec<&[Op]> = match commits {
            Commits::One => vec![invocation.ops.as_slice()],
            Commits::PerOp => invocation.ops.chunks(1).collect(),
        };
        for ops in commit_ops {
            store
                .edit_content(KIND, ID, &stamp, |tx, root| {
                    containers.insert(ROOT_LABEL.to_owned(), root.clone());
                    for op in ops {
                        apply_op(tx, &mut containers, &mut list_values, &full_model.lists, op)?;
                        apply_model(&mut full_model, op);
                    }
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(store)
}

pub(crate) fn apply_op(
    tx: &mut automerge::transaction::Transaction<'_>,
    containers: &mut HashMap<String, ObjId>,
    list_values: &mut HashMap<String, Vec<String>>,
    generator_lists: &BTreeMap<String, Vec<String>>,
    op: &Op,
) -> Result<(), automerge::AutomergeError> {
    match op {
        Op::PutKey { target, key, value } => {
            let map = lookup_container(containers, target)?;
            tx.put(&map, key.as_str(), value.as_str())?;
        }
        Op::DeleteKey { target, key } => {
            let map = lookup_container(containers, target)?;
            tx.delete(&map, key.as_str())?;
        }
        Op::Insert {
            target,
            after,
            value,
        } => {
            let list = lookup_container(containers, target)?;
            let entries = list_values.entry(target.clone()).or_default();
            let index = match after {
                Some(after_value) => {
                    if let Some(position) = entries.iter().position(|item| item == after_value) {
                        position + 1
                    } else {
                        fallback_insert_index(generator_lists, target, after_value, entries)
                    }
                }
                None => 0,
            };
            tx.insert(&list, index, value.as_str())?;
            entries.insert(index, value.clone());
        }
        Op::Delete { target, value } => {
            let list = lookup_container(containers, target)?;
            let entries = list_values.entry(target.clone()).or_default();
            if let Some(index) = entries.iter().position(|item| item == value) {
                tx.delete(&list, Prop::Seq(index))?;
                entries.remove(index);
            }
        }
        Op::CreateMap { parent, key, name } => {
            let parent = lookup_container(containers, parent)?;
            let made = tx.put_object(&parent, key.as_str(), ObjType::Map)?;
            containers.insert(name.clone(), made);
        }
        Op::CreateList { parent, key, name } => {
            let parent = lookup_container(containers, parent)?;
            let made = tx.put_object(&parent, key.as_str(), ObjType::List)?;
            containers.insert(name.clone(), made);
            list_values.insert(name.clone(), Vec::new());
        }
    }
    Ok(())
}

pub(crate) fn lookup_container(
    containers: &HashMap<String, ObjId>,
    target: &str,
) -> Result<ObjId, automerge::AutomergeError> {
    containers
        .get(target)
        .cloned()
        .ok_or_else(|| automerge::AutomergeError::InvalidIndex(0))
}

pub(crate) fn fallback_insert_index(
    generator_lists: &BTreeMap<String, Vec<String>>,
    target: &str,
    after: &str,
    entries: &[String],
) -> usize {
    let Some(list) = generator_lists.get(target) else {
        return 0;
    };
    let Some(index) = list.iter().position(|item| item == after) else {
        return 0;
    };
    for previous in list[..index].iter().rev() {
        if let Some(position) = entries.iter().position(|item| item == previous) {
            return position + 1;
        }
    }
    0
}

pub(crate) fn observe(store: &Store) -> JsonValue {
    store.content_json(KIND, ID).unwrap_or(JsonValue::Null)
}

pub(crate) fn p1_violation(case: &Case) -> Result<bool, String> {
    p1_violation_with(case, &|_| {})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum P1Outcome {
    Holds,
    RestoredOrderOnly,
    Violated,
}

/// An element the target deleted, with the elements that preceded it when it was deleted.
struct Restored {
    value: String,
    preceding: Vec<String>,
}

/// Elements the target deleted, keyed by the JSON path of the list they were deleted from.
type Restorations = BTreeMap<Vec<String>, Vec<Restored>>;

fn restorations(history: &History, target: &str) -> Restorations {
    let mut model = base_model();
    let mut found = Restorations::new();
    for invocation in &history.invocations {
        for op in &invocation.ops {
            if let (
                true,
                Op::Delete {
                    target: list,
                    value,
                },
            ) = (invocation.id == target, op)
            {
                record_restored(&model, &mut found, list, value);
            }
            apply_model(&mut model, op);
        }
        if invocation.id == target {
            break;
        }
    }
    found
}

fn record_restored(model: &Model, found: &mut Restorations, list: &str, value: &str) {
    let items = &model.lists[list];
    let index = items
        .iter()
        .position(|item| item == value)
        .expect("deleted value exists");
    found
        .entry(model.paths[list].clone())
        .or_default()
        .push(Restored {
            value: value.to_owned(),
            preceding: items[..index].to_vec(),
        });
}

fn classify_p1(after: &JsonValue, without: &JsonValue, restorations: &Restorations) -> P1Outcome {
    if after == without {
        P1Outcome::Holds
    } else if same_up_to_restored(after, without, &[], restorations) {
        P1Outcome::RestoredOrderOnly
    } else {
        P1Outcome::Violated
    }
}

fn same_up_to_restored(
    after: &JsonValue,
    without: &JsonValue,
    path: &[String],
    restorations: &Restorations,
) -> bool {
    match (after, without) {
        (JsonValue::Array(after_items), JsonValue::Array(without_items)) => {
            let restored = restorations
                .get(path)
                .map(Vec::as_slice)
                .unwrap_or_default();
            arrays_match(after_items, without_items, restored)
        }
        (JsonValue::Object(after_map), JsonValue::Object(without_map)) => {
            after_map.len() == without_map.len()
                && after_map.iter().all(|(key, after_value)| {
                    let mut child = path.to_vec();
                    child.push(key.clone());
                    without_map.get(key).is_some_and(|without_value| {
                        same_up_to_restored(after_value, without_value, &child, restorations)
                    })
                })
        }
        _ => after == without,
    }
}

/// Other elements keep the replay's order; each restored element sits right after its anchor.
fn arrays_match(after: &[JsonValue], without: &[JsonValue], restored: &[Restored]) -> bool {
    without_restored(after, restored) == without_restored(without, restored)
        && restored.iter().all(|entry| {
            match (
                position_of(after, &entry.value),
                position_of(without, &entry.value),
            ) {
                (None, None) => true,
                (Some(index), Some(_)) => placed_after_anchor(after, index, entry, restored),
                _ => false,
            }
        })
}

/// A later invocation may have deleted the predecessor, so the anchor is the nearest
/// preceding element that survived. Only other restored elements may sit between.
fn placed_after_anchor(
    after: &[JsonValue],
    index: usize,
    entry: &Restored,
    restored: &[Restored],
) -> bool {
    let start = entry
        .preceding
        .iter()
        .rev()
        .find_map(|name| position_of(after, name))
        .map_or(0, |anchor| anchor + 1);
    start <= index
        && after[start..index]
            .iter()
            .all(|item| is_restored(item, restored))
}

fn without_restored<'a>(items: &'a [JsonValue], restored: &[Restored]) -> Vec<&'a JsonValue> {
    items
        .iter()
        .filter(|item| !is_restored(item, restored))
        .collect()
}

fn is_restored(item: &JsonValue, restored: &[Restored]) -> bool {
    restored
        .iter()
        .any(|entry| item.as_str() == Some(entry.value.as_str()))
}

fn position_of(items: &[JsonValue], value: &str) -> Option<usize> {
    items.iter().position(|item| item.as_str() == Some(value))
}

pub(crate) fn p1_outcome_with(
    case: &Case,
    prepare: &dyn Fn(&mut Store),
) -> Result<P1Outcome, String> {
    p1_outcome_with_commits(case, prepare, Commits::One)
}

pub(crate) fn p1_outcome_with_commits(
    case: &Case,
    prepare: &dyn Fn(&mut Store),
    commits: Commits,
) -> Result<P1Outcome, String> {
    let analysis = analyse(&case.history, &case.target);
    if !analysis.independent {
        return Ok(P1Outcome::Holds);
    }

    let after_undo = {
        let mut store = play_history_with(&case.history, None, commits)?;
        prepare(&mut store);
        if store.undo(&case.target, &undo_stamp(&case.target)).is_err() {
            return Ok(P1Outcome::Violated);
        }
        observe(&store)
    };

    let without = {
        let store = play_history(&case.history, Some(&case.target))?;
        observe(&store)
    };

    let restored = restorations(&case.history, &case.target);
    Ok(classify_p1(&after_undo, &without, &restored))
}

pub(crate) fn p1_violation_with(case: &Case, prepare: &dyn Fn(&mut Store)) -> Result<bool, String> {
    Ok(matches!(
        p1_outcome_with(case, prepare)?,
        P1Outcome::Violated
    ))
}

pub(crate) fn case_size(case: &Case) -> usize {
    case.history
        .invocations
        .iter()
        .map(|invocation| invocation.ops.len())
        .sum()
}

pub(crate) fn shrink(case: Case) -> Case {
    shrink_by(case, &|candidate| {
        matches!(p1_violation(candidate), Ok(true))
    })
}

/// The smallest case found by dropping invocations, then ops, while `bad` still holds.
pub(crate) fn shrink_by(mut case: Case, bad: &dyn Fn(&Case) -> bool) -> Case {
    loop {
        let mut improved = false;

        for index in (0..case.history.invocations.len()).rev() {
            let id = case.history.invocations[index].id.clone();
            if id == "i1" || id == case.target {
                continue;
            }
            let mut candidate = case.clone();
            candidate.history.invocations.remove(index);
            if valid_history(&candidate.history) && bad(&candidate) {
                case = candidate;
                improved = true;
                break;
            }
        }
        if improved {
            continue;
        }

        'ops: for invocation_index in 0..case.history.invocations.len() {
            let invocation_id = case.history.invocations[invocation_index].id.clone();
            for op_index in (0..case.history.invocations[invocation_index].ops.len()).rev() {
                if invocation_id == case.target
                    && case.history.invocations[invocation_index].ops.len() == 1
                {
                    continue;
                }
                let mut candidate = case.clone();
                candidate.history.invocations[invocation_index]
                    .ops
                    .remove(op_index);
                if valid_history(&candidate.history) && bad(&candidate) {
                    case = candidate;
                    improved = true;
                    break 'ops;
                }
            }
        }

        if !improved {
            return case;
        }
    }
}

pub(crate) fn counterexample_json(case: &Case) -> JsonValue {
    let ops = case
        .history
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation.ops.iter().map(
                move |op| serde_json::json!({"invocation": invocation.id, "op": format!("{op:?}")}),
            )
        })
        .collect::<Vec<_>>();
    serde_json::json!({ "target": case.target, "ops": ops })
}

#[test]
fn generator_and_replay_agree_with_themselves() {
    for seed in 0..50 {
        let case = generate_case(seed);
        assert!(valid_history(&case.history), "seed {seed}");
        assert_ne!(case.target, "i1", "seed {seed}");

        let first = {
            let store = play_history(&case.history, None).expect("history replays");
            observe(&store)
        };
        let second = {
            let store = play_history(&case.history, None).expect("history replays");
            observe(&store)
        };
        assert_eq!(first, second, "seed {seed}");
    }
}

fn insert(value: &str, after: Option<&str>) -> Op {
    Op::Insert {
        target: "i1:items".to_owned(),
        after: after.map(str::to_owned),
        value: value.to_owned(),
    }
}

fn delete(value: &str) -> Op {
    Op::Delete {
        target: "i1:items".to_owned(),
        value: value.to_owned(),
    }
}

/// The base invocation `i1`, then `i2`, `i3`, ... with the given ops.
fn case_of(invocations: Vec<Vec<Op>>, target: &str) -> Case {
    let mut all = vec![Invocation {
        id: "i1".to_owned(),
        ops: Vec::new(),
    }];
    all.extend(
        invocations
            .into_iter()
            .enumerate()
            .map(|(index, ops)| Invocation {
                id: format!("i{}", index + 2),
                ops,
            }),
    );
    Case {
        history: History { invocations: all },
        target: target.to_owned(),
    }
}

fn classify(case: &Case, after: &[&str], without: &[&str]) -> P1Outcome {
    let items = |values: &[&str]| serde_json::json!({"meta": {"items": values}});
    classify_p1(
        &items(after),
        &items(without),
        &restorations(&case.history, &case.target),
    )
}

/// i2 inserts v1, i3 (the target) deletes it, i4 inserts v2 at the head.
fn head_insert_case() -> Case {
    case_of(
        vec![
            vec![insert("v1", None)],
            vec![delete("v1")],
            vec![insert("v2", None)],
        ],
        "i3",
    )
}

#[test]
fn p1_outcome_equal_states_hold() {
    let case = head_insert_case();

    assert_eq!(
        classify(&case, &["v2", "v1"], &["v2", "v1"]),
        P1Outcome::Holds
    );
}

#[test]
fn p1_outcome_restored_at_its_predecessor_is_order_only() {
    let case = head_insert_case();

    assert_eq!(
        classify(&case, &["v1", "v2"], &["v2", "v1"]),
        P1Outcome::RestoredOrderOnly
    );
}

#[test]
fn p1_outcome_restored_element_in_the_wrong_place_is_violated() {
    let case = case_of(
        vec![
            vec![insert("v1", None)],
            vec![insert("v2", Some("v1"))],
            vec![delete("v1")],
            vec![insert("v3", Some("v2"))],
        ],
        "i4",
    );

    assert_eq!(
        classify(&case, &["v2", "v3", "v1"], &["v1", "v2", "v3"]),
        P1Outcome::Violated
    );
}

#[test]
fn p1_outcome_missing_or_changed_element_is_violated() {
    let case = head_insert_case();

    assert_eq!(classify(&case, &["v2"], &["v2", "v1"]), P1Outcome::Violated);
    assert_eq!(
        classify(&case, &["v1", "v3"], &["v2", "v1"]),
        P1Outcome::Violated
    );
}

#[test]
#[ignore = "a measurement: run it by name"]
fn measure_selective_undo() {
    let seeds = env::var("TILO_SEEDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2000);

    let mut histories = 0_u64;
    let mut independent = 0_u64;
    let mut dependent = 0_u64;
    let mut p1_violations = 0_u64;
    let mut replay_errors = 0_u64;
    let mut p1_counterexamples = Vec::new();

    for seed in 0..seeds {
        let case = generate_case(seed);
        let analysis = analyse(&case.history, &case.target);

        histories += 1;
        if analysis.independent {
            independent += 1;
        } else {
            dependent += 1;
        }

        match p1_violation(&case) {
            Ok(true) => {
                p1_violations += 1;
                p1_counterexamples.push(shrink(case));
            }
            Ok(false) => {}
            Err(_) => replay_errors += 1,
        }
    }

    p1_counterexamples.sort_by_key(case_size);

    let output = serde_json::json!({
        "histories": histories,
        "independent": independent,
        "dependent": dependent,
        "replay_errors": replay_errors,
        "p1_violations": p1_violations,
        "p1_counterexamples": p1_counterexamples.iter().take(3).map(counterexample_json).collect::<Vec<_>>(),
    });

    println!("{output}");
}
