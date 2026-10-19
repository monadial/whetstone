# payments

A small multi-tenant payments service. Tenants create payments, the service
charges them through an external payment provider, and a payment whose outcome
is unknown can be retried safely.

## Running

```sh
PAYMENTS_PROVIDER_URL=http://localhost:9000 cargo run
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `PAYMENTS_LISTEN_ADDR` | `127.0.0.1:8080` | Address the API listens on |
| `PAYMENTS_DATA_DIR` | `data` | Directory holding the payment log |
| `PAYMENTS_LOG_LEVEL` | `info` | Most detailed log level written to stdout |
| `PAYMENTS_PROVIDER_URL` | required | Base URL of the payment provider |
| `PAYMENTS_PROVIDER_TIMEOUT_MS` | `2000` | Timeout for each attempt of a charge |
| `PAYMENTS_PROVIDER_MAX_ATTEMPTS` | `3` | Attempts per charge |
| `PAYMENTS_PROVIDER_BACKOFF_MS` | `100` | Wait before the first retry; it doubles each time |
| `PAYMENTS_CACHE_PER_TENANT` | `1024` | Payments each tenant keeps in the in-memory cache |

## API

```sh
curl -X POST localhost:8080/v1/tenants/acme/payments \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: order-1042' \
  -d '{"amount_minor": 1250, "currency": "EUR"}'

curl localhost:8080/v1/tenants/acme/payments/pay_...
```

Repeating a `POST` with the same `Idempotency-Key` returns the same payment.
`GET /metrics` exports counters in the Prometheus text format, and
`GET /healthz` answers `ok` while the service is up.

## Tests

`cargo test` runs offline. The API tests talk to a fake provider on
localhost.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
