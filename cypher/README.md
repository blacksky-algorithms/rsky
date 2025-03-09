# Cypher

**⚠️ Warning:** *This project is currently very unstable—more of a concept than a working prototype. Use cautiously and expect frequent changes.*

An AT Protocol app-view designed for local-only posting and global views. Cypher is inspired by [Hometown](https://github.com/hometown-fork/hometown) from the Fediverse, offering a space for communities to engage without their posts automatically federating. While AT Protocol prioritizes public content, Cypher acknowledges the need for private, local-first social interactions.

## Features

- **Local-only posting**: Posts are stored on-disk per instance and do not federate out.
- **AT Protocol integration**: Users authenticate via OAuth, leveraging decentralized identity and their own PDS.
- **Moderation compatibility**: Integrates with Ozone and third-party moderation services.
- **Cross-platform support**: Built using [Dioxus](https://github.com/DioxusLabs/dioxus) for flexibility across platforms.
- **Global views**: Cypher can consume the public firehose to provide a broader experience while keeping local communities intact.

## Roadmap

### v1 Implementation
- Store local-only posts in a central database on the app-view.
- Ensure compatibility with existing AT Protocol APIs.
- Allow Cypher admins to manage moderation policies for their instance.
- Support instance-level moderation and integration with external moderation services.
- **Arbitrary group management**: Initially supporting list-based groups, with future expansion into PDS-based and domain-based group management.

### Future Considerations
- Migrate to AT Protocol's private state model when consensus is reached.
- Introduce "namespaces" for structuring private and public data within AT Protocol.
- Enable feed generators for local-only posts.
- Support authentication and authorization mechanisms for private content access.

## Development

### Frontend Setup

Cypher's frontend uses Dioxus and Tailwind CSS.

#### Tailwind Installation
1. Install npm: [npm documentation](https://docs.npmjs.com/downloading-and-installing-node-js-and-npm)
2. Install the Tailwind CSS CLI: [Tailwind installation](https://tailwindcss.com/docs/installation)
3. Run the following command to start the Tailwind CSS compiler:

```bash
npx tailwindcss -i ./input.css -o ./assets/tailwind.css --watch
```

### Running the App

To start developing with the default platform:

```bash
dx serve
```

To specify a platform (e.g., desktop):

```bash
dx serve --platform desktop
```

## Privacy & Moderation

While local-only posts do not federate, Cypher prioritizes moderation to ensure safe community interactions. Each instance is responsible for removing harmful or illegal content. Integration with external moderation services, such as Ozone, allows for scalable and flexible moderation policies.

## Future of Private Content in AT Protocol

The AT Protocol community is [actively discussing](https://github.com/bluesky-social/atproto/discussions/3363#discussioncomment-12099116) the introduction of private repositories and namespaces. The proposed model would:
- Introduce "namespaces" to store records privately in a user’s PDS.
- Differentiate between public and private storage, ensuring access control by DIDs.
- Provide a structured, scalable system for handling private data.
- Support different access control levels, including collection-level and record-level permissions.
- Maintain AT Protocol's addressable record system while enabling private interactions.

While these features are not yet finalized, Cypher aims to integrate them as they become part of the AT Protocol standard.

## Future Vision

These are early ideas, not guarantees, of potential features and services:

- **Managed Hosting Services**: Blacksky Algorithms may offer hosting solutions that allow community builders to quickly spin up Cypher instances without worrying about infrastructure, letting them focus on building and engaging their communities.
- **Server Boost Features**: Enhanced hosting capabilities such as video hosting and increased resources tailored to community-specific needs.
- **Community Funding Integrations**: Potential integrations with platforms like [Open Collective](https://opencollective.com/) to empower community members to financially support their community.
- **Customizable Community Badges**: Community-specific configurable badges to recognize and reward member contributions (e.g., "Early Adopter", "Helpful Guide").

These ideas reflect a commitment to empowering community creators, providing them with robust tools, flexibility, and resources needed for effective community building.

## License

Cypher is released under the [Apache License 2.0](../LICENSE).

