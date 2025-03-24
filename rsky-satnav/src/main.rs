#![allow(clippy::unused_io_amount)]
use anyhow::{anyhow, bail, Result};
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
use std::collections::HashMap;

// Use iroh-car's asynchronous CarReader.
use iroh_car::CarReader;

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const FAVICON: Asset = asset!("/assets/favicon.ico");

use components::Hero;

mod components;

// ----------------------------------------------------------
// Data structures
// ----------------------------------------------------------
#[derive(Clone, Debug)]
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
// Main app: only shows the MST directory listing
// ----------------------------------------------------------
fn main() {
    launch(App);
}

#[component]
fn App() -> Element {
    let car_data = use_signal(|| None::<CarTree>);

    let on_file_change = {
        let car_data = car_data.clone();
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

    let content = match car_data.as_ref() {
        Some(tree) => rsx! {
            MstRepoView { mst_entries: tree.mst_entries.clone() }
        },
        None => rsx! {
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
                onchange: on_file_change,
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

                let result = dagcbor::from_slice::<Ipld>(data.as_slice());
                let (cli_ipld_json, raw_hex) = match result {
                    Ok(ipld) => (Some(ipld_to_json_cli_style(&ipld)), None),
                    Err(_) => (None, Some(hex_encode(&data))),
                };

                // Insert into block map for MST logic
                block_map.insert(cid_str.clone(), data);

                blocks.push(BlockView {
                    cid: cid_str,
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
