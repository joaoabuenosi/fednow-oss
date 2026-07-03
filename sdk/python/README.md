# fednow-gateway-client (Python)

Thin Python client for the [fednow-gateway](../../gateway/) REST API —
**stdlib only, zero runtime dependencies**.

```python
from fednow_client import GatewayClient, ProfileViolation

gw = GatewayClient("http://localhost:8090")

payment = gw.submit(
    "order-2026-0001",                 # idempotency key — mandatory by design
    reference="ORDER0001",
    amount_cents=125_000,              # integer cents, never floats
    debtor_name="Jane Example",   debtor_account="123456789012",
    creditor_name="John Example", creditor_account="987654321000",
    creditor_agent_routing_number="091000019",
)
print(payment.state)                   # ACK_PENDING — the answer is async

final = gw.wait_final("order-2026-0001")
print(final.state)                     # SETTLED (or REJECTED + rejection_reason)
```

The client mirrors the gateway's operating rules instead of hiding them:

- **Idempotency first.** The key is the first positional argument of
  `submit`; calling it again with the same key returns the payment as it
  stands — safe inside any retry loop.
- **`wait_final` understands the timeout case.** `TIMEOUT_UNRESOLVED` is not
  final: the gateway's reconciler is resolving it with a pacs.028 status
  request (never a resend), so the client keeps polling through it.
- **Profile violations are exceptions with rule codes.**
  `ProfileViolation.codes` carries the gateway's stable identifiers
  (`fednow.ctgypurp.known`, `fednow.aba.checksum`, …) — every violation at
  once, before anything reaches the wire.

## Install / test

```sh
pip install ./sdk/python              # or: pip install -e ./sdk/python
pytest sdk/python                     # unit tests (stub server, no gateway)

# integration against the live stack (see QUICKSTART.md at the repo root):
FEDNOW_GW_URL=http://localhost:8090 pytest sdk/python
```

The integration tests run in CI against a real gateway↔simulator pair in
MQ mode on every commit.
