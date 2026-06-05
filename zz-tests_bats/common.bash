#! /bin/bash -e
#
# zz-tests_bats/common.bash — load from every .bats file's setup():
#
#   setup() {
#     load "$(dirname "$BATS_TEST_FILE")/common.bash"
#     setup_test_home
#   }

if [[ -z $BATS_TEST_TMPDIR ]]; then
  echo 'common.bash loaded before $BATS_TEST_TMPDIR set. aborting.' >&2
  exit 1
fi

pushd "$BATS_TEST_TMPDIR" >/dev/null || exit 1

bats_load_library bats-support
bats_load_library bats-assert
bats_load_library bats-emo
bats_load_library bats-island

require_bin JUST_BIN just

# Wrap the just CLI invocation so every test gets the same
# per-invocation timeout and bats-assert's `run` semantics.
#
# The 5s here is per-invocation: just is killed if a single call hangs
# that long. Independent of BATS_TEST_TIMEOUT (set in bats.nix
# extraEnv) which caps the whole @test block's wall-clock.
run_just() {
  local bin="${JUST_BIN:-just}"
  run timeout --preserve-status 5s "$bin" "$@"
}
