# rsky-satnav: Structured Archive Traversal, Navigation & Verification 🛰️ 🚘

**Work-in-Progress:**  
rsky-satnav is a prototype tool for visually exploring DASL CAR and AT Protocol repositories entirely in your browser. All processing is done locally—no data is tracked or sent externally.

## Overview

rsky-satnav lets you load a CAR file (Content-Addressable Archive) and visually inspect its contents using a collapsible, directory-style UI. It leverages MST (Merkle Search Tree) logic to group records by collection for an intuitive file-directory-like experience.

## Features

- **Local-Only Processing:**  
  All CAR file processing happens in your browser. No data is transmitted or stored externally.

- **Visual Exploration:**  
  View repository data in a collapsible, directory-style listing that mimics a file explorer.

- **MST-Based Grouping:**  
  Automatically groups repository records by collection, making it easier to navigate complex data structures.


## Getting a CAR File

CAR files contain all public records from an AT Protocol repository and can be used as part of the "credible exit" path for users exporting their data. To obtain a CAR file:

1. **From the Bluesky app**
  - Go to **Settings > Account > Export My Data**
  - Click **Download CAR file**

2. **Using the Bluesky API & Scripts**
  - Follow the [official guide](https://docs.bsky.app/blog/repo-export#the-repository-car-file) to export via API.
  - Use the provided scripts to fetch your repository archive.

3. **With boat**
  - The tool [boat](https://boat.kelinci.net/repo-export) provides a browser-based interface for exporting CAR files from a given DID.

Once you have a CAR file, you can load it into **rsky-satnav** for structured exploration.


## Roadmap

- **CAR Diffing:**  
  Compare different CAR files to identify changes and differences.

- **CAR Slicing:**  
  Extract and work with subsets of repository data.

- **Enhanced Sorting & Filtering:**  
  Improve UI controls to sort and filter collections more easily.

- **Blob Retrieval & Verification:**  
  Retrieve embedded Blobs and cryptographically verify CAR files.

- **External API Integration (if needed):**
    - Look up DIDs and fetch public signing keys for commit signature verification.
    - Retrieve current PDS data using `com.atproto.sync.getRepo` for downloading repositories.

## Development

rsky-satnav is built with [Dioxus](https://github.com/DioxusLabs/dioxus) for the UI and styled with [Tailwind CSS](https://tailwindcss.com/).

### Prerequisites

- **Rust:** A recent version is required.
- **Node.js & npm:** Needed for managing Tailwind CSS.

### Setup

1. Install npm: https://docs.npmjs.com/downloading-and-installing-node-js-and-npm
2. Install the Tailwind CSS CLI: https://tailwindcss.com/docs/installation
3. Run the following command in the root of the project to start the Tailwind CSS compiler:

```bash
npx tailwindcss -i ./input.css -o ./assets/tailwind.css --watch
```

### Serving The App

Run the following command in the root of your project to start developing with the default platform:

```bash
dx serve
```

To run for a different platform, use the `--platform platform` flag. E.g.
```bash
dx serve --platform desktop
```

## Privacy

All file processing and data exploration is performed entirely on your local machine in the browser. No external tracking or data transmission occurs.

## License

This project is released under the [Apache License 2.0](../LICENSE).
