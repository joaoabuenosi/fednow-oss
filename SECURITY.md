# Security Policy

This project handles payment-message construction and signing; we take vulnerability
reports seriously and appreciate coordinated disclosure.

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Preferred channel: **GitHub private vulnerability reporting** — use the
"Report a vulnerability" button under the repository's *Security* tab.

If you cannot use GitHub, email **joaobuenosi@gmail.com** with subject
`[SECURITY] fednow-oss`.

What to include: affected crate and version/commit, reproduction steps or PoC,
and impact assessment if you have one.

## What to expect

- Acknowledgement within **72 hours**.
- A triage verdict (accepted / not a vulnerability / needs info) within **7 days**.
- Fix timeline agreed case by case; critical issues in released code get priority.
- Credit in the release notes and a CVE requested via GitHub Security Advisories when
  applicable, unless you prefer to stay anonymous.

## Scope notes

- This project never ships with credentials, certificates or live endpoints; reports
  about the *absence* of those are out of scope.
- The simulator (`fednow-sim`) is a development tool and is not hardened for exposure
  to untrusted networks; running it on the public internet is out of scope.

## Supply chain

Releases are signed and ship with a changelog and an SBOM. CI builds are public.
