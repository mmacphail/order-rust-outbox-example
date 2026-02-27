#!/usr/bin/env bash
# scripts/run_e2e_tests.sh
#
# Starts the required Docker Compose services, waits for them to be healthy,
# runs the end-to-end Rust tests, and then tears everything down.
#
# Usage:
#   ./scripts/run_e2e_tests.sh [--no-teardown]
#
# Options:
#   --no-teardown   Leave the Docker Compose stack running after the test.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

TEARDOWN=true
for arg in "$@"; do
  if [[ "$arg" == "--no-teardown" ]]; then
    TEARDOWN=false
  fi
done

cd "$REPO_ROOT"

teardown() {
  if [[ "$TEARDOWN" == "true" ]]; then
    echo "==> Stopping Docker Compose services..."
    docker compose down -v --remove-orphans
  else
    echo "==> --no-teardown set; leaving services running."
  fi
}
trap teardown EXIT

# ── 1. Start infrastructure services (not the order_service – we run it in the test process)
echo "==> Starting infrastructure services..."
docker compose up -d postgres kafka schema-registry debezium

# ── 2. Wait for Postgres
echo "==> Waiting for Postgres to be healthy..."
until docker compose exec -T postgres pg_isready -U order_user -d order_db > /dev/null 2>&1; do
  sleep 2
done
echo "    Postgres is ready."

# ── 3. Wait for Kafka
echo "==> Waiting for Kafka to be healthy..."
until docker compose exec -T kafka \
    /opt/kafka/bin/kafka-topics.sh --bootstrap-server localhost:9092 --list > /dev/null 2>&1; do
  sleep 3
done
echo "    Kafka is ready."

# ── 4. Wait for Schema Registry
echo "==> Waiting for Schema Registry..."
until curl -sf http://localhost:8081/subjects > /dev/null 2>&1; do
  sleep 3
done
echo "    Schema Registry is ready."

# ── 5. Wait for Debezium Connect REST API
echo "==> Waiting for Debezium Connect..."
until curl -sf http://localhost:8083/connectors > /dev/null 2>&1; do
  sleep 3
done
echo "    Debezium Connect is ready."

# ── 6. Run the end-to-end tests
echo "==> Running E2E tests..."
export DATABASE_URL="postgres://order_user:order_pass@localhost:5432/order_db"

cargo test --test e2e_test -- --include-ignored --nocapture

echo "==> E2E tests passed."
