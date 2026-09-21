---
# TRADEMARK RULE: no FedNow mark in `title` or `description`.
title: About
description: Who maintains Pacsmith, why it exists, and what it is not.
---

Pacsmith is an open-source ISO 20022 toolkit for the FedNow® Service: a message library,
a local simulator, a send gateway and client SDKs, all Apache-2.0.

## Why it exists

More than 1,500 institutions are on the network and most of them can only receive. The
asymmetry is not an accident — the receive side can be bought as part of a core banking
package, while the send side means owning ISO 20022 message construction, profile
validation, message signing, 24x7 operation, and the failure mode nobody demos: **you sent
a payment and no response came back**.

That last one is why this project has the shape it does. A sending institution that guesses
wrong about an unanswered payment either loses money or sends it twice. Pacsmith takes the
position that unresolved submissions are reconciled with a status request and **never**
resent blindly, and builds that invariant into the gateway's state machine rather than into
a runbook.

## Who maintains it

Built and maintained by **João Bueno Simonassi** — a payments engineer who ran Pix
point-of-sale middleware in production: instant payments, 24x7, in a market where the
rail's settlement is final and the merchant is standing at the terminal waiting. That is
the same problem shape as an instant-payment send side in the US, and most of the hard-won
lessons transfer directly: idempotency is not optional, timeouts are a state and not an
error, and reconciliation is a product feature rather than an operations chore.

The project is documentation-first on purpose. The
[Integration handbook](/docs/handbook/) is written alongside the code, every claim either
sourced from public documentation or demonstrated by something you can run against the
simulator.

- Source, issues and roadmap: [github.com/joaoabuenosi/fednow-oss](https://github.com/joaoabuenosi/fednow-oss)
- Security reports: see the [security policy](/evaluate/security-policy/) — not a public issue
- Contributions: [CONTRIBUTING.md](https://github.com/joaoabuenosi/fednow-oss/blob/main/CONTRIBUTING.md)

## What this is not

**Pacsmith is an independent open-source project. It is not affiliated with, endorsed by or
sponsored by the Federal Reserve.** "FedNow" is a service mark of the Federal Reserve Banks
and is used on this site only to describe, factually, what the software interoperates with.
No Federal Reserve branding, imagery or colours are used here, and none of the project's
names or assets should be read as suggesting a relationship that does not exist.

Nor is it certified. Nothing in this repository has been approved for production use by any
payment operator, there is no compatibility guarantee, and passing the local simulator or
the conformance vectors is a rehearsal — not a substitute for an operator's own testing
programme. It is early development software, offered under a licence that
[disclaims warranty](/evaluate/licence/).

## The name

A smith works `pacs` messages — pacs.008, pacs.002, pacs.028, pacs.004 — the ISO 20022
family this toolkit spends its life shaping. The name belongs to the project, which is the
point: the brand is ours, and the service mark stays where it belongs, describing what the
software talks to.

## Privacy

This site loads no analytics, sets no cookies, and makes no third-party requests except the
OpenSSF Scorecard badge on the [Evaluate](/evaluate/) page, which is served by OpenSSF. The
software itself has no telemetry and never phones home.
