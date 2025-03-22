#![allow(clippy::unused_io_amount)]
use anyhow::{anyhow, bail, Result};
use cid::Cid;
use dioxus::prelude::*;
use dioxus_web::WebEventExt;
use gloo_file::{futures::read_as_bytes, Blob};
use std::io::Cursor;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

// For DAG-CBOR decoding
use ipld_core::ipld::Ipld;
use serde_ipld_dagcbor as dagcbor;

// For hex encoding
use hex::encode as hex_encode;

// For converting Ipld -> JSON for a readable display.
use serde_json;
use std::collections::{BTreeSet, HashMap, HashSet};

// Use iroh-car's asynchronous CarReader.
use iroh_car::{CarHeader, CarReader, CarWriter};

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const FAVICON: Asset = asset!("/assets/favicon.ico");

use components::Hero;

mod components;

// ----------------------------------------------------------
// Data structures
// ----------------------------------------------------------
#[derive(PartialEq, Clone, Debug)]
struct CarTree {
    /// The root CIDs declared in the CAR header.
    roots: Vec<String>,
    /// We won't display these blocks in the UI, but we still store them
    /// for MST decoding.
    blocks: Vec<BlockView>,

    /// MST-based “repo entries” found by walking the MST
    mst_entries: Vec<MstEntry>,
}

#[derive(Props, PartialEq, Clone, Debug)]
struct BlockView {
    cid: String,
    data: Vec<u8>,
    refs: HashSet<String>,
    cli_ipld_json: Option<String>,
    raw_hex: Option<String>,
}

#[derive(PartialEq, Clone, Debug)]
struct MstEntry {
    collection: String,
    rkey: String,
    record_cid: String,
    record_cli_json: String,
}

// ----------------------------------------------------------
// MST/Commit logic (unchanged from the table version)
// ----------------------------------------------------------
fn extract_cids(ipld: &Ipld, refs: &mut HashSet<String>) {
    match ipld {
        Ipld::Bytes(bytes) => {}
        Ipld::Link(cid) => {
            refs.insert(cid.to_string());
        }
        Ipld::Map(obj) => {
            for (_, v) in obj {
                extract_cids(v, refs);
            }
        }
        Ipld::List(list) => {
            for v in list {
                extract_cids(v, refs);
            }
        }
        other => {}
    }
}

fn convert_ipld_cli_style(ipld: &Ipld) -> serde_json::Value {
    match ipld {
        Ipld::Bytes(bytes) => {
            serde_json::json!({
                "/": {
                    "bytes": base64::encode(bytes).replace("=", "")
                }
            })
        }
        Ipld::Link(cid) => {
            serde_json::json!({ "/": cid.to_string() })
        }
        Ipld::Map(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj {
                map.insert(k.clone(), convert_ipld_cli_style(v));
            }
            serde_json::Value::Object(map)
        }
        Ipld::List(list) => {
            let arr = list
                .iter()
                .map(|v| convert_ipld_cli_style(v))
                .collect::<Vec<_>>();
            serde_json::Value::Array(arr)
        }
        other => serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
    }
}

fn ipld_to_json_cli_style(ipld: &Ipld) -> String {
    let val = convert_ipld_cli_style(ipld);
    serde_json::to_string_pretty(&val).unwrap_or_else(|_| "<error converting to JSON>".into())
}

#[derive(Debug)]
struct Commit {
    version: u64,
    did: String,
    data: ipld_core::cid::Cid, // MST root
    rev: String,
    prev: Option<ipld_core::cid::Cid>,
    sig: Vec<u8>,
}

#[derive(Debug)]
struct MstNode {
    l: Option<ipld_core::cid::Cid>,
    e: Vec<TreeEntry>,
}

#[derive(Debug)]
struct TreeEntry {
    p: usize,
    k: Vec<u8>,
    v: ipld_core::cid::Cid,
    t: Option<ipld_core::cid::Cid>,
}

type BlockMap = HashMap<String, Vec<u8>>;

// Reads the commit from the single root
fn read_commit(block_map: &BlockMap, root_cid: &str) -> Result<Commit> {
    let bytes = block_map
        .get(root_cid)
        .ok_or_else(|| anyhow!("Root commit block not found: {}", root_cid))?;
    let ipld = dagcbor::from_slice::<Ipld>(bytes)?;

    let map = match ipld {
        Ipld::Map(m) => m,
        _ => bail!("Commit is not a Map"),
    };

    let version = match map.get("version") {
        Some(Ipld::Integer(n)) => *n as u64,
        _ => bail!("Commit version is missing or not an integer"),
    };
    let did = match map.get("did") {
        Some(Ipld::String(s)) => s.clone(),
        _ => bail!("Commit did is missing or not a string"),
    };
    let data_cid = match map.get("data") {
        Some(Ipld::Link(cid)) => cid.clone(),
        _ => bail!("Commit data is missing or not a link"),
    };
    let rev = match map.get("rev") {
        Some(Ipld::String(s)) => s.clone(),
        _ => bail!("Commit rev is missing or not a string"),
    };
    let prev = match map.get("prev") {
        Some(Ipld::Link(cid)) => Some(cid.clone()),
        Some(Ipld::Null) | None => None,
        _ => bail!("Commit prev is invalid"),
    };
    let sig_bytes = match map.get("sig") {
        Some(Ipld::Bytes(b)) => b.clone(),
        _ => bail!("Commit sig is missing or not bytes"),
    };

    Ok(Commit {
        version,
        did,
        data: data_cid,
        rev,
        prev,
        sig: sig_bytes,
    })
}

// Reads and decodes an MST node from the block map
fn read_mst_node(block_map: &BlockMap, cid: &ipld_core::cid::Cid) -> Result<MstNode> {
    let bytes = block_map
        .get(&cid.to_string())
        .ok_or_else(|| anyhow!("MST node not found in block map: {}", cid))?;

    let ipld = dagcbor::from_slice::<Ipld>(bytes)
        .map_err(|e| anyhow!("Error decoding MST node cbor: {:?}", e))?;

    let map = match ipld {
        Ipld::Map(m) => m,
        _ => bail!("MST node is not a Map"),
    };

    let l_cid = match map.get("l") {
        Some(Ipld::Link(cid)) => Some(cid.clone()),
        Some(Ipld::Null) | None => None,
        other => bail!("MST node 'l' is invalid: {:?}", other),
    };

    let e_list = match map.get("e") {
        Some(Ipld::List(list)) => list,
        _ => bail!("MST node 'e' is missing or not a list"),
    };

    let mut entries = Vec::new();
    for item in e_list {
        let map = match item {
            Ipld::Map(m) => m,
            _ => bail!("TreeEntry is not a map"),
        };

        let p_val = match map.get("p") {
            Some(Ipld::Integer(i)) => *i as usize,
            _ => bail!("TreeEntry 'p' is invalid"),
        };

        let k_val = match map.get("k") {
            Some(Ipld::Bytes(b)) => b.clone(),
            _ => bail!("TreeEntry 'k' is not bytes"),
        };

        let v_cid = match map.get("v") {
            Some(Ipld::Link(cid)) => cid.clone(),
            _ => bail!("TreeEntry 'v' is not a link"),
        };

        let t_cid = match map.get("t") {
            Some(Ipld::Link(cid)) => Some(cid.clone()),
            Some(Ipld::Null) | None => None,
            other => bail!("TreeEntry 't' is invalid: {:?}", other),
        };

        entries.push(TreeEntry {
            p: p_val,
            k: k_val,
            v: v_cid,
            t: t_cid,
        });
    }

    Ok(MstNode {
        l: l_cid,
        e: entries,
    })
}

// Collect (collection, rkey, record_cid) from MST
fn walk_mst_entries(
    block_map: &BlockMap,
    mst_root: &ipld_core::cid::Cid,
) -> Result<Vec<(String, String, ipld_core::cid::Cid)>> {
    let mut out = Vec::new();
    walk_mst(block_map, mst_root, String::new(), &mut out)?;
    Ok(out)
}

// Recursively walk an MST node
fn walk_mst(
    block_map: &BlockMap,
    node_cid: &ipld_core::cid::Cid,
    mut last_key: String,
    out: &mut Vec<(String, String, ipld_core::cid::Cid)>,
) -> Result<()> {
    let mst_node = read_mst_node(block_map, node_cid)?;

    // Left subtree
    if let Some(left_cid) = mst_node.l {
        walk_mst(block_map, &left_cid, last_key.clone(), out)?;
    }

    // Entries
    for entry in mst_node.e.iter() {
        let prefix = &last_key[..entry.p.min(last_key.len())];
        let partial = String::from_utf8_lossy(&entry.k);
        let full_key = format!("{}{}", prefix, partial);

        // Right subtree
        if let Some(right_subtree_cid) = &entry.t {
            walk_mst(block_map, right_subtree_cid, full_key.clone(), out)?;
        }

        // The collection is everything before the first '/', the rkey is after
        let parts: Vec<&str> = full_key.splitn(2, '/').collect();
        if parts.len() == 2 {
            let (collection, rkey) = (parts[0], parts[1]);
            out.push((collection.to_string(), rkey.to_string(), entry.v.clone()));
        }

        last_key = full_key;
    }

    Ok(())
}

// ----------------------------------------------------------
// Collapsible directory listing component
// ----------------------------------------------------------
#[component]
fn MstRepoView(mst_entries: Vec<MstEntry>) -> Element {
    // Group them by collection
    let mut grouped: HashMap<String, Vec<MstEntry>> = HashMap::new();
    for entry in mst_entries {
        grouped
            .entry(entry.collection.clone())
            .or_default()
            .push(entry);
    }

    // Sort by collection name
    let mut sorted: Vec<(String, Vec<MstEntry>)> = grouped.into_iter().collect();
    sorted.sort_by_key(|(coll, _)| coll.clone());

    // If empty, just show a message
    if sorted.is_empty() {
        return rsx! { p { "No MST entries found, or no commit found." } };
    }

    // A single "Root" with a nested <ul> of “folders” (collections).
    // Each folder is a <details> that can be toggled open/closed,
    // and each record is a nested <details> for the JSON.
    rsx! {
        // Root container
        ul {
            li {
                details {
                    open: "true", // keep the root open by default
                    summary {
                        class: "cursor-pointer font-semibold text-blue-700",
                        "Root"
                    }
                    ul {
                        class: "ml-6 list-none", // indent the child items
                        for (collection, items) in sorted {
                            li {
                                details {
                                    summary {
                                        class: "cursor-pointer flex items-center",
                                        // A simple folder icon, tweak or remove as you like:
                                        span { class: "mr-1", "📁" }
                                        span { "{collection}" }
                                    }
                                    ul {
                                        class: "ml-6 list-none",
                                        for entry in items {
                                            li {
                                                details {
                                                    summary {
                                                        class: "cursor-pointer flex items-center",
                                                        // A simple file icon:
                                                        span { class: "mr-1", "📄" }
                                                        // Show rkey as the “filename”
                                                        span { "{entry.rkey}.json" }
                                                    }
                                                    // The record JSON is revealed upon expansion
                                                    pre {
                                                        class: "ml-6 mt-1 whitespace-pre-wrap text-sm bg-gray-100 p-2 rounded",
                                                        "{entry.record_cli_json}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ----------------------------------------------------------
// Collapsible diffing component
// ----------------------------------------------------------
#[component]
fn MstDiffView(tree1: CarTree, tree2: CarTree) -> Element {
    if tree1.roots[0] == tree2.roots[0] {
        return rsx! {
            p { "Identical files." }
            MstRepoView { mst_entries: tree1.mst_entries.clone() }
        };
    }

    let map1 = tree1
        .mst_entries
        .iter()
        .enumerate()
        .map(|(idx, ent)| (&ent.record_cid, idx))
        .collect::<HashMap<_, _>>();
    let map2 = tree2
        .mst_entries
        .iter()
        .enumerate()
        .map(|(idx, ent)| (&ent.record_cid, idx))
        .collect::<HashMap<_, _>>();
    let mut added_cids = HashSet::new();
    let mut added_records = HashMap::<_, BTreeSet<_>>::new();
    for (entry, idx) in &map2 {
        if let Some(idx1) = map1.get(entry) {
            assert_eq!(tree1.mst_entries[*idx1], tree2.mst_entries[*idx]);
        } else {
            let entry = &tree2.mst_entries[*idx];
            added_cids.insert(entry.record_cid.clone());
            added_records
                .entry(entry.collection.clone())
                .or_default()
                .insert(*idx);
        }
    }
    let mut added_records = added_records
        .into_iter()
        .map(|(coll, entries)| {
            (
                coll,
                entries
                    .into_iter()
                    .map(|idx| tree2.mst_entries[idx].clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    added_records.sort_by_key(|(coll, _)| coll.clone());
    let mut removed_cids = HashSet::new();
    let mut removed_records = HashMap::<_, BTreeSet<_>>::new();
    for (entry, idx) in &map1 {
        if let Some(idx2) = map2.get(entry) {
            assert_eq!(tree1.mst_entries[*idx], tree2.mst_entries[*idx2]);
        } else {
            let entry = &tree1.mst_entries[*idx];
            removed_cids.insert(entry.record_cid.clone());
            removed_records
                .entry(entry.collection.clone())
                .or_default()
                .insert(*idx);
        }
    }
    let mut removed_records = removed_records
        .into_iter()
        .map(|(coll, entries)| {
            (
                coll,
                entries
                    .into_iter()
                    .map(|idx| tree1.mst_entries[idx].clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    removed_records.sort_by_key(|(coll, _)| coll.clone());

    let map1 = tree1
        .blocks
        .iter()
        .enumerate()
        .map(|(idx, blk)| (&blk.cid, idx))
        .collect::<HashMap<_, _>>();
    let map2 = tree2
        .blocks
        .iter()
        .enumerate()
        .map(|(idx, blk)| (&blk.cid, idx))
        .collect::<HashMap<_, _>>();
    let mut added_blocks = BTreeSet::new();
    for (block, idx) in &map2 {
        if !map1.contains_key(block) {
            added_blocks.insert(*idx);
        }
    }
    let added_blocks = added_blocks
        .into_iter()
        .map(|idx| (idx, tree2.blocks[idx].clone()))
        .collect::<Vec<_>>();
    let mut removed_blocks = BTreeSet::new();
    for (block, idx) in &map1 {
        if !map2.contains_key(block) {
            removed_blocks.insert(*idx);
        }
    }
    let removed_blocks = removed_blocks
        .into_iter()
        .map(|idx| (idx, tree1.blocks[idx].clone()))
        .collect::<Vec<_>>();

    let mut from_map = HashMap::<_, HashSet<_>>::new();
    for (_, block) in added_blocks.iter().chain(removed_blocks.iter()) {
        for cid in &block.refs {
            from_map
                .entry(cid.clone())
                .or_default()
                .insert(block.cid.clone());
        }
    }

    let curr = use_signal(|| String::new());
    let froms = use_signal(|| HashSet::new());
    let intos = use_signal(|| HashSet::new());
    let onhover = |cid: String, into: HashSet<String>| {
        let mut curr = curr.clone();
        let mut froms = froms.clone();
        let mut intos = intos.clone();
        let from = from_map.get(&cid).cloned().unwrap_or_default();
        move |_| {
            curr.set(cid.clone());
            froms.set(from.clone());
            intos.set(into.clone());
        }
    };

    let view = rsx! {
        // Root container
        ul {
            li {
                details {
                    open: "true", // keep the root open by default
                    summary {
                        class: "cursor-pointer font-semibold text-blue-700",
                        "Root"
                    }
                    ul {
                        li {
                            onmouseover: onhover(String::new(), [tree1.roots[0].clone()].into()),
                            if tree1.roots[0] == *curr.read() {
                                span { class: "cursor-pointer flex items-center font-bold", style: "color: blue", "old: {tree1.roots[0]}" }
                            } else {
                                span { class: "cursor-pointer flex items-center font-bold", "old: {tree1.roots[0]}" }
                            }
                        }
                        li {
                            onmouseover: onhover(String::new(), [tree2.roots[0].clone()].into()),
                            if tree2.roots[0] == *curr.read() {
                                span { class: "cursor-pointer flex items-center font-bold", style: "color: blue", "new: {tree2.roots[0]}" }
                            } else {
                                span { class: "cursor-pointer flex items-center font-bold", "new: {tree2.roots[0]}" }
                            }
                        }
                    }
                }
            }
            li {
                details {
                    open: "true", // keep the root open by default
                    summary {
                        class: "cursor-pointer font-semibold text-blue-700",
                        "Blocks"
                    }
                    ul {
                        class: "ml-6 list-none", // indent the child items
                        for (collection, items) in [("added", &added_blocks), ("removed", &removed_blocks)] {
                            li {
                                details {
                                    summary {
                                        class: "cursor-pointer flex items-center",
                                        // A simple folder icon, tweak or remove as you like:
                                        span { class: "mr-1", "📁" }
                                        span { "{collection}" }
                                    }
                                    ul {
                                        class: "ml-6 list-none",
                                        for (idx, entry) in items {
                                            li {
                                                onmouseover: onhover(entry.cid.clone(), entry.refs.clone()),
                                                details {
                                                    summary {
                                                        class: "cursor-pointer flex items-center",
                                                        // A simple file icon:
                                                        // Show rkey as the “filename”
                                                        if entry.cid == tree1.roots[0] || entry.cid == tree2.roots[0] {
                                                            span { class: "mr-1", "🏁" }
                                                        } else if added_cids.contains(&entry.cid) || removed_cids.contains(&entry.cid) {
                                                            span { class: "mr-1", "📀" }
                                                        } else {
                                                            span { class: "mr-1", "📦" }
                                                        }
                                                        if froms.read().contains(&entry.cid) {
                                                            span { style: "color: blue", "{idx}: {entry.cid}.json" }
                                                        } else if intos.read().contains(&entry.cid) {
                                                            span { style: "color: red", "{idx}: {entry.cid}.json" }
                                                        } else {
                                                            span { "{idx}: {entry.cid}.json" }
                                                        }
                                                    }
                                                    if let Some(json) = entry.cli_ipld_json.as_ref() {
                                                        // The record JSON is revealed upon expansion
                                                        pre {
                                                            class: "ml-6 mt-1 whitespace-pre-wrap text-sm bg-gray-100 p-2 rounded",
                                                            "{json}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            li {
                details {
                    open: "true", // keep the root open by default
                    summary {
                        class: "cursor-pointer font-semibold text-blue-700",
                        "Added Records"
                    }
                    ul {
                        class: "ml-6 list-none", // indent the child items
                        for (collection, items) in added_records {
                            li {
                                details {
                                    summary {
                                        class: "cursor-pointer flex items-center",
                                        // A simple folder icon, tweak or remove as you like:
                                        span { class: "mr-1", "📁" }
                                        span { "{collection}" }
                                    }
                                    ul {
                                        class: "ml-6 list-none",
                                        for entry in items {
                                            li {
                                                details {
                                                    onmouseover: onhover(entry.record_cid.clone(), HashSet::new()),
                                                    summary {
                                                        class: "cursor-pointer flex items-center",
                                                        // A simple file icon:
                                                        span { class: "mr-1", "📀" }
                                                        // Show rkey as the “filename”
                                                        if froms.read().contains(&entry.record_cid) {
                                                            span { style: "color: blue", "{entry.rkey}.json ({entry.record_cid})" }
                                                        } else if intos.read().contains(&entry.record_cid) {
                                                            span { style: "color: red", "{entry.rkey}.json ({entry.record_cid})" }
                                                        } else {
                                                            span { "{entry.rkey}.json ({entry.record_cid})" }
                                                        }
                                                    }
                                                    // The record JSON is revealed upon expansion
                                                    pre {
                                                        class: "ml-6 mt-1 whitespace-pre-wrap text-sm bg-gray-100 p-2 rounded",
                                                        "{entry.record_cli_json}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            li {
                details {
                    open: "true", // keep the root open by default
                    summary {
                        class: "cursor-pointer font-semibold text-blue-700",
                        "Removed Records"
                    }
                    ul {
                        class: "ml-6 list-none", // indent the child items
                        for (collection, items) in removed_records {
                            li {
                                details {
                                    summary {
                                        class: "cursor-pointer flex items-center",
                                        // A simple folder icon, tweak or remove as you like:
                                        span { class: "mr-1", "📁" }
                                        span { "{collection}" }
                                    }
                                    ul {
                                        class: "ml-6 list-none",
                                        for entry in items {
                                            li {
                                                onmouseover: onhover(entry.record_cid.clone(), HashSet::new()),
                                                details {
                                                    summary {
                                                        class: "cursor-pointer flex items-center",
                                                        // A simple file icon:
                                                        span { class: "mr-1", "📀" }
                                                        // Show rkey as the “filename”
                                                        if froms.read().contains(&entry.record_cid) {
                                                            span { style: "color: blue", "{entry.rkey}.json ({entry.record_cid})" }
                                                        } else if intos.read().contains(&entry.record_cid) {
                                                            span { style: "color: red", "{entry.rkey}.json ({entry.record_cid})" }
                                                        } else {
                                                            span { "{entry.rkey}.json ({entry.record_cid})" }
                                                        }
                                                    }
                                                    // The record JSON is revealed upon expansion
                                                    pre {
                                                        class: "ml-6 mt-1 whitespace-pre-wrap text-sm bg-gray-100 p-2 rounded",
                                                        "{entry.record_cli_json}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    spawn_local(async move {
        match write_car(tree2.roots[0].clone(), added_blocks).await {
            Ok(()) => (),
            Err(err) => {
                web_sys::console::error_1(&JsValue::from_str(&format!("{err:?}")));
            }
        }
    });

    view
}

// ----------------------------------------------------------
// Main app: only shows the MST directory listing
// ----------------------------------------------------------
fn main() {
    launch(App);
}

#[component]
fn App() -> Element {
    let car_data1 = use_signal(|| None::<CarTree>);
    let car_data2 = use_signal(|| None::<CarTree>);

    let on_file_change = |car_data: Signal<Option<CarTree>>| {
        move |evt: Event<FormData>| {
            if let Some(target) = evt.try_as_web_event().unwrap().target() {
                if let Ok(input) = target.dyn_into::<HtmlInputElement>() {
                    if let Some(file_list) = input.files() {
                        if let Some(file) = file_list.get(0) {
                            let mut car_data = car_data.clone();
                            spawn_local(async move {
                                match load_car(file).await {
                                    Ok(tree) => car_data.set(Some(tree)),
                                    Err(err) => {
                                        web_sys::console::error_1(&JsValue::from_str(&format!(
                                            "{err:?}"
                                        )));
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }
    };

    let content = match (car_data1.as_ref(), car_data2.as_ref()) {
        (Some(tree1), Some(tree2)) => rsx! {
            p { "Diff:" }
            MstDiffView { tree1: tree1.clone(), tree2: tree2.clone() }
        },
        (Some(tree), None) | (None, Some(tree)) => rsx! {
            p { "Select 2nd file to diff." }
            MstRepoView { mst_entries: tree.mst_entries.clone() }
        },
        (None, None) => rsx! {
            p { "No CAR file loaded yet." }
        },
    };

    rsx! {
        link { rel: "icon", href: FAVICON },
        link { rel: "stylesheet", href: TAILWIND_CSS },
        link { rel: "stylesheet", href: MAIN_CSS },

        Hero {}

        div {
            class: "p-4",
            h1 {
                class: "text-3xl font-bold mb-4",
                "Directory Style DASL CAR Viewer"
            }
            input {
                r#type: "file",
                accept: ".car",
                onchange: on_file_change(car_data1.clone()),
                class: "bg-purple-500 hover:bg-purple-700 text-white font-bold py-2 px-4 rounded"
            }
            span { " " }
            input {
                r#type: "file",
                accept: ".car",
                onchange: on_file_change(car_data2.clone()),
                class: "bg-purple-500 hover:bg-purple-700 text-white font-bold py-2 px-4 rounded"
            }
            {content}
        }
    }
}

// ----------------------------------------------------------
// CAR loading logic is unchanged. We store blocks for MST
// decoding and build `mst_entries` for the UI.
// ----------------------------------------------------------
async fn load_car(file: web_sys::File) -> Result<CarTree> {
    let blob = Blob::from(file);
    let bytes = read_as_bytes(&blob).await?;
    let mut cursor = Cursor::new(bytes);

    let mut reader = CarReader::new(&mut cursor)
        .await
        .map_err(|e| anyhow!("CARReader init error: {:?}", e))?;

    let header = reader.header();
    let root_cids: Vec<String> = header.roots().iter().map(|cid| cid.to_string()).collect();

    let mut block_map: BlockMap = HashMap::new();
    let mut blocks = Vec::new();

    // Collect all blocks
    loop {
        match reader.next_block().await {
            Ok(Some(block)) => {
                let cid_str = block.0.to_string();
                let data = block.1;

                let mut refs = HashSet::new();
                let result = dagcbor::from_slice::<Ipld>(data.as_slice());
                let (cli_ipld_json, raw_hex) = match result {
                    Ok(ipld) => {
                        extract_cids(&ipld, &mut refs);
                        (Some(ipld_to_json_cli_style(&ipld)), None)
                    }
                    Err(_) => (None, Some(hex_encode(&data))),
                };

                // Insert into block map for MST logic
                block_map.insert(cid_str.clone(), data.clone());

                blocks.push(BlockView {
                    cid: cid_str,
                    data,
                    refs,
                    cli_ipld_json,
                    raw_hex,
                });
            }
            Ok(None) => break,
            Err(e) => bail!("CAR block error: {:?}", e),
        }
    }

    // Attempt MST parse if exactly 1 root
    let mut mst_entries = Vec::new();
    if root_cids.len() == 1 {
        let commit_cid = &root_cids[0];
        if let Ok(commit) = read_commit(&block_map, commit_cid) {
            if let Ok(all) = walk_mst_entries(&block_map, &commit.data) {
                for (collection, rkey, rec_cid) in all {
                    let rec_cid_str = rec_cid.to_string();
                    if let Some(bytes) = block_map.get(&rec_cid_str) {
                        if let Ok(ipld) = dagcbor::from_slice::<Ipld>(bytes) {
                            let cli_json = ipld_to_json_cli_style(&ipld);
                            mst_entries.push(MstEntry {
                                collection,
                                rkey,
                                record_cid: rec_cid_str,
                                record_cli_json: cli_json,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(CarTree {
        roots: root_cids,
        blocks,
        mst_entries,
    })
}

async fn write_car(root: String, blocks: Vec<(usize, BlockView)>) -> Result<()> {
    let header = CarHeader::new_v1(vec![Cid::try_from(root)?]);
    let mut writer = CarWriter::new(header, vec![]);
    for (_, block) in blocks {
        let cid = Cid::try_from(block.cid.as_str())?;
        writer.write(cid, block.data).await?;
    }
    let _buffer = writer.finish().await?;
    Ok(())
}
