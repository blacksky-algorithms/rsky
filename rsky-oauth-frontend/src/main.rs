use gloo::net::http::Request;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

const SIGN_IN_ENDPOINT: &str = "https://inspired-amusing-tick.ngrok-free.app/oauth/authorize/sign-in";
const ACCEPT_ENDPOINT: &str = "https://inspired-amusing-tick.ngrok-free.app/oauth/authorize/accept";
const REJECT_ENDPOINT: &str = "https://inspired-amusing-tick.ngrok-free.app/oauth/authorize/reject";

#[function_component(App)]
fn app() -> Html {
    let
    let user_state = use_state(|| ("".to_string(), "".to_string(), None as Option<i32>));
    let message = use_state(|| "".to_string());
    let users = use_state(Vec::new);

    let sign_in_callback = {
        let users = users.clone();
        let message = message.clone();
        Callback::from(move |_| {
            let users = users.clone();
            let message = message.clone();
            spawn_local(async move {
                match Request::get("http://127.0.0.1:8000/api/users").send().await {
                    Ok(resp) if resp.ok() => {
                        let fetched_users: Vec<User> = resp.json().await.unwrap_or_default();
                        users.set(fetched_users);
                    }

                    _ => message.set("Failed to fetch users".into()),
                }
            });
        })
    };

    let get_users = {
        let users = users.clone();
        let message = message.clone();
        Callback::from(move |_| {
            let users = users.clone();
            let message = message.clone();
            spawn_local(async move {
                match Request::get("http://127.0.0.1:8000/api/users").send().await {
                    Ok(resp) if resp.ok() => {
                        let fetched_users: Vec<User> = resp.json().await.unwrap_or_default();
                        users.set(fetched_users);
                    }

                    _ => message.set("Failed to fetch users".into()),
                }
            });
        })
    };

    let create_user = {
        let user_state = user_state.clone();
        let message = message.clone();
        let get_users = get_users.clone();
        Callback::from(move |_| {
            let (name, email, _) = (*user_state).clone();
            let user_state = user_state.clone();
            let message = message.clone();
            let get_users = get_users.clone();

            spawn_local(async move {
                let user_data = serde_json::json!({ "name": name, "email": email });

                let response = Request::post("http://127.0.0.1:8000/api/users")
                    .header("Content-Type", "application/json")
                    .body(user_data.to_string())
                    .send()
                    .await;

                match response {
                    Ok(resp) if resp.ok() => {
                        message.set("User created successfully".into());
                        get_users.emit(());
                    }

                    _ => message.set("Failed to create user".into()),
                }

                user_state.set(("".to_string(), "".to_string(), None));
            });
        })
    };

    let update_user = {
        let user_state = user_state.clone();
        let message = message.clone();
        let get_users = get_users.clone();

        Callback::from(move |_| {
            let (name, email, editing_user_id) = (*user_state).clone();
            let user_state = user_state.clone();
            let message = message.clone();
            let get_users = get_users.clone();

            if let Some(id) = editing_user_id {
                spawn_local(async move {
                    let response = Request::put(&format!("http://127.0.0.1:8000/api/users/{}", id))
                        .header("Content-Type", "application/json")
                        .body(serde_json::to_string(&(id, name.as_str(), email.as_str())).unwrap())
                        .send()
                        .await;

                    match response {
                        Ok(resp) if resp.ok() => {
                            message.set("User updated successfully".into());
                            get_users.emit(());
                        }

                        _ => message.set("Failed to update user".into()),
                    }

                    user_state.set(("".to_string(), "".to_string(), None));
                });
            }
        })
    };

    let delete_user = {
        let message = message.clone();
        let get_users = get_users.clone();

        Callback::from(move |id: i32| {
            let message = message.clone();
            let get_users = get_users.clone();

            spawn_local(async move {
                let response = Request::delete(&format!("http://127.0.0.1:8000/api/users/{}", id))
                    .send()
                    .await;

                match response {
                    Ok(resp) if resp.ok() => {
                        message.set("User deleted successfully".into());
                        get_users.emit(());
                    }

                    _ => message.set("Failed to delete user".into()),
                }
            });
        })
    };

    let edit_user = {
        let user_state = user_state.clone();
        let users = users.clone();

        Callback::from(move |id: i32| {
            if let Some(user) = users.iter().find(|u| u.id == id) {
                user_state.set((user.name.clone(), user.email.clone(), Some(id)));
            }
        })
    };

    //html

    html! {
        <div class="container mx-auto p-4">
            <h1 class="text-4xl font-bold text-blue-500 mb-4">{ "User Management" }</h1>
                <div class="mb-4">
                    <input
                        placeholder="Name"
                        value={user_state.0.clone()}
                        oninput={Callback::from({
                            let user_state = user_state.clone();
                            move |e: InputEvent| {
                                let input = e.target_dyn_into::<web_sys::HtmlInputElement>().unwrap();
                                user_state.set((input.value(), user_state.1.clone(), user_state.2));
                            }
                        })}
                        class="border rounded px-4 py-2 mr-2"
                    />
                    <input
                        placeholder="Email"
                        value={user_state.1.clone()}
                        oninput={Callback::from({
                            let user_state = user_state.clone();
                            move |e: InputEvent| {
                                let input = e.target_dyn_into::<web_sys::HtmlInputElement>().unwrap();
                                user_state.set((user_state.0.clone(), input.value(), user_state.2));
                            }
                        })}
                        class="border rounded px-4 py-2 mr-2"
                    />

                    <button
                        onclick={if user_state.2.is_some() { update_user.clone() } else { create_user.clone() }}
                        class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                    >
                        { if user_state.2.is_some() { "Update User" } else { "Create User" } }

                    </button>
                        if !message.is_empty() {
                        <p class="text-green-500 mt-2">{ &*message }</p>
                    }
                </div>

                <button
                    onclick={get_users.reform(|_| ())}
                    class="bg-gray-500 hover:bg-gray-700 text-white font-bold py-2 px-4 rounded mb-4"
                >
                    { "Fetch User List" }
                </button>

                <h2 class="text-2xl font-bold text-gray-700 mb-2">{ "User List" }</h2>

                <ul class="list-disc pl-5">
                    { for (*users).iter().map(|user| {
                        let user_id = user.id;
                        html! {
                            <li class="mb-2">
                                <span class="font-semibold">{ format!("ID: {}, Name: {}, Email: {}", user.id, user.name, user.email) }</span>
                                <button
                                    onclick={delete_user.clone().reform(move |_| user_id)}
                                    class="ml-4 bg-red-500 hover:bg-red-700 text-white font-bold py-1 px-2 rounded"
                                >
                                    { "Delete" }
                                </button>
                                <button
                                    onclick={edit_user.clone().reform(move |_| user_id)}
                                    class="ml-4 bg-yellow-500 hover:bg-yellow-700 text-white font-bold py-1 px-2 rounded"
                                >
                                    { "Edit" }
                                </button>
                            </li>
                        }
                    })}

                </ul>


        </div>
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct User {
    id: i32,
    name: String,
    email: String,
}

fn main() {
    yew::Renderer::<App>::new().render();
}
