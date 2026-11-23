set quiet

# This repository has two halves and no root Cargo workspace:
#   cli/       the Rust crate (the bus)
#   extension/ the pi extension (TypeScript, tested with node --test)
# Every cargo recipe therefore runs inside cli/, and every find() is scoped to cli/src.

crate := "cli"
ext   := "extension"

# Content-addressed lcov path, relative to the crate: changes whenever the crate's sources or
# manifests change. Kept under cli/target because that is where cargo runs.
export LCOV_FILE := `\
    _tree=$(git ls-tree -r --full-tree HEAD -- cli/src cli/tests cli/Cargo.toml cli/Cargo.lock 2>/dev/null | sha256sum | cut -c1-8); \
    _dirty=$(git diff --no-ext-diff -- cli/src cli/tests cli/Cargo.toml cli/Cargo.lock 2>/dev/null | sha256sum | cut -c1-8); \
    printf "target/lcov-%s.info" "$(printf '%s:%s' "$_tree" "$_dirty" | sha256sum | cut -c1-8)"`

# The shared engine's tests need a PostgreSQL with pgmq. The image is built locally from a pinned
# upstream version, so CI needs podman but neither a third-party image publisher nor network access
# once the image is cached.
PG_IMAGE := "localhost/pi-msgq-pgmq:18-v1.13.1"
PG_NAME  := "pi-msgq-pg"
PG_PORT  := "55433"

# CRAP score above which a function counts as crappy
CRAP_THRESHOLD := "30"

# Soft target for a source file, in lines. Files listed as known debt are reported but not failed,
# so the rule stops new offenders without pretending the existing ones are fine.
FILE_TARGET := "550"

# Build the test database image. pgmq's SQL is fetched at build time from the pinned tag.
pg-image:
    podman build -t {{PG_IMAGE}} ci/pgmq

# Print a connection string for a PostgreSQL with pgmq, starting a throwaway one when needed.
# A caller-supplied MSGQ_TEST_DB_URI wins, so a real server can be used instead.
pg-url:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "${MSGQ_TEST_DB_URI:-}" ]; then
        printf '%s\n' "$MSGQ_TEST_DB_URI"
        exit 0
    fi
    if ! command -v podman >/dev/null; then
        echo "podman is needed to run the shared engine's tests" >&2
        exit 1
    fi
    if ! podman container exists {{PG_NAME}} >/dev/null 2>&1; then
        podman build -t {{PG_IMAGE}} ci/pgmq >/dev/null
        podman run -d --name {{PG_NAME}} \
            -e POSTGRES_USER=msgq -e POSTGRES_DB=msgq \
            -e POSTGRES_HOST_AUTH_METHOD=trust \
            -p 127.0.0.1:{{PG_PORT}}:5432 {{PG_IMAGE}} >/dev/null
    fi
    podman start {{PG_NAME}} >/dev/null 2>&1 || true
    for _ in $(seq 1 60); do
        if podman exec {{PG_NAME}} pg_isready -U msgq -d msgq >/dev/null 2>&1; then
            printf 'postgres://msgq@127.0.0.1:%s/msgq\n' {{PG_PORT}}
            exit 0
        fi
        sleep 1
    done
    echo "the test database did not become ready" >&2
    exit 1

# Install the bus's own schema on the test database.
#
# The image ships pgmq only; msgq's tables and functions are the client's to install, and the Rust
# suites get them from connecting. A JS test has no such hook, so the CLI is asked to connect once
# — the same BOOTSTRAP and authorizing SQL any deployment would run.
pg-schema:
    #!/usr/bin/env bash
    set -euo pipefail
    dsn=$(just pg-url)
    (cd {{crate}} && cargo run --quiet -- --db "$dsn" sessions >/dev/null)

# Stop the test database without losing it, so the next run starts quickly.
pg-down:
    podman stop {{PG_NAME}}

# Install the zsh completion where `fpath` already looks. `ZSH_COMPLETION_DIR` names the directory;
# without it the script goes to ~/.zfunc, which is what the README tells people to add to fpath.
completions dir="":
    #!/usr/bin/env bash
    set -euo pipefail
    target="${ZSH_COMPLETION_DIR:-{{dir}}}"
    if [ -z "$target" ]; then target="$HOME/.zfunc"; fi
    mkdir -p "$target"
    cargo run --quiet --manifest-path {{crate}}/Cargo.toml -- completions zsh > "$target/_msgq"
    printf 'wrote %s\n' "$target/_msgq"

# The same behavioural fixtures against the shared engine. Two engines, one product: a change that
# only one of them honours is exactly what this catches.
behaviour-shared:
    #!/usr/bin/env bash
    export MSGQ_TEST_DB_URI="$(just pg-url)"
    export MSGQ_BEHAVIOUR_ENGINE=shared
    if output=$(cd {{crate}} && cargo test --test behaviour 2>&1); then
        echo "✓ behavioural fixtures passed on both engines"
    else
        printf '%s\n' "$output"
        exit 1
    fi

# Run all checks (mirrors what CI will run)
ci: veriplan check fmt lint lint-suppressions check-file-sizes check-test-names machete rust-test-then-crap behaviour-shared extension-test
# make explicit that we run tests then crap (crap reuses test's lcov output)
rust-test-then-crap: test crap

build: cargo-build

alias lint-spec := veriplan

# Fast compile check — all targets of the crate
check:
    #!/usr/bin/env bash
    if output=$(cd {{crate}} && cargo check --all-targets 2>&1); then
        echo "✓ check passed"
    else
        printf '%s\n' "$output"
        exit 1
    fi

# Build (dev profile)
cargo-build:
    #!/usr/bin/env bash
    if output=$(cd {{crate}} && cargo build 2>&1); then
        echo "✓ build passed"
    else
        printf '%s\n' "$output"
        exit 1
    fi

# Run tests under coverage (needs cargo-nextest + cargo-llvm-cov).
# The lcov output feeds `just crap`.
test:
    #!/usr/bin/env bash
    if ! command -v cargo-nextest &>/dev/null; then
        echo "cargo-nextest not found. Install with:"
        echo "  cargo install cargo-nextest --locked"
        exit 1
    fi
    if ! command -v cargo-llvm-cov &>/dev/null; then
        echo "cargo-llvm-cov not found. Install with:"
        echo "  cargo install cargo-llvm-cov --locked"
        exit 1
    fi
    rm -f {{crate}}/target/lcov-*.info
    mkdir -p {{crate}}/target
    # The shared engine's tests need a database with pgmq. Provide one so that a run without any
    # setup still exercises both engines, instead of scoring the shared engine as untested.
    export MSGQ_TEST_DB_URI="$(just pg-url)"
    output=$(cd {{crate}} && cargo llvm-cov nextest --lib --bins --tests \
        --lcov --output-path "$LCOV_FILE" 2>&1)
    code=$?
    if [ $code -eq 0 ]; then
        echo "✓ tests passed"
    else
        printf '%s\n' "$output"
        exit $code
    fi

# Clippy over every target; warnings are errors.
lint:
    #!/usr/bin/env bash
    if output=$(cd {{crate}} && cargo clippy --all-targets -- -Dwarnings 2>&1); then
        echo "✓ lint passed"
    else
        printf '%s\n' "$output"
        exit 1
    fi

# Advisory: flag clippy suppressions for too_many_arguments / too_many_lines; resolve by
# restructuring. Warns only — never fails the gate.
lint-suppressions:
    #!/usr/bin/env bash
    hits=$(find {{crate}}/src -name '*.rs' -exec grep -nE '#!?\[(allow|expect)\([^]]*clippy::(too_many_arguments|too_many_lines)' {} + || true)
    if [ -n "$hits" ]; then
        printf '⚠ clippy suppression(s) for too_many_arguments/too_many_lines found; prefer restructuring:\n%s\n' "$hits"
    else
        echo "✓ lint-suppressions passed"
    fi

# see https://github.com/autonomous-toaster/veriplan
[group('optional')]
veriplan change="":
    #!/usr/bin/env bash
    if ! command -v veriplan >/dev/null 2>&1; then
        echo "⚠ veriplan skipped (veriplan not installed)"
        exit 0
    fi
    if [ -n "{{change}}" ]; then
        plans="{{change}}"
    else
        # The plan in flight: every change that is not complete.
        plans=$(openspec list --json 2>/dev/null \
            | jq -r '.changes[] | select(.status != "complete") | .name')
    fi
    if [ -z "$plans" ]; then
        echo "⚠ veriplan skipped (no plan in flight)"
        exit 0
    fi
    fail=0
    while IFS= read -r plan; do
        [ -n "$plan" ] || continue
        if output=$(veriplan check "$plan" 2>&1); then
            echo "✓ veriplan passed: $plan"
        else
            printf '%s\n' "$output"
            fail=1
        fi
    done <<< "$plans"
    exit $fail

# Check format without modifying files
fmt:
    #!/usr/bin/env bash
    if output=$(cd {{crate}} && cargo fmt --check 2>&1); then
        echo "✓ fmt passed"
    else
        printf '%s\n' "$output"
        echo "→ fix with: cargo fmt"
        exit 1
    fi

# Unused dependency check
machete:
    #!/usr/bin/env bash
    if output=$(cd {{crate}} && cargo machete 2>&1); then
        echo "✓ machete passed"
    else
        printf '%s\n' "$output"
        exit 1
    fi

# CRAP complexity — generates coverage then scores; fails if any function exceeds CRAP_THRESHOLD.
crap:
    #!/usr/bin/env bash
    if [ ! -f "{{crate}}/$LCOV_FILE" ]; then
        just test || exit $?
    fi
    if output=$(cd {{crate}} && cargo crap \
        --lcov "$LCOV_FILE" \
        --threshold {{CRAP_THRESHOLD}} \
        --exclude '**/tests/**' --exclude 'src/main.rs' \
        --missing skip \
        --fail-above --format json 2>/dev/null); then
        echo "✓ crap passed"
    else
        printf '%s\n' "$output" | jq -r '
            . as $root
            | [ .entries[] | select(.crap > {{CRAP_THRESHOLD}}) ] as $bad
            | "\($bad | length) of \($root.entries | length) functions exceed the CRAP threshold of {{CRAP_THRESHOLD}} (worst first)"
          '
        printf '%s\n' "$output" | jq -r '
            [ .entries[] | select(.crap > {{CRAP_THRESHOLD}}) ]
            | sort_by(.crap) | reverse
            | [ "CRAP\tCC\tCov%\tFunction\tLocation" ]
              + map([ "\((.crap * 10 | round) / 10 | floor).\((.crap * 10 | round) % 10)",
                      "\(.cyclomatic | floor)",
                      "\((.coverage * 10 | round) / 10 | floor).\((.coverage * 10 | round) % 10)%",
                      .function,
                      "\(.file):\(.line)" ] | @tsv)
            | .[]
          ' | column -t -s $'\t'
        exit 1
    fi

# Check that no source file exceeds the line target. Files in KNOWN_DEBT are reported but do not
# fail, so the rule prevents new offenders while the existing ones stay visible.
check-file-sizes max=FILE_TARGET:
    #!/usr/bin/env bash
    TARGET={{max}}
    # Known debt: over target today, kept out of the gate so it can shrink, never grow silently.
    KNOWN_DEBT=" {{crate}}/src/commands.rs {{crate}}/src/store.rs "
    fail=0
    debt=0
    while IFS= read -r f; do
        lines=$(wc -l < "$f")
        if [ "$lines" -gt "$TARGET" ]; then
            if printf '%s' "$KNOWN_DEBT" | grep -q " $f "; then
                debt=$((debt + 1))
                echo "⚠ $f has $lines lines (target $TARGET) — known debt"
            else
                echo "FAIL: $f has $lines lines (target $TARGET)"
                fail=1
            fi
        fi
    done < <(find {{crate}}/src -name '*.rs' | sort)
    [ $fail -eq 0 ] && echo "✓ no new files over $TARGET lines ($debt known)"
    exit $fail

# Reject sequenced test-file names: a stem ending in `test`/`tests` + optional `_` + digits.
check-test-names:
    #!/usr/bin/env bash
    fail=0
    while IFS= read -r f; do
        stem=$(basename "$f" .rs)
        if printf '%s' "$stem" | grep -Eq 'tests?_?[0-9]+$'; then
            echo "FAIL: $f has a numeric suffix; name it for its topic (tests_<topic>.rs)"
            fail=1
        fi
    done < <(find {{crate}}/src -name '*.rs')
    [ $fail -eq 0 ] && echo "✓ no numerically-suffixed test files"
    exit $fail

# The extension's unit and conformance suites.
extension-test:
    #!/usr/bin/env bash
    # `pg` is an optional dependency: a session on a local file never loads it, but the shared
    # engine's tests do, so install when it is missing.
    if [ ! -d {{ext}}/node_modules ]; then
        (cd {{ext}} && npm install --no-audit --no-fund >/dev/null) || {
            echo "the extension's optional dependencies could not be installed" >&2
            exit 1
        }
    fi
    export MSGQ_TEST_DB_URI="$(just pg-url)"
    just pg-schema >/dev/null
    if output=$(cd {{ext}} && node --test test/ 2>&1); then
        echo "✓ extension tests passed"
    else
        printf '%s\n' "$output"
        exit 1
    fi
