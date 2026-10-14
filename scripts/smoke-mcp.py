#!/usr/bin/env python3
"""Exercise the deployed MCP protocol and every read tool without printing finances."""
import json
from decimal import Decimal
import sys
import urllib.error
import urllib.request

URL = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8091/mcp"
request_id = 0


def rpc(method, params=None, expected_error=False):
    global request_id
    request_id += 1
    payload = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(URL, data=json.dumps(payload).encode(), headers={
        "Content-Type": "application/json", "Accept": "application/json, text/event-stream",
        "MCP-Protocol-Version": "2025-03-26"})
    with urllib.request.urlopen(request, timeout=60) as response:
        body = json.load(response)
    if expected_error:
        if expected_error == "protocol":
            assert "error" in body and "result" not in body, method
        else:
            assert "error" not in body, f"{method}: tool failure returned a protocol error"
            assert body["result"].get("isError") is True, method
            assert body["result"]["content"][0]["text"], method
        return body
    assert "error" not in body, f"{method}: {body.get('error')}"
    result = body["result"]
    assert not result.get("isError"), f"{method}: tool error"
    return result


def call(name, arguments=None, expected_error=False):
    result = rpc("tools/call", {"name": name, "arguments": arguments or {}}, expected_error)
    if expected_error:
        return result
    structured = result.get("structuredContent")
    if structured is None:
        structured = json.loads(result["content"][0]["text"])
    assert "meta" in structured and "data" in structured, name
    text = json.loads(result["content"][0]["text"])
    assert text == structured, f"{name}: text and structured data differ"
    check_money(structured["data"])
    return structured["data"]


def check_money(value):
    if isinstance(value, list):
        for row in value:
            check_money(row)
    elif isinstance(value, dict):
        for key, item in value.items():
            if key in {"raw", "payload"}:
                continue
            if key in {"income", "expense", "savings", "signed_amount_total", "amount", "credited",
                       "balance", "signed_balance", "value", "net_worth", "total_value", "cost_basis",
                       "monthly_amount", "shares_value", "exercisable_value", "cost_to_exercise"} and isinstance(item, float):
                decimal = Decimal(str(item))
                assert decimal == decimal.quantize(Decimal("0.01")), f"{key}: amount not rounded to cents"
            check_money(item)


info = rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
                          "clientInfo": {"name": "nestegg-smoke", "version": "1"}})
assert info["serverInfo"]["name"] == "nestegg"
tools = rpc("tools/list")["tools"]
expected = {"get_sync_status", "get_overview", "get_accounts", "search_transactions", "get_cash_flow",
            "get_history", "get_document", "get_investments", "get_recurring", "get_equity_summary",
            "get_equity", "get_record_detail", "get_categories"}
assert {t["name"] for t in tools} == expected
assert all(t["annotations"]["readOnlyHint"] for t in tools)
status = call("get_sync_status")
call("get_overview")
accounts = call("get_accounts", {"limit": 2})
transactions = call("search_transactions", {"limit": 2})
if transactions["next_offset"] is not None:
    following = call("search_transactions", {"limit": 2, "offset": transactions["next_offset"]})
    assert not ({t["id"] for t in transactions["items"]} & {t["id"] for t in following["items"]})
if transactions["items"]:
    t = transactions["items"][0]
    detail = call("get_record_detail", {"kind": "transaction", "id": t["id"]})
    assert detail["record"]["transaction_id"] == t["id"]
    filtered = call("search_transactions", {"start_date": t["date"], "end_date": t["date"], "account_id": t["account_id"]})
    assert all(row["date"] == t["date"] and row["account_id"] == t["account_id"] for row in filtered["items"])
for kind in ["net_worth", "investments", "equity"]:
    call("get_history", {"kind": kind, "limit": 2})
if accounts["items"]:
    account_id = accounts["items"][0]["id"]
    call("get_history", {"kind": "account", "account_id": account_id, "limit": 2})
    call("get_record_detail", {"kind": "account", "id": account_id})
for kind in ["budget", "goals", "credit_score", "household", "household_members", "transaction_rules"]:
    document = call("get_document", {"kind": kind})
    if kind in {d["kind"] for d in status["documents"]}:
        assert document is not None and document["kind"] == kind and "payload" in document
        historical = call("get_document", {"kind": kind, "as_of_date": document["as_of_date"]})
        assert historical == document
for kind in ["holdings", "positions", "tax_lots"]:
    investments = call("get_investments", {"kind": kind, "limit": 2})
    if kind == "positions" and investments["items"]:
        position = investments["items"][0]
        call("get_record_detail", {"kind": "position", "id": position["position_id"]})
        call("get_investments", {"kind": "tax_lots", "position_id": position["position_id"]})
        if position["security_id"]:
            call("get_record_detail", {"kind": "security", "id": position["security_id"]})
call("get_cash_flow", {"start_date": "2026-01-01", "end_date": "2026-12-31"})
call("get_recurring", {"days": 30, "limit": 2})
equity = call("get_equity_summary")
if equity["issuers"]:
    call("get_record_detail", {"kind": "issuer", "id": str(equity["issuers"][0]["id"])})
for kind in ["grants", "certificates", "exercises", "tenders", "vesting", "snapshots"]:
    page = call("get_equity", {"kind": kind, "limit": 2})
    if page["items"] and kind in ["grants", "certificates", "exercises", "tenders"]:
        row = page["items"][0]
        if kind == "grants":
            row = row["grant"]
        key = {"grants": "id", "certificates": "certificate_id", "exercises": "certificate_id", "tenders": "tender_id"}[kind]
        call("get_record_detail", {"kind": kind[:-1], "id": str(row[key])})
call("get_categories")
for name, args in [("get_accounts", {"limit": 501}),
                   ("search_transactions", {"start_date": "2026-02-30"}),
                   ("search_transactions", {"start_date": "2026-10-03", "end_date": "2026-10-01"}),
                   ("get_history", {"kind": "account"}),
                   ("get_recurring", {"days": 999}),
                   ("get_record_detail", {"kind": "transaction", "id": "missing'; SELECT 1; --"}),
                   ("get_accounts", {"sql": "SELECT 1"}),
                   ("get_document", {"kind": "household", "as_of_date": "not-a-date"}),
                   ("get_document", {})]:
    call(name, args, expected_error=True)
call("unknown_tool", expected_error="protocol")
for header in [{"Host": "untrusted.invalid"}, {"Origin": "https://untrusted.invalid"}]:
    request = urllib.request.Request(URL, data=b'{}', headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream", **header})
    try:
        urllib.request.urlopen(request, timeout=10)
        raise AssertionError(f"Untrusted header accepted: {header}")
    except urllib.error.HTTPError as e:
        assert e.code == 403, e.code
print(f"PASS: initialize, {len(tools)} read-only tools, all six document kinds, collections, cent rounding, pagination, filters, tool/protocol errors, Host/Origin rejection ({request_id} RPC calls)")
