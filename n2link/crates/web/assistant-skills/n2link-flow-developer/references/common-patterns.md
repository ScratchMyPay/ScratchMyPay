# Common n2link node patterns

Use these properties when the corresponding type appears in the supplied runtime catalog. Preserve an existing configuration-node ID verbatim when referencing it.

## Periodic timestamp

```json
{
  "type": "inject",
  "config": {
    "props": [{"p": "payload"}, {"p": "topic", "vt": "str"}],
    "repeat": "180",
    "crontab": "",
    "once": false,
    "onceDelay": 0.1,
    "topic": "",
    "payload": "",
    "payloadType": "date"
  }
}
```

`repeat` is seconds. Three minutes is `180`.

## Local text split

```json
{
  "type": "ai-split",
  "config": {
    "chunkSize": 512,
    "overlap": 0,
    "separator": "",
    "maxChunks": 256,
    "property": "payload"
  }
}
```

Empty `separator` is character windows. Do not invent a tokenizer.

## Structured JSON

```json
{
  "type": "ai-structured",
  "config": {
    "property": "payload",
    "schemaSource": "node",
    "schema": {"type": "object"},
    "output": "payload"
  }
}
```

Do not use `$ref`, `pattern`, or `allOf`.

## Embeddings

```json
{
  "type": "ai-embed",
  "config": {
    "provider": "EXISTING_AI_PROVIDER_ID",
    "model": "text-embedding-3-small",
    "property": "payload"
  }
}
```

`model` is required. Reuse an existing `ai-provider`. Do not use the chat `defaultModel`.

## MQTT publish and subscribe

```json
{
  "type": "mqtt out",
  "config": {
    "topic": "n2link/timestamp",
    "qos": "",
    "retain": "",
    "respTopic": "",
    "contentType": "",
    "userProps": "",
    "correl": "",
    "expiry": "",
    "broker": "EXISTING_MQTT_BROKER_ID"
  }
}
```

```json
{
  "type": "mqtt in",
  "config": {
    "topic": "n2link/timestamp",
    "qos": "1",
    "datatype": "auto-detect",
    "broker": "EXISTING_MQTT_BROKER_ID",
    "nl": false,
    "rap": true,
    "rh": 0,
    "inputs": 0
  }
}
```

Use the same topic and broker ID for a loopback publish/subscribe flow. If no MQTT broker configuration exists, return a warning and do not invent one or ask for its password in flow JSON.

## Convert a received timestamp to CSV

Use a `change` node to make an object before the CSV node:

```json
{
  "type": "change",
  "config": {
    "rules": [
      {"t": "set", "p": "payload", "pt": "msg", "to": "{\"timestamp\": payload}", "tot": "jsonata"}
    ]
  }
}
```

Then encode and append:

```json
{
  "type": "csv",
  "config": {
    "spec": "rfc",
    "sep": ",",
    "hdrin": false,
    "hdrout": "once",
    "multi": "one",
    "ret": "\\r\\n",
    "temp": "timestamp",
    "skip": "0",
    "strings": true,
    "include_empty_strings": false,
    "include_null_values": false
  }
}
```

```json
{
  "type": "file",
  "config": {
    "filename": "data/timestamps.csv",
    "filenameType": "str",
    "appendNewline": false,
    "createDir": true,
    "overwriteFile": "false",
    "encoding": "none"
  }
}
```

The CSV node already emits a record terminator, so `appendNewline` is false.

## RGB payment request shown as a QR code

Use when the user wants a payment request, an invoice, or a QR code to get paid in an RGB asset.
The `rgb-invoice` node uses the crew (recipient) wallet; leave `wallet` out and it is filled
automatically. Bind the endpoint to this computer with a free port from 1895 to 1899.

```json
{"version": 1, "summary": "Web page that creates an RGB payment request and shows it as a QR code",
 "nodes": [
  {"ref": "page_in", "type": "http in", "name": "Pay me", "x": 140, "y": 80,
   "config": {"url": "/pay-me", "method": "get", "host": "127.0.0.1", "port": 1895, "upload": false}},
  {"ref": "invoice", "type": "rgb-invoice", "name": "Payment request", "x": 340, "y": 80,
   "config": {"assetTicker": "TUSDT"}},
  {"ref": "qr", "type": "qr-code", "name": "QR", "x": 530, "y": 80,
   "config": {"format": "html", "caption": "Scan to pay"}},
  {"ref": "page_out", "type": "http response", "name": "Show page", "x": 700, "y": 80, "config": {}}
 ],
 "wires": [
  {"from": "page_in", "output": 0, "to": "invoice"},
  {"from": "invoice", "output": 0, "to": "qr"},
  {"from": "qr", "output": 0, "to": "page_out"}
 ]}
```

Open `http://127.0.0.1:1895/pay-me` to see it. `qr-code` encodes `msg.payload` when it is text,
otherwise the invoice from an `rgb-invoice` result. Payouts (sending money) are not drafted here:
they are prepared and approved in the PowerEdge app.

## Pre-show treasury check (Tether WDK)

Use when the user wants to know whether the treasury can cover tonight's payouts, or wants
a daily money check. `wdk-balance` then `wdk-fee-quote` gives `msg.payload.coverage.enough`
(true/false). Leave `wallet` out; the "Treasury (Tether WDK)" node is filled automatically.

```json
{"version": 1, "summary": "Every day at 17:00, check whether the WDK treasury covers tonight's 5 payouts",
 "nodes": [
  {"ref": "tick", "type": "inject", "name": "17:00 check", "x": 140, "y": 80,
   "config": {"props": [{"p": "payload"}], "repeat": "", "crontab": "00 17 * * *", "once": false, "payload": "", "payloadType": "date"}},
  {"ref": "balance", "type": "wdk-balance", "name": "Treasury balance", "x": 330, "y": 80, "config": {}},
  {"ref": "cost", "type": "wdk-fee-quote", "name": "Cost of tonight", "x": 520, "y": 80, "config": {"payoutsTonight": 5, "speed": "normal"}},
  {"ref": "enough", "type": "switch", "name": "Enough?", "x": 700, "y": 80,
   "config": {"property": "payload.coverage.enough", "propertyType": "msg", "rules": [{"t": "true"}, {"t": "false"}], "checkall": "true", "repair": false, "outputs": 2}},
  {"ref": "ok", "type": "debug", "name": "Treasury OK", "x": 890, "y": 60, "config": {"active": true, "tosidebar": true, "console": false, "complete": "payload.coverage"}},
  {"ref": "low", "type": "debug", "name": "TOP UP the treasury", "x": 890, "y": 110, "config": {"active": true, "tosidebar": true, "console": true, "complete": "payload.coverage"}}
 ],
 "wires": [
  {"from": "tick", "output": 0, "to": "balance"},
  {"from": "balance", "output": 0, "to": "cost"},
  {"from": "cost", "output": 0, "to": "enough"},
  {"from": "enough", "output": 0, "to": "ok"},
  {"from": "enough", "output": 1, "to": "low"}
 ]}
```

`coverage` has `haveSat`, `needSat`, `payoutsCovered`, `shortSat`. These WDK nodes are read-only.
