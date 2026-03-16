#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  export output

  TEST_DIR="$BATS_TEST_TMPDIR/project"
  mkdir -p "$TEST_DIR"
}

function sigint_reaches_pty_child { # @test
  local marker="$BATS_TEST_TMPDIR/got-sigint"
  local ready="$BATS_TEST_TMPDIR/ready"

  cat > "$TEST_DIR/justfile" <<JUSTFILE
default:
  #!/usr/bin/env bash
  trap 'echo sigint > $marker; exit 0' INT
  touch $ready
  sleep 30
JUSTFILE

  just-me --justfile "$TEST_DIR/justfile" &
  local just_pid=$!

  local waited=0
  while [[ ! -f "$ready" ]] && (( waited < 50 )); do
    sleep 0.1
    (( waited++ )) || true
  done

  [[ -f "$ready" ]] || {
    echo "child never became ready" >&2
    kill "$just_pid" 2>/dev/null || true
    return 1
  }

  # Send SIGINT to just-me only
  kill -INT "$just_pid"

  # Wait for just-me to exit (non-zero is expected — it exits 130)
  wait "$just_pid" 2>/dev/null || true

  # Give a moment for filesystem sync
  sleep 0.2

  # The child should have received SIGINT and written the marker
  [[ -f "$marker" ]]
}
