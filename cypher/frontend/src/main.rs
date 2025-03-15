use dioxus::prelude::*;
use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent, Window};

mod components;

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[derive(Clone, Copy)]
struct DarkMode(bool);

#[derive(Serialize, Deserialize, Clone)]
struct Post {
    uri: String,
    author: String,
    created_at: String,
    text: Option<String>,
    external_uri: Option<String>,
    external_title: Option<String>,
    external_description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    session_id: String,
    did: String,
}

fn main() {
    launch(App);
}

#[component]
fn App() -> Element {
    let posts = use_signal(|| Vec::<Post>::new());
    let session = use_signal(|| LocalStorage::get::<Session>("session").ok());

    let login_url = "http://127.0.0.1:8080/login";

    let mut event_src = use_signal(|| {
        EventSource::new("http://127.0.0.1:8080/stream").expect("Failed to connect to SSE")
    });

    use_context_provider(|| Signal::new(DarkMode(false)));
    let is_logged_in = session.read().is_some();
    let component = if is_logged_in {
        rsx! {
            h1 { class: "text-2xl font-bold", "Timeline" },
            PostInputBox {},
            PostList { posts: posts.clone() }
        }
    } else {
        rsx! {
            div { class: "flex flex-col items-center justify-center min-h-screen",
                h1 { class: "text-3xl font-bold", "Welcome to the App" },
                a {
                    class: "bg-blue-500 text-white px-4 py-2 rounded mt-4",
                    href: login_url,
                    "Login with OAuth"
                }
            }
        }
    };
    rsx! {
        link { rel: "stylesheet", href: TAILWIND_CSS },
        div { class: "container mx-auto p-4",
            {component}
        }
    }
}

#[component]
fn PostInputBox() -> Element {
    let mut text = use_signal(String::new);
    let session = use_signal(|| LocalStorage::get::<Session>("session").ok());

    let submit_post = move |_| {
        if let Some(session) = session.read().as_ref() {
            let post = Post {
                uri: "new".to_string(),
                author: session.did.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
                text: Some(text.read().clone()),
                external_uri: None,
                external_title: None,
                external_description: None,
            };

            wasm_bindgen_futures::spawn_local(async move {
                let _ = reqwest::Client::new()
                    .post("http://127.0.0.1:8080/post")
                    .json(&post)
                    .send()
                    .await;
            });

            text.set(String::new());
        }
    };

    rsx! {
        div { class: "mt-4",
            textarea {
                class: "w-full border p-2 rounded",
                value: "{text}",
                oninput: move |e| text.set(e.value().clone())
            }
            button {
                class: "bg-blue-500 text-white px-4 py-2 rounded mt-2",
                onclick: submit_post,
                "Post"
            }
        }
    }
}

#[component]
fn PostList(posts: Signal<Vec<Post>>) -> Element {
    rsx! {
        div { class: "mt-4",
            for post in posts.read().iter() {
                div { class: "border p-2 rounded mb-2",
                    h2 { class: "font-bold", "{post.author}" },
                    p { "{post.text.clone().unwrap_or_default()}" },
                    p { class: "text-sm text-gray-500", "{post.created_at}" }
                }
            }
        }
    }
}
