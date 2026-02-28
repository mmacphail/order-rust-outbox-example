# ADR 001 – CI Caching Strategy

## Status

Accepted

## Date

2026-02-28

## Context

The CI pipeline runs two jobs on every pull request targeting `main`:

1. **Build, Lint & Unit Tests** – compiles the Rust codebase, runs `clippy`, and executes unit tests.
2. **End-to-End Tests** – spins up the full Docker Compose stack (PostgreSQL, Kafka, Schema Registry, Debezium) and exercises the HTTP API + Kafka consumer.

Without caching, both jobs start cold on every run:

| Job | Cold duration |
|-----|--------------|
| Build, Lint & Unit Tests | ~162s |
| End-to-End Tests | ~200–300s |

Two bottlenecks were identified:

- **Rust compilation**: `cargo build` recompiles all dependencies from scratch on every run, even when `Cargo.lock` has not changed.
- **Docker image pulls**: the four images required by the stack (`debezium/postgres:16`, `apache/kafka:4.0.0`, `confluentinc/cp-schema-registry:7.6.0`, and a locally built Debezium image) total over 2 GB compressed and must be pulled from the internet on each run.

An additional structural problem was identified: the workflow only triggered on `pull_request` events, so caches written by one PR branch were deleted when that branch was deleted after merging. Every new PR therefore started cold even when dependencies had not changed.

## Decision

### 1. Cache Rust compilation artifacts

Use `actions/cache@v4` in both jobs, keyed on the hash of `Cargo.lock`:

```
key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
restore-keys: ${{ runner.os }}-cargo-
```

The cached paths are `~/.cargo/registry/`, `~/.cargo/git/db/`, and `target/`. The restore key prefix ensures a partial hit is used when `Cargo.lock` changes, avoiding a completely cold build.

### 2. Cache Docker images as a tar archive

Use `actions/cache@v4` in the E2E job, keyed on the hash of `docker-compose.yml` and `debezium/Dockerfile`:

```
key: docker-${{ runner.os }}-${{ hashFiles('docker-compose.yml', 'debezium/Dockerfile') }}
```

On a cache miss the images are pulled/built and saved via `docker save`. On a cache hit they are restored via `docker load`, skipping all network traffic.

### 3. Populate the cache from `main` after every merge

The workflow trigger was extended to also fire on `push` to `main`:

```yaml
on:
  push:
    branches:
      - main
  pull_request:
    branches:
      - main
```

This ensures a warm cache always exists on the base branch. Every new PR can immediately restore from it, eliminating cold starts on the first push of a branch.

### 4. Run e2e tests on every PR

The E2E job runs unconditionally on every pull request. The core value proposition of this service is the transactional outbox pipeline (order write → outbox insert → Debezium CDC → Kafka). That invariant cannot be verified by unit tests alone; it requires the full stack. The 2m20s warm-cache cost is low enough that skipping e2e to save time is not worth the risk of merging a broken pipeline.

## Alternatives Considered

### Replace Confluent Schema Registry with Apicurio Registry

`confluentinc/cp-schema-registry` is ~1.4 GB compressed, roughly half the total Docker cache. `apicurio/apicurio-registry` is ~414 MB — a 3.5× reduction.

This was rejected because:
- The Debezium Confluent Avro Converter is tightly coupled to the Confluent Schema Registry API and wire format.
- Apicurio's Confluent-compat endpoint (`/apis/ccompat/v6`) has subtle behavioural differences that could produce false test failures.
- Production uses Confluent Schema Registry; diverging in CI reduces test fidelity.

### Run e2e tests only on path-filtered changes

Skipping e2e for PRs that only touch documentation or CI configuration was considered. This was deferred because the current cost is acceptable and path filtering adds maintenance overhead.

## Consequences

- **Warm-cache Build job**: ~32s (down from ~162s, 5× faster).
- **Warm-cache E2E job**: ~2m20s (down from ~3m20s+).
- The `main` branch always has a populated cache; new PRs never pay the cold-start penalty unless `Cargo.lock` or Docker configuration changes.
- The Docker cache key must be invalidated manually (by touching `docker-compose.yml` or `debezium/Dockerfile`) if an image tag is changed outside those files.
