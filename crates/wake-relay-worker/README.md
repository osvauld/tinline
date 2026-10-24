# wake-relay-worker

The push wake relay as a Cloudflare Worker. Design: `docs/push.md`; all logic is in
`crates/wake-relay-core` (tested natively). This crate is outside the main workspace because it
builds for `wasm32-unknown-unknown`.

## Endpoints

| | |
|---|---|
| `GET /v1/health` | `ok` |
| `POST /v1/register` | device → sealed wake tokens, one per contact tag |
| `POST /v1/wake` | sender → empty push to the device (202, or 410 drop the token, 429/503 retry later) |
| `POST /v1/revoke` | device takes a token back (block, calls turned off) |
| `GET /v1/probe/apns` | only while `PROBE = "1"`: checks that APNs (HTTP/2 only) answers this Worker |

## First deploy (the APNs probe; no Apple or Firebase keys needed)

Run from this directory. Needs Rust with the `wasm32-unknown-unknown` target and Node.

1. `npx wrangler login` (opens the browser; log in to the Osvauld Cloudflare account).
2. `npx wrangler kv namespace create WAKE_KV`, then put the printed `id` in `wrangler.toml`.
3. Relay sealing key (32 random bytes, kid 1):
   `printf '1:%s' "$(head -c 32 /dev/urandom | basenc --base64url | tr -d '=')" | npx wrangler secret put RELAY_KEYS`
   Keep a copy in the password manager: losing it invalidates every wake token.
4. `npx wrangler deploy` (builds with worker-build, prints the `*.workers.dev` URL).
5. `curl https://tinline-wake.<account>.workers.dev/v1/probe/apns`
   - `apns answered 403 {"reason":"InvalidProviderToken"}` (or another APNs reason): HTTP/2 to
     Apple works from Workers. Set `PROBE = "0"` and redeploy.
   - `no answer: …`: it does not; the fallback host is Fly.io Mumbai (docs/push.md).

## Provider secrets (later)

- FCM: `npx wrangler secret put FCM_SERVICE_ACCOUNT < wake-relay.json` (the `wake-relay`
  service account key; only the FCM API Admin role).
- APNs (after the Apple membership renewal): `APNS_KEY_P8` (`< AuthKey_XXXX.p8`),
  `APNS_KEY_ID`, `APNS_TEAM_ID`.
- Key rotation: add a new key first in `RELAY_KEYS` (`2:new,1:old`); drop the old one 31 days
  later, when every token sealed with it has expired.

Never commit these files; `.dev.vars` (local secrets for `wrangler dev`) is git-ignored.
Note: `wrangler dev` cannot reach APNs (the local runtime has no HTTP/2; workerd #4841); test
APNs only on the deployed Worker.
