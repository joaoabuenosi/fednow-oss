"""Integration: the SDK against a live fednow-gateway (+ fednow-sim).

Skipped unless FEDNOW_GW_URL is set. CI launches the stack with
FEDNOW_GW_SWEEP_SECS=1 / FEDNOW_GW_TIMEOUT_SECS=3 / FEDNOW_GW_BACKOFF_SECS=2
so the timeout arc completes in seconds; locally:

    cargo run -p fednow-sim &
    FEDNOW_GW_SOUTHBOUND=mq FEDNOW_GW_SWEEP_SECS=1 \
      FEDNOW_GW_TIMEOUT_SECS=3 FEDNOW_GW_BACKOFF_SECS=2 \
      cargo run -p fednow-gateway &
    FEDNOW_GW_URL=http://localhost:8090 pytest sdk/python
"""

import os
import uuid

import pytest

from fednow_client import GatewayClient, ProfileViolation

GW_URL = os.environ.get("FEDNOW_GW_URL")

pytestmark = pytest.mark.skipif(
    not GW_URL, reason="set FEDNOW_GW_URL to run against a live gateway"
)


@pytest.fixture()
def client():
    c = GatewayClient(GW_URL)
    assert c.healthy(), f"no gateway answering at {GW_URL}"
    return c


def submit(client, key, amount_cents):
    ref = f"SDK{uuid.uuid4().hex[:12].upper()}"
    return client.submit(
        key,
        reference=ref,
        amount_cents=amount_cents,
        debtor_name="Jane Example",
        debtor_account="123456789012",
        creditor_name="John Example",
        creditor_account="987654321000",
        creditor_agent_routing_number="091000019",
    )


def test_settle_and_idempotent_replay(client):
    key = f"sdk-it-{uuid.uuid4()}"
    first = submit(client, key, 125_000)
    settled = client.wait_final(key, timeout=30)
    assert settled.state == "SETTLED"

    # Same key: same payment back, no new events from a second send.
    replay = client.submit(
        key,
        reference=first.end_to_end_identification,
        amount_cents=125_000,
        debtor_name="Jane Example",
        debtor_account="123456789012",
        creditor_name="John Example",
        creditor_account="987654321000",
        creditor_agent_routing_number="091000019",
    )
    assert replay.state == "SETTLED"
    assert replay.events == settled.events


def test_rejection_carries_iso_reason(client):
    key = f"sdk-it-{uuid.uuid4()}"
    submit(client, key, 125_011)  # .11 → receiving bank rejects
    final = client.wait_final(key, timeout=30)
    assert final.state == "REJECTED"
    assert final.rejection_reason == "AC04"


def test_timeout_arc_resolves_without_resend(client):
    key = f"sdk-it-{uuid.uuid4()}"
    submit(client, key, 125_033)  # .33 → no advice until pacs.028 asks
    final = client.wait_final(key, timeout=60)
    assert final.state == "SETTLED"
    assert final.queries_sent >= 1  # resolved by status request, not resend


def test_profile_violation_never_reaches_the_wire(client):
    key = f"sdk-it-{uuid.uuid4()}"
    with pytest.raises(ProfileViolation) as exc:
        client.submit(
            key,
            reference="SDKBAD0001",
            amount_cents=125_000,
            category_purpose="WRONG",
            debtor_name="Jane Example",
            debtor_account="123456789012",
            creditor_name="John Example",
            creditor_account="987654321000",
            creditor_agent_routing_number="091000019",
        )
    assert "fednow.ctgypurp.known" in exc.value.codes
