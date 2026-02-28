# FIXME – Production Readiness Gaps

Tracked items to close the gap between demo and production-grade quality.

---

## Security

- [ ] Add `cargo audit` step to CI pipeline (supply chain vulnerabilities)
- [ ] Add `SECURITY.md` with vulnerability disclosure policy
- [ ] Add API authentication (JWT, API keys, or OAuth2)
- [ ] Enable TLS for all service traffic (API, Postgres, Kafka)
- [ ] Add rate limiting middleware (e.g. `actix-governor`)
- [ ] Add stricter input validation on request payloads
- [ ] Add Dependabot or Renovate for automated dependency updates
- [ ] Add secrets scanning (git-secrets, GitHub secret scanning)
- [ ] Generate SBOM (CycloneDX or SPDX)
- [ ] Add SAST scanning (semgrep, CodeQL, or `cargo-deny`)
- [ ] Add security headers middleware (CORS, CSP, X-Frame-Options)

---

## Observability

- [ ] Add `/health` and `/readiness` HTTP endpoints (required for Kubernetes probes)
- [ ] Add `/metrics` endpoint with Prometheus exposition format
- [ ] Switch to structured JSON logging (replace `env_logger` with `tracing` + `tracing-subscriber`)
- [ ] Add distributed tracing (OpenTelemetry + Jaeger or Zipkin)
- [ ] Add request correlation IDs propagated through logs and error responses
- [ ] Add Grafana dashboard definitions
- [ ] Add Prometheus alerting rules (error rate, latency, Kafka lag)
- [ ] Document log retention and rotation policy

---

## Deployment

- [ ] Add Kubernetes manifests (Deployment, Service, Ingress, ConfigMap, Secret)
- [ ] Add Helm chart
- [ ] Add container image publishing to a registry (GHCR, ECR, or Docker Hub) in CI
- [ ] Add environment-specific configuration (dev / staging / prod separation)
- [ ] Add IaC for cloud infrastructure (Terraform, Pulumi, or CDK)
- [ ] Pin Docker base image digests (not just tags)
- [ ] Add deployment runbook (scaling, failover, rollback)
- [ ] Document blue-green or canary rollout strategy

---

## Release & Governance

- [ ] Add `LICENSE` file (MIT or Apache-2.0)
- [ ] Add `CHANGELOG.md` and create initial `v0.1.0` git tag
- [ ] Add `CONTRIBUTING.md` with branching strategy and PR process
- [ ] Add `CODE_OF_CONDUCT.md` (Contributor Covenant)
- [ ] Add `CODEOWNERS` file
- [ ] Add `.github/pull_request_template.md`
- [ ] Add `.github/ISSUE_TEMPLATE/` (bug report + feature request)
- [ ] Add release automation (semantic-release or release-please)
- [ ] Move commit conventions doc to repo root (not only in `.claude/`)

---

## Testing

- [ ] Add code coverage reporting (`cargo-tarpaulin` or `cargo-llvm-cov`) with CI gate
- [ ] Add performance benchmarks (`criterion`)
- [ ] Add property-based tests (`proptest` or `quickcheck`)
- [ ] Add migration rollback tests (run `down.sql` and verify)
- [ ] Add load/stress testing config (k6 or wrk)

---

## Database

- [ ] Define and document a database backup strategy
- [ ] Add explicit index definitions in migrations
- [ ] Document connection pool sizing and tuning guidelines
- [ ] Add a distributed migration lock for multi-instance safety
- [ ] Document PostgreSQL HA strategy (standby, WAL archiving)

---

## CI/CD

- [ ] Add `cargo audit` job
- [ ] Add code coverage upload (Codecov or Coveralls)
- [ ] Add Docker image build + push to registry on merge to `main`
- [ ] Add branch protection rules (require PR, passing checks, review)
- [ ] Add scheduled nightly security scan

---

## Quick Wins (low effort, high value)

These can each be done in under an hour:

- [ ] `LICENSE` file
- [ ] `CHANGELOG.md` + `git tag v0.1.0`
- [ ] `CONTRIBUTING.md`
- [ ] `CODE_OF_CONDUCT.md`
- [ ] `SECURITY.md`
- [ ] `CODEOWNERS`
- [ ] `.github/pull_request_template.md`
- [ ] `.github/ISSUE_TEMPLATE/` (bug + feature)
- [ ] `cargo audit` in CI
- [ ] `/health` endpoint
