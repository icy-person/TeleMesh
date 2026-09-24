# TeleMesh

Rust Telegram MTProto gateway with a Linux client.

## Architecture

Telegram <-> **TeleMesh Server** <-> **Linux Client**

Only the server stores the Telegram MTProto session. The Linux client talks to the server using an authenticated HTTP API and an event WebSocket.

## Build

```bash
cp .env.example .env
# edit TELEGRAM_API_ID, TELEGRAM_API_HASH and TELEMESH_TOKEN
cargo check
cargo build --workspace --release
```

## First Telegram login

Run the server:

```bash
cargo run -p telemesh-server
```

From a trusted machine:

```bash
curl -X POST http://127.0.0.1:8787/api/v1/auth/start \
  -H 'content-type: application/json' \
  -H 'x-telemesh-token: YOUR_TOKEN' \
  -d '{"phone":"+123456789"}'
```

Then:

```bash
curl -X POST http://127.0.0.1:8787/api/v1/auth/complete \
  -H 'content-type: application/json' \
  -H 'x-telemesh-token: YOUR_TOKEN' \
  -d '{"code":"12345"}'
```

If Telegram 2FA is enabled, call `/api/v1/auth/password`.

## Linux client

```bash
cargo run -p telemesh-client -- --token YOUR_TOKEN health
cargo run -p telemesh-client -- --token YOUR_TOKEN me
cargo run -p telemesh-client -- --token YOUR_TOKEN dialogs
cargo run -p telemesh-client -- --token YOUR_TOKEN send username "hello from TeleMesh"
cargo run -p telemesh-client -- --token YOUR_TOKEN events
```

## API

- GET `/health`
- GET `/api/v1/me`
- GET `/api/v1/dialogs`
- POST `/api/v1/messages/send`
- POST `/api/v1/auth/start`
- POST `/api/v1/auth/complete`
- POST `/api/v1/auth/password`
- GET `/api/v1/events`

## Security

Keep `TELEMESH_TOKEN` long and random. Do not expose the service directly to the public Internet without TLS/VPN or another protected transport. The Telegram session database is equivalent to an authenticated Telegram session and must be protected.

## Planned

- message history and search
- media upload/download
- per-client identities and permissions
- short-lived WS tickets
- TLS/reverse-proxy deployment
- TypeScript desktop UI
- local AI agent integration
