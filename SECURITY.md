# Security Policy

## Supported Versions

rsky does not yet publish tagged releases; `main` is the actively maintained line. If you
believe a vulnerability affects a specific crate, service, or deployment, please note the
commit, crate version, or image tag in your report.

## Reporting a Vulnerability

Please do **not** open a public GitHub issue for a suspected security vulnerability.

The preferred way to report a vulnerability is through GitHub's private reporting flow:

1. Go to the [Security tab](https://github.com/blacksky-algorithms/rsky/security) of this repository.
2. Click **Report a vulnerability**.
3. Fill in the advisory form with as much detail as you can.

This opens a private conversation with maintainers and keeps the report out of public
view until it's resolved.

If you'd rather not use GitHub, you can instead email **support@blacksky.app**.

In either case, please include:

- A description of the vulnerability and its impact.
- The affected crate, service, or component (e.g. `rsky-pds`, `rsky-relay`).
- Steps to reproduce, including any proof-of-concept code.
- Any special configuration required to reproduce the issue.

## What to Expect

- We'll acknowledge new reports as soon as a maintainer is available.
- We'll work with you to confirm the issue, assess impact, and develop a fix.
- We'll credit reporters in the advisory when a fix is published, unless you'd prefer to
  stay anonymous.

## Scope

In-scope: rsky crates and services in this repository, including authentication,
authorization, data handling, federation, and moderation/labeling behavior.

Out of scope: denial-of-service testing, social engineering, and any testing against
production infrastructure or accounts you don't own. If you need to test against a live
deployment, ask first in your report.
