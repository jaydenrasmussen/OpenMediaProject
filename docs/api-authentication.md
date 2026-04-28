# Authentication Guide

OpenMediaProject supports two parallel authentication mechanisms: **API key auth** for programmatic / script access and **JWT session auth** for browser-based use. Both layers are active on every request when authentication is enabled.

---

## Auth Layers at a Glance

| Layer | Credential | Use case |
|---|---|---|
| API Key | `X-API-Key` header (or `?api_key=` query param) | Scripts, curl, integrations |
| JWT Session | `Authorization: Bearer <token>` header | Browser / SvelteKit frontend |

Both layers run in sequence on every request. Passing either one is sufficient for most endpoints. Admin-only endpoints (`/wanted/:id/approve`, `/wanted/:id/reject`) additionally require a JWT with `role=admin`.

---

## API Key Authentication

### Setup

Enable authentication and set (or auto-generate) a key:

```env
ENABLE_AUTH=true
API_KEY=              # leave blank to auto-generate
ALLOW_API_KEY_QUERY=false
SKIP_AUTH_ENDPOINTS=/api/health,/api/v1/health
```

When `API_KEY` is blank, the server generates a 26-character random key at startup, saves it to `.apikey`, and logs it:

```
Generated new API key and saved to .apikey file  api_key=EYEisGgbdborLJ0z2xaJn7KRZM
```

Save this value in your environment for future restarts. On subsequent starts the key is loaded from `.apikey` automatically.

### Sending the API Key

**HTTP header (recommended):**
```bash
curl -H "X-API-Key: EYEisGgbdborLJ0z2xaJn7KRZM" http://localhost:8080/api/v1/movies
```

**Query parameter** (requires `ALLOW_API_KEY_QUERY=true`):
```bash
curl "http://localhost:8080/api/v1/movies?api_key=EYEisGgbdborLJ0z2xaJn7KRZM"
```

Query parameters are less secure — keys can appear in server logs and browser history. Use headers in production.

### Key Management Endpoints

| Endpoint | Method | Description |
|---|---|---|
| `/api/v1/auth/key` | GET | Read the current API key |
| `/api/v1/auth/key` | POST | Generate and store a new API key |
| `/api/v1/auth/validate` | GET | Validate the provided key, returns 200 or 401 |

---

## JWT Session Authentication

JWT auth is designed for the browser-based UI. Users log in with a username and password and receive a short-lived signed token.

### Admin Bootstrap

On every startup, if no user with `role=admin` exists in the database, one is created automatically:

- **Username:** `admin`
- **Password:** value of `ADMIN_PASSWORD` env var, or a random 24-character hex string if the env var is not set

If the password was generated randomly, it is printed once at startup:
```
WARN  Created initial admin user — change this password immediately  username=admin  password=a3f9c2...
```

**Change this password immediately** by updating `ADMIN_PASSWORD` and restarting, or by using the user management API.

### Login

```
POST /api/v1/auth/login
Content-Type: application/json

{
  "username": "admin",
  "password": "yourpassword"
}
```

**Response:**
```json
{
  "token": "eyJhbGci...",
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "admin",
  "role": "admin"
}
```

The token is a signed HS256 JWT, valid for 24 hours. Include it on subsequent requests:

```bash
curl -H "Authorization: Bearer eyJhbGci..." http://localhost:8080/api/v1/history
```

**JWT secret:** the signing key is loaded from `JWT_SECRET` env var. If absent, it falls back to the `.apikey` file content, and finally to a random per-process value (sessions are lost on restart in this case). Set `JWT_SECRET` to a stable secret in production.

### Register

```
POST /api/v1/auth/register
Content-Type: application/json

{
  "username": "alice",
  "email": "alice@example.com",
  "password": "hunter2",
  "first_name": "Alice",
  "last_name": "Smith"
}
```

All new registrations receive `role=user`. Role elevation to `admin` requires direct database access or a future admin-only upgrade endpoint.

### Token Expiry and Refresh

Tokens expire after 24 hours. There is no refresh endpoint yet — clients must call `/auth/login` again. The middleware rejects expired tokens with `401 Unauthorized`.

### Role-Based Access

Two middleware guards are available and composed per-route:

| Middleware | Behaviour |
|---|---|
| `JWTMiddleware` | Validates Bearer token if present; lenient (passes requests with no token) |
| `RequireRole("admin")` | Rejects with 403 if the JWT `role` claim is not `"admin"` |

Currently admin-gated endpoints:

| Endpoint | Requires |
|---|---|
| `POST /api/v1/wanted/:id/approve` | `role=admin` JWT |
| `POST /api/v1/wanted/:id/reject` | `role=admin` JWT |

---

## User Roles

| Role | Capabilities |
|---|---|
| `admin` | Full access — configure, download, approve/reject requests |
| `user` | Request media; view library, history, and own requests |

---

## Skipping Auth for Specific Endpoints

Set `SKIP_AUTH_ENDPOINTS` to a comma-separated list of path prefixes that bypass API key auth entirely (useful for health checks):

```env
SKIP_AUTH_ENDPOINTS=/api/health,/api/v1/health
```

Note: skipping API key auth does not bypass JWT role checks. A `RequireRole` guard still enforces the role requirement on its route even if the API key middleware was skipped.

---

## Environment Variable Reference

| Variable | Description | Default |
|---|---|---|
| `ENABLE_AUTH` | Enable API key authentication | `false` |
| `API_KEY` | Fixed API key; auto-generated if blank | — |
| `ALLOW_API_KEY_QUERY` | Allow `?api_key=` query parameter | `false` |
| `SKIP_AUTH_ENDPOINTS` | Comma-separated prefix list to bypass API key auth | — |
| `JWT_SECRET` | HMAC signing secret for session JWTs | random per-process |
| `ADMIN_PASSWORD` | Password for the auto-created admin account | random (logged once) |

---

## Frontend Integration

The SvelteKit frontend stores the JWT in memory (or `localStorage` for persistence) and includes it on every API request:

```typescript
// src/lib/api/index.ts
const token = localStorage.getItem('token');

const res = await fetch('/api/v1/wanted', {
  headers: {
    'Authorization': `Bearer ${token}`,
    'Content-Type': 'application/json',
  }
});
```

On login, store the returned `token` and `role`; on logout, clear them. The frontend reads `role` to show or hide admin-only controls (approve/reject buttons).

---

## Security Best Practices

1. **Set `JWT_SECRET`** to a long random string in production — without it, sessions are invalidated on every restart.
2. **Set `ADMIN_PASSWORD`** explicitly — do not rely on a logged random password.
3. **Use HTTPS** in production to prevent token interception.
4. **Rotate the API key** periodically via `POST /api/v1/auth/key`.
5. **Never expose API keys in client-side code** — use the JWT flow for browser clients.

---

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| 401 on every request | API key wrong, missing, or `ENABLE_AUTH=false` was toggled |
| 401 with a Bearer token | Token expired or signed with a different `JWT_SECRET` |
| 403 on approve/reject | JWT present but `role` is `"user"`, not `"admin"` |
| Sessions lost on restart | `JWT_SECRET` not set; server uses a random key each start |
| Cannot log in as admin | No admin user exists — check startup logs for the bootstrap password |
