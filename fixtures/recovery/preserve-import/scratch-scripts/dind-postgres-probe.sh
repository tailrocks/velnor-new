#!/bin/sh
# Probe Postgres readiness inside one private DinD. Do not raise the 60s budget.
set -u
DIND="${1:?dind container id}"
LOG="${2:?log path}"
BUDGET=60

note() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG"
}

note "dind=$DIND"
docker inspect "$DIND" --format 'mounts={{range .Mounts}}{{.Name}}->{{.Destination}};{{end}} image={{.Image}} platform={{.Platform}}' | tee -a "$LOG"
note "wait-api-start"
api=0
i=0
while [ "$i" -lt 90 ]; do
  if docker exec "$DIND" docker info --format '{{.ServerVersion}} {{.Driver}} {{.DockerRootDir}}' >"$LOG.api" 2>"$LOG.api.err"; then
    api=1
    break
  fi
  i=$((i + 1))
  sleep 1
done
note "api_ready=$api seconds=$i"
if [ "$api" -ne 1 ]; then
  note "FAIL api"
  cat "$LOG.api.err" >>"$LOG" || true
  exit 1
fi
cat "$LOG.api" | tee -a "$LOG"
docker exec "$DIND" sh -c 'df -P /var/lib/docker; echo ---; mount | grep " /var/lib/docker "' >>"$LOG" 2>&1 || true

note "pull-or-load postgres:18-alpine"
pull_start=$(date +%s)
if docker exec "$DIND" docker image inspect postgres:18-alpine >/dev/null 2>&1; then
  note "image-already-present"
else
  if docker exec "$DIND" docker pull postgres:18-alpine >>"$LOG" 2>&1; then
    note "pull-ok"
  else
    note "FAIL pull"
    exit 1
  fi
fi
pull_end=$(date +%s)
note "pull_seconds=$((pull_end - pull_start))"

one_start() {
  name="$1"
  note "create $name"
  t0=$(date +%s)
  if ! docker exec "$DIND" docker rm -f "$name" >>"$LOG" 2>&1; then
    note "no-prior $name"
  fi
  if ! docker exec "$DIND" docker run -d --name "$name" \
    -e POSTGRES_PASSWORD=postgres \
    -e POSTGRES_USER=postgres \
    -e POSTGRES_DB=postgres \
    postgres:18-alpine >>"$LOG" 2>&1; then
    note "FAIL create $name"
    return 1
  fi
  ready=0
  s=0
  while [ "$s" -lt "$BUDGET" ]; do
    if docker exec "$DIND" docker exec "$name" pg_isready -U postgres -d postgres >>"$LOG" 2>&1; then
      ready=1
      break
    fi
    s=$((s + 1))
    sleep 1
  done
  t1=$(date +%s)
  note "$name pg_isready=$ready elapsed=$((t1 - t0))s budget=${BUDGET}s"
  logs=$(docker exec "$DIND" docker logs "$name" 2>&1 || true)
  hits=$(printf '%s\n' "$logs" | grep -c 'database system is ready to accept connections' || true)
  note "$name ready_log_hits=$hits"
  if [ "$ready" -ne 1 ]; then
    note "FAIL ready $name"
    printf '%s\n' "$logs" | tail -n 40 >>"$LOG"
    return 1
  fi
  if docker exec "$DIND" docker exec "$name" psql -U postgres -d postgres -c 'select 1' >>"$LOG" 2>&1; then
    note "$name sql=ok"
  else
    note "FAIL sql $name"
    return 1
  fi
  docker exec "$DIND" docker rm -f "$name" >>"$LOG" 2>&1 || note "WARN rm $name"
  return 0
}

one_start pg1 || exit 1
one_start pg2 || exit 1
note "PASS two starts"
exit 0
