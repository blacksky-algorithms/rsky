# Security Policy

## Supported Versions

The `main` branch is the active development line for rsky. If you believe a vulnerability affects a released crate, service, or deployment artifact, please include the affected version, commit, or image tag in your report.

## Reporting a Vulnerability

Please do **not** open a public GitHub issue for suspected security vulnerabilities.

Email security reports privately to **rudy@blacksky.app**. This address is already listed in `CONTRIBUTING.md` for security-vulnerability reports and is repeated here so researchers can find it from GitHub's security-policy entry point.

Include as much of the following as you can:

- A short summary of the suspected vulnerability.
- The affected rsky component, crate, service, endpoint, or deployment mode.
- Steps to reproduce, proof-of-concept code, logs, or screenshots where safe to share.
- Expected impact, including whether the issue affects confidentiality, integrity, availability, identity, moderation, federation, or account data.
- Any conditions required to exploit the issue.
- Your preferred contact method for follow-up.

## Coordinated Disclosure

After receiving a report, maintainers should aim to:

1. Acknowledge receipt when maintainer availability allows.
2. Triage whether the report is reproducible and security-sensitive.
3. Keep discussion private until a fix, mitigation, or non-issue determination is ready.
4. Credit the reporter in release notes or advisories when appropriate and requested.

## Scope Guidance

Useful security reports may include issues in rsky services, crates, deployment defaults, authentication/authorization behavior, data handling, federation behavior, or moderation/labeling boundaries.

Please avoid destructive testing, social engineering, spam, denial-of-service testing against public infrastructure, or attempts to access data that is not yours. If a reproduction needs more than local or self-controlled resources, ask first in the private report.
