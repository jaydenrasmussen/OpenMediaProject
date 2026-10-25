# Browser tests

A smoke test that drives the real UI with Playwright against a running server.

```
cd ui && npm ci && npm run build && cd ..
cargo run --release -p omp-server &       # OMP_ADDR defaults to 0.0.0.0:8080
cd e2e && npm install && npx playwright install chromium
OMP_URL=http://localhost:8080 npx playwright test
```

It creates the first account if there is none (`OMP_E2E_USER` / `OMP_E2E_PASSWORD`, default `e2e` / `e2e-password-1`), then visits each page and checks that it renders without errors and that the settings tabs open.
