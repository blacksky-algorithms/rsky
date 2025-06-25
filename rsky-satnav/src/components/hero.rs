use dioxus::prelude::*;

const HEADER_SVG: Asset = asset!("/assets/header.svg");

#[component]
pub fn Hero() -> Element {
    rsx! {
        // Full‑width coloured band (optional – remove bg‑indigo‑50 if you prefer plain)
        section {
            id: "hero",
            class: "bg-indigo-50 py-12 px-4 sm:px-8",
            div { class: "max-w-4xl mx-auto",

                // Logo / illustration
                img { src: HEADER_SVG, id: "header", class: "w-full max-w-md mb-8" }

                // Headline
                h1 { class: "text-4xl sm:text-5xl font-extrabold leading-tight mb-4 text-gray-900",
                     "A CAR viewer for atproto’s \"credible exit\"" }

                // Tagline
                p  { class: "text-lg sm:text-xl mb-8 text-gray-700",
                     "rsky‑satnav lets anyone open, inspect and diff their exported AT Protocol "
                     "repository—all locally in the browser." }

                // How‑it‑works steps
                h2 { class:"text-2xl font-semibold mb-3 text-gray-900", "How it works" }
                ol { class:"list-decimal list-inside space-y-2 text-gray-700",
                    li { "In Blacksky go to \"Settings → Account → Export my data\" (or fetch via API) to download your \".car\" file." }
                    li { "Drop the file into the viewer below, or click \"Choose file\". Load a second export to see a visual diff." }
                    li { "satnav parses everything in‑browser—no servers, no uploads—and renders a familiar folder‑style view." }
                }

                // Problem / value prop
                h2 { class:"text-2xl font-semibold mt-8 mb-3 text-gray-900", "Why bother?" }
                p  { class:"text-gray-700 mb-6",
                     "CAR archives are the backbone of AT Protocol’s data‑portability promise, but they’re "
                     "a niche developer format. satnav gives everyday users the confidence to verify that "
                     "a hosting service really returned their *entire* repository—nothing missing, nothing altered." }

                // Privacy note
                h2 { class:"text-2xl font-semibold mt-8 mb-3 text-gray-900", "100 % local & private" }
                p  { class:"text-gray-700 mb-8",
                     "The app is a single‑page WASM bundle. Your CAR never leaves your device, and no analytics are embedded." }

                // Call‑to‑action jumps to the viewer section
                a  { href:"#viewer",
                     class:"inline-block bg-indigo-600 hover:bg-indigo-700 text-white font-semibold py-3 px-6 rounded-lg shadow",
                     "Open my CAR file ↓" }
            }
        }
    }
}