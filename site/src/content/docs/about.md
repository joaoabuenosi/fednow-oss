---
# TRADEMARK RULE: no FedNow mark in `title` or `description`.
title: About
description: Who maintains Pacsmith, why it exists, and what it is not.
---

Pacsmith is an open-source ISO 20022 toolkit for the FedNow® Service: a message library,
a local simulator, a send gateway and client SDKs, all Apache-2.0.

## Why it exists

[1,725 banks and credit unions — 1,155 banks and 570 credit unions](https://www.richmondfed.org/publications/research/economic_brief/2026/eb_26-28) had joined the
network by the first quarter of 2026. How many of them can *send* is not a published figure:
the Federal Reserve does not release it, and [a Fed spokesperson declined to comment](https://www.paymentsdive.com/news/fednow-advances-over-hurdles/825071/)
when asked; what is reported is that many institutions signed up only to receive. The
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

## Talk to the maintainer

Until now the only contact address on this site was the one for reporting a vulnerability,
which is a poor door to knock on if you simply have a question. So, explicitly: if you are
at a bank or a credit union, the following are welcome, and they are what decides what gets
built next.

- **Evaluation questions** — the supply chain, the licence, the disclosure process, or what
  is genuinely implemented today versus what is still on the roadmap. The
  [Evaluate](/evaluate/) page is the written answer; anything it does not cover, ask.
- **Pilots** — standing the gateway and the simulator up against your own environment, and
  working out what would have to be true before any of it went near production.
- **Design partners** — the simulator is being packaged as a certification-readiness kit
  ([#79](https://github.com/joaoabuenosi/fednow-oss/issues/79)), a local rehearsal of the
  Customer Testing Program's scenarios before you book operator time. Institutions that
  will actually run those scenarios get to shape it.

Two channels, both already in use:

- **[GitHub Discussions](https://github.com/joaoabuenosi/fednow-oss/discussions)** — public,
  and the better default: someone else is usually asking the same thing.
- **Email** — [joaobuenosi@gmail.com](mailto:joaobuenosi@gmail.com), the address in the
  [security policy](/evaluate/security-policy/), for anything that should not be public.

No support contract sits behind either of them, and there is no vendor relationship on
offer: one maintainer answers as soon as they reasonably can. The 72-hour acknowledgement
in the security policy is a commitment about **vulnerability reports** specifically — and
those still go through the disclosure process, not through the two channels above.

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

This site counts page views with Vercel Web Analytics — privacy-friendly and cookieless. It
stores no personal data and no IP addresses, writes nothing to your device to do it, and
therefore needs no consent banner. It is first-party: both the script and the endpoint it
reports to are served from this domain under `/_vercel/insights/`, never from a third-party
host. The only third-party request this site makes is the OpenSSF Scorecard badge on the
[Evaluate](/evaluate/) page, which is served by OpenSSF.

**This site sets no cookies.** Not for analytics, not for anything else.

It does keep exactly two preferences in your own browser, and they are worth naming rather
than hiding behind "no tracking":

- `starlight-theme`, in `localStorage` — whether you chose the light or the dark theme.
- `sl-sidebar-state`, in `sessionStorage` — which sidebar sections you left open, and
  where you had scrolled to.

Both are written by the site's documentation theme so a page looks the way you left it.
Neither is a cookie, neither is ever sent anywhere, and neither identifies you. If a third
key ever appears, the build fails until this list is updated — `npm run check` holds the
page and the code to each other.

For completeness rather than comfort: the site is hosted by Vercel, so Vercel already
serves every request to it. The page-view count adds a number, not a new party.

The software itself is a separate matter and unchanged — it has no telemetry and never
phones home. Nothing you run reports back to this project.
