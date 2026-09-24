# TeleMesh

A Rust Telegram MTProto gateway with a native Linux desktop client.

## Architecture

```text
Telegram
   ↕ MTProto
TeleMesh Server (Rust)
   ├─ persistent Telegram session
   ├─ REST API
   ├─ one-time WebSocket tickets
   └─ realtime Telegram updates
          ↕ HTTP / WebSocket
Tauri 2 + React + TypeScript (Linux)
```

The Telegram MTProto session and Telegram credentials stay on the server. Linux clients only receive the TeleMesh token and never need direct Telegram access.

## Build the server

```bash
cp .env.example .env
# edit TELEGRAM_API_ID, TELEGRAM_API_HASH and TELEMESH_TOKEN
cargo check
cargo build --workspace --release
```

Run:

```bash
cargo run -p telemesh-server
```

## Telegram login

The Linux UI can perform the complete Telegram login flow: phone number → login code → optional 2FA password.

The API can also be used directly:

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

If Telegram 2FA is enabled:

```bash
curl -X POST http://127.0.0.1:8787/api/v1/auth/password \
  -H 'content-type: application/json' \
  -H 'x-telemesh-token: YOUR_TOKEN' \
  -d '{"password":"YOUR_2FA_PASSWORD"}'
```

## CLI client

```bash
cargo run -p telemesh-client -- --token YOUR_TOKEN health
cargo run -p telemesh-client -- --token YOUR_TOKEN me
cargo run -p telemesh-client -- --token YOUR_TOKEN dialogs
cargo run -p telemesh-client -- --token YOUR_TOKEN history username --limit 50
cargo run -p telemesh-client -- --token YOUR_TOKEN send username "hello from TeleMesh"
cargo run -p telemesh-client -- --token YOUR_TOKEN events
```

## Linux desktop client

Requirements: Node.js/npm and the Tauri 2 Linux build dependencies.

```bash
cd ui
npm install
npm run tauri dev
```

Production package:

```bash
npm run tauri build
```

The UI currently provides:

- server/token setup
- Telegram phone/code/2FA authorization
- account information
- chat list and local chat filtering
- real Telegram message history
- older-message pagination
- message sending and replies
- edit/delete/forward/reactions
- message and global chat search
- read state and unread badges
- media upload
- native Linux notifications
- realtime new-message events
- responsive desktop/mobile layout

## HTTP API

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/health` | Server and Telegram authorization state |
| GET | `/api/v1/me` | Current Telegram account |
| GET | `/api/v1/dialogs` | Dialog list |
| GET | `/api/v1/messages?peer=...&limit=...&offset_id=...` | Message history |
| POST | `/api/v1/messages/send` | Send a text/reply message |
| POST | `/api/v1/messages/media` | Upload and send media |
| GET | `/api/v1/messages/media/download?peer=...&message_id=...` | Download message media |
| POST | `/api/v1/messages/edit` | Edit a message |
| POST | `/api/v1/messages/delete` | Delete messages |
| POST | `/api/v1/messages/forward` | Forward messages |
| POST | `/api/v1/messages/react` | Add/remove reaction |
| POST | `/api/v1/messages/read` | Mark chat read |
| GET | `/api/v1/messages/search?q=...` | Search messages |
| POST | `/api/v1/auth/start` | Send Telegram login code |
| POST | `/api/v1/auth/complete` | Complete login code |
| POST | `/api/v1/auth/password` | Complete Telegram 2FA |
| POST | `/api/v1/events/ticket` | Issue one-time WebSocket ticket |
| GET | `/api/v1/events?ticket=...` | Realtime events |

All protected endpoints use `x-telemesh-token`.

## Security

Use a long random `TELEMESH_TOKEN`. Do not expose the service directly to the public Internet without TLS/VPN or another protected transport.

The Telegram session database is equivalent to an authenticated Telegram session and must be protected like a credential.

The WebSocket endpoint does not accept the long-lived TeleMesh token directly; clients first obtain a short-lived one-time ticket.

## Next production layer

The remaining work is now mostly Telegram-client completeness rather than the basic gateway:

- media upload/download
- reply/edit/delete/forward/reactions
- global and per-chat search
- unread counts and read state
- typing/presence
- notifications
- per-client identities and permissions
- TLS/reverse-proxy deployment
- encrypted local client configuration
- AI/MCP agent integration
