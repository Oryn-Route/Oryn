#!/bin/bash
# scripts/wait-for-dbs.sh
# Polls PostgreSQL and Redis containers until they are healthy/ready.

set -e

TIMEOUT=30
INTERVAL=1
ELAPSED=0

echo "⏳ Waiting for database services to be ready..."

# 1. Wait for Postgres
until docker exec orynroute-postgres pg_isready -U orynroute >/dev/null 2>&1; do
    if [ $ELAPSED -ge $TIMEOUT ]; then
        echo "❌ Error: Postgres (orynroute-postgres) failed to become ready within $TIMEOUT seconds."
        echo "   Please check container logs: docker logs orynroute-postgres"
        exit 1
    fi
    sleep $INTERVAL
    ELAPSED=$((ELAPSED + INTERVAL))
done
echo "✅ Postgres is ready!"

# Reset elapsed time for Redis
ELAPSED=0

# 2. Wait for Redis
until docker exec orynroute-redis redis-cli ping 2>/dev/null | grep -q "PONG"; do
    if [ $ELAPSED -ge $TIMEOUT ]; then
        echo "❌ Error: Redis (orynroute-redis) failed to become ready within $TIMEOUT seconds."
        echo "   Please check container logs: docker logs orynroute-redis"
        exit 1
    fi
    sleep $INTERVAL
    ELAPSED=$((ELAPSED + INTERVAL))
done
echo "✅ Redis is ready!"

echo "🎉 All database services are healthy and ready!"
