"""Python client for the fednow-gateway REST API.

Stdlib only — no runtime dependencies. See the repository QUICKSTART for the
gateway itself: https://github.com/joaoabuenosi/fednow-oss
"""

from .client import (
    FINAL_STATES,
    GatewayClient,
    GatewayError,
    Payment,
    ProfileViolation,
    UnknownPayment,
)

__all__ = [
    "FINAL_STATES",
    "GatewayClient",
    "GatewayError",
    "Payment",
    "ProfileViolation",
    "UnknownPayment",
]
