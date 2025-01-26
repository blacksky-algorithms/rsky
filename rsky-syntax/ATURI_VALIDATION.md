# Notes on AT-URI Validation

At the time of writing this, this rsky-syntax validation of AT-URI reflects the Typescript implementation of the [syntax package](https://github.com/bluesky-social/atproto/tree/main/packages/syntax) instead of the atproto.com [specification](https://atproto.com/specs/at-uri-scheme).  There are some differences with the typescript and rust validation implementations and the specification for AT URIs:

  - The validation conforms more to the "Full AT URI Syntax" form than the "Restricted AT URI Syntax" that is used in the [lexicon](https://github.com/bluesky-social/atproto/tree/main/packages/lexicon).  
  - The Rust AT-URI validation does admit some invalid syntax.
  - The Rust DID and Handle validation adheres to the specification.
  - The Rust NSID and TID validation adheres to the specification.
  - The Rust Record Key validation adheres to the specification.

  # References

  1. [rsky-syntax PR](https://github.com/blacksky-algorithms/rsky/pull/39).