"""GatewayClient: a thin, dependency-free client for fednow-gateway.

Design notes, mirroring the gateway's own rules:

- The idempotency key is a **required positional argument** of ``submit`` —
  the gateway rejects submissions without one, and retrying with the same
  key is always safe (the settled payment comes back; nothing touches the
  wire twice).
- Amounts are **integer cents**. There is no float anywhere in this module.
- ``SETTLED`` and ``REJECTED`` are the only final states.
  ``TIMEOUT_UNRESOLVED`` is a work item the gateway's reconciler resolves
  via pacs.028 — ``wait_final`` keeps waiting through it.
"""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Optional

#: States that no advice can change anymore.
FINAL_STATES = frozenset({"SETTLED", "REJECTED"})


class GatewayError(Exception):
    """Base class for gateway-reported errors."""


class ProfileViolation(GatewayError):
    """The payment fails the FedNow Release 1 profile (HTTP 422).

    ``codes`` carries the gateway's stable rule identifiers, e.g.
    ``["fednow.ctgypurp.known"]`` — every violation at once, not just the
    first.
    """

    def __init__(self, codes: list[str]):
        super().__init__(f"FedNow profile violation: {', '.join(codes)}")
        self.codes = codes


class UnknownPayment(GatewayError, KeyError):
    """No payment exists under this idempotency key (HTTP 404)."""


@dataclass(frozen=True)
class Payment:
    """The gateway's view of one payment."""

    idempotency_key: str
    state: str
    message_identification: str
    end_to_end_identification: str
    uetr: Optional[str]
    queries_sent: int
    rejection_reason: Optional[str]
    events: int

    @property
    def is_final(self) -> bool:
        return self.state in FINAL_STATES

    @staticmethod
    def _from_json(data: dict) -> "Payment":
        return Payment(
            idempotency_key=data["idempotency_key"],
            state=data["state"],
            message_identification=data["message_identification"],
            end_to_end_identification=data["end_to_end_identification"],
            uetr=data.get("uetr"),
            queries_sent=data.get("queries_sent", 0),
            rejection_reason=data.get("rejection_reason"),
            events=data.get("events", 0),
        )


class GatewayClient:
    """Client for one fednow-gateway instance.

    >>> gw = GatewayClient("http://localhost:8090")
    >>> p = gw.submit("order-1", reference="ORDER0001", amount_cents=125000,
    ...               debtor_name="Jane", debtor_account="123456789012",
    ...               creditor_name="John", creditor_account="987654321000",
    ...               creditor_agent_routing_number="091000019")
    >>> p = gw.wait_final("order-1")
    >>> p.state
    'SETTLED'
    """

    def __init__(self, base_url: str, timeout: float = 10.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    # -- API calls ---------------------------------------------------------

    def submit(
        self,
        idempotency_key: str,
        *,
        reference: str,
        amount_cents: int,
        debtor_name: str,
        debtor_account: str,
        creditor_name: str,
        creditor_account: str,
        creditor_agent_routing_number: str,
        category_purpose: str = "CONS",
        end_to_end_identification: Optional[str] = None,
        uetr: Optional[str] = None,
    ) -> Payment:
        """Submit a payment, idempotently.

        Resubmitting the same ``idempotency_key`` returns the payment as it
        stands without sending anything again — safe to call from retry
        loops.

        Raises :class:`ProfileViolation` when the payment fails FedNow
        Release 1 validation (nothing reached the wire).
        """
        if not isinstance(amount_cents, int) or isinstance(amount_cents, bool):
            raise TypeError("amount_cents must be an int (cents, never a float)")
        body = {
            "reference": reference,
            "amount_cents": amount_cents,
            "debtor_name": debtor_name,
            "debtor_account": debtor_account,
            "creditor_name": creditor_name,
            "creditor_account": creditor_account,
            "creditor_agent_routing_number": creditor_agent_routing_number,
            "category_purpose": category_purpose,
        }
        if end_to_end_identification is not None:
            body["end_to_end_identification"] = end_to_end_identification
        if uetr is not None:
            body["uetr"] = uetr
        data = self._request(
            "POST",
            "/payments",
            body=body,
            headers={"Idempotency-Key": idempotency_key},
        )
        return Payment._from_json(data)

    def get(self, idempotency_key: str) -> Payment:
        """Current state of a payment. Raises :class:`UnknownPayment`."""
        data = self._request("GET", f"/payments/{idempotency_key}")
        return Payment._from_json(data)

    def reconcile(self, idempotency_key: str) -> Payment:
        """Drive one reconciliation pass now (the gateway also does this on
        its background sweeper — calling it is never required)."""
        data = self._request("POST", f"/payments/{idempotency_key}/reconcile")
        return Payment._from_json(data)

    def healthy(self) -> bool:
        try:
            with urllib.request.urlopen(
                f"{self.base_url}/healthz", timeout=self.timeout
            ) as resp:
                return resp.status == 200
        except (urllib.error.URLError, OSError):
            return False

    # -- Conveniences ------------------------------------------------------

    def wait_final(
        self,
        idempotency_key: str,
        timeout: float = 120.0,
        poll_interval: float = 1.0,
    ) -> Payment:
        """Poll until the payment reaches ``SETTLED`` or ``REJECTED``.

        ``TIMEOUT_UNRESOLVED`` is *not* final: the gateway's reconciler is
        resolving it via pacs.028, so this keeps waiting. Raises
        :class:`TimeoutError` with the last-seen payment attached
        (``exc.payment``) when ``timeout`` elapses first.
        """
        deadline = time.monotonic() + timeout
        payment = self.get(idempotency_key)
        while not payment.is_final:
            if time.monotonic() >= deadline:
                exc = TimeoutError(
                    f"payment '{idempotency_key}' still {payment.state} "
                    f"after {timeout:.0f}s"
                )
                exc.payment = payment  # type: ignore[attr-defined]
                raise exc
            time.sleep(poll_interval)
            payment = self.get(idempotency_key)
        return payment

    # -- Plumbing ----------------------------------------------------------

    def _request(
        self,
        method: str,
        path: str,
        body: Optional[dict] = None,
        headers: Optional[dict] = None,
    ) -> dict:
        req = urllib.request.Request(
            f"{self.base_url}{path}",
            method=method,
            data=json.dumps(body).encode() if body is not None else None,
        )
        req.add_header("content-type", "application/json")
        for name, value in (headers or {}).items():
            req.add_header(name, value)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                return json.loads(resp.read().decode())
        except urllib.error.HTTPError as e:
            raise self._map_error(e) from None

    @staticmethod
    def _map_error(e: urllib.error.HTTPError) -> GatewayError:
        detail = e.read().decode(errors="replace")
        if e.code == 404:
            return UnknownPayment(detail or "unknown payment")
        if e.code == 422:
            try:
                codes = json.loads(detail).get("codes", [])
            except (json.JSONDecodeError, AttributeError):
                codes = []
            return ProfileViolation(codes)
        return GatewayError(f"HTTP {e.code}: {detail}")
