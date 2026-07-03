"""Unit tests: GatewayClient against a stdlib stub server (no gateway needed)."""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from fednow_client import GatewayClient, Payment, ProfileViolation, UnknownPayment

PAYMENT = {
    "idempotency_key": "k1",
    "state": "ACK_PENDING",
    "message_identification": "20260703021040078QS0001",
    "end_to_end_identification": "QS0001",
    "uetr": None,
    "queries_sent": 0,
    "rejection_reason": None,
    "events": 4,
}


class StubHandler(BaseHTTPRequestHandler):
    """Emulates the gateway's REST surface for the paths the tests hit."""

    calls = []

    def _send(self, status, payload, content_type="application/json"):
        body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers.get("content-length", 0))
        body = json.loads(self.rfile.read(length)) if length else None
        StubHandler.calls.append(("POST", self.path, dict(self.headers), body))

        if self.path == "/payments":
            if not self.headers.get("Idempotency-Key"):
                self._send(400, b"the Idempotency-Key header is mandatory", "text/plain")
                return
            if body.get("category_purpose") == "WRONG":
                self._send(
                    422,
                    {"error": "fednow_profile_violation", "codes": ["fednow.ctgypurp.known"]},
                )
                return
            self._send(200, PAYMENT)
        elif self.path.endswith("/reconcile"):
            self._send(200, {**PAYMENT, "state": "SETTLED", "queries_sent": 1, "events": 7})
        else:
            self._send(404, b"unknown payment", "text/plain")

    def do_GET(self):
        StubHandler.calls.append(("GET", self.path, dict(self.headers), None))
        if self.path == "/payments/k1":
            # First poll pending, then settled — exercises wait_final.
            gets = sum(1 for c in StubHandler.calls if c[:2] == ("GET", "/payments/k1"))
            state = "ACK_PENDING" if gets < 2 else "SETTLED"
            self._send(200, {**PAYMENT, "state": state, "events": 4 if gets < 2 else 5})
        elif self.path == "/healthz":
            self._send(200, b"ok", "text/plain")
        else:
            self._send(404, b"unknown payment", "text/plain")

    def log_message(self, *args):  # keep pytest output clean
        pass


@pytest.fixture()
def gateway_stub():
    StubHandler.calls = []
    server = ThreadingHTTPServer(("127.0.0.1", 0), StubHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_address[1]}"
    server.shutdown()


def submit(client, key="k1", **overrides):
    kwargs = dict(
        reference="QS0001",
        amount_cents=125000,
        debtor_name="Jane",
        debtor_account="123456789012",
        creditor_name="John",
        creditor_account="987654321000",
        creditor_agent_routing_number="091000019",
    )
    kwargs.update(overrides)
    return client.submit(key, **kwargs)


def test_submit_parses_payment_and_sends_key_header(gateway_stub):
    client = GatewayClient(gateway_stub)
    p = submit(client)
    assert isinstance(p, Payment)
    assert p.state == "ACK_PENDING"
    assert not p.is_final
    method, path, headers, body = StubHandler.calls[0]
    assert (method, path) == ("POST", "/payments")
    assert headers.get("Idempotency-Key") == "k1"
    assert body["amount_cents"] == 125000
    assert "end_to_end_identification" not in body  # omitted when None


def test_profile_violation_raises_with_codes(gateway_stub):
    client = GatewayClient(gateway_stub)
    with pytest.raises(ProfileViolation) as exc:
        submit(client, category_purpose="WRONG")
    assert exc.value.codes == ["fednow.ctgypurp.known"]


def test_unknown_payment_raises(gateway_stub):
    client = GatewayClient(gateway_stub)
    with pytest.raises(UnknownPayment):
        client.get("nope")


def test_wait_final_polls_through_pending(gateway_stub):
    client = GatewayClient(gateway_stub)
    p = client.wait_final("k1", timeout=10, poll_interval=0.01)
    assert p.state == "SETTLED"
    assert p.is_final


def test_wait_final_timeout_carries_last_payment():
    # A stub that always answers pending.
    class Pending(StubHandler):
        def do_GET(self):
            self._send(200, PAYMENT)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Pending)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        client = GatewayClient(f"http://127.0.0.1:{server.server_address[1]}")
        with pytest.raises(TimeoutError) as exc:
            client.wait_final("k1", timeout=0.05, poll_interval=0.01)
        assert exc.value.payment.state == "ACK_PENDING"
    finally:
        server.shutdown()


def test_amount_must_be_int(gateway_stub):
    client = GatewayClient(gateway_stub)
    with pytest.raises(TypeError):
        submit(client, amount_cents=1250.00)


def test_healthy(gateway_stub):
    assert GatewayClient(gateway_stub).healthy()
    assert not GatewayClient("http://127.0.0.1:1").healthy()
