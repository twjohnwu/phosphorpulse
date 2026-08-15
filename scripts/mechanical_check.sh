#!/usr/bin/env bash

# Run from the repository root.  The optional first argument is the expected
# aggregate number of passed tests; without it, only failing tests fail check 2.

set -u

ledger=scripts/test-fingerprints.txt
allowlist=scripts/allowed-modified.txt
spec_file=STDD/tui/spec.md
design_file=STDD/tui/design-ux.md
log_file=${MECH_LOG:-${TMPDIR:-/tmp}/phosphorpulse-mechanical.log}
expected_passed=${1-}
overall_status=0

append_log() {
    printf '\n[%s]\n%s\n' "$1" "$2" >> "$log_file"
}

mark_failure() {
    overall_status=1
}

# 1. Locked test-file fingerprints.
fingerprint_output=$(shasum -a 256 -c "$ledger" 2>&1)
if [ $? -eq 0 ]; then
    printf 'MECH fingerprints: PASS\n'
else
    printf 'MECH fingerprints: FAIL — checksum mismatch\n'
    append_log fingerprints "$fingerprint_output"
    mark_failure
fi

# 2. Cargo test aggregate.
cargo_cmd=${CARGO:-"$HOME/.cargo/bin/cargo"}
cargo_output=$("$cargo_cmd" test 2>&1)
test_summary=$(printf '%s\n' "$cargo_output" | awk '
    /^test result:/ {
        result_lines++
        for (i = 1; i <= NF; i++) {
            if ($i == "passed;") passed += $(i - 1)
            if ($i == "failed;") failed += $(i - 1)
        }
    }
    END { print passed + 0, failed + 0, result_lines + 0 }
')
set -- $test_summary
passed_count=$1
failed_count=$2
result_lines=$3

if [ "$result_lines" -eq 0 ]; then
    printf 'MECH tests: FAIL — no test result lines\n'
    append_log tests "$cargo_output"
    mark_failure
elif [ "$failed_count" -gt 0 ]; then
    printf 'MECH tests: FAIL — %s failed\n' "$failed_count"
    append_log tests "$cargo_output"
    mark_failure
elif [ -n "$expected_passed" ] && [ "$passed_count" != "$expected_passed" ]; then
    printf 'MECH tests: FAIL — expected %s passed, got %s\n' "$expected_passed" "$passed_count"
    append_log tests "$cargo_output"
    mark_failure
else
    printf 'MECH tests: PASS\n'
fi

# 3. Tracked working-tree modifications must be allowlisted when a list exists.
if [ -f "$allowlist" ]; then
    status_output=$(git status --porcelain 2>&1)
    if [ $? -ne 0 ]; then
        printf 'MECH scope: FAIL — git status failed\n'
        append_log scope "$status_output"
        mark_failure
    else
        scope_extra=$(printf '%s\n' "$status_output" | awk '
            NR == FNR { allowed[$0] = 1; next }
            substr($0, 1, 3) == " M " {
                path = substr($0, 4)
                if (!(path in allowed)) print path
            }
        ' "$allowlist" -)
        if [ -n "$scope_extra" ]; then
            printf 'MECH scope: FAIL — unallowlisted tracked modifications\n'
            append_log scope "$scope_extra"
            mark_failure
        else
            printf 'MECH scope: PASS\n'
        fi
    fi
else
    printf 'MECH scope: PASS\n'
fi

# 4. Approved fingerprints cover document bodies after the second frontmatter fence.
body_hash() {
    awk 'found { print } $0 == "---" { if (++fences == 2) found = 1 }' "$1" |
        shasum -a 256 |
        awk '{ print $1 }'
}

frontmatter_value() {
    awk -F'"' -v key="$2" '$0 ~ ("^" key ":") { print $2; exit }' "$1"
}

actual_spec=$(body_hash "$spec_file")
actual_design=$(body_hash "$design_file")
approved_spec=$(frontmatter_value "$spec_file" approved_fingerprint)
approved_design=$(frontmatter_value "$spec_file" design_ux_fingerprint)

if [ -z "$approved_spec" ] || [ -z "$approved_design" ]; then
    printf 'MECH spec: FAIL — approved fingerprint missing\n'
    append_log spec "approved_fingerprint=$approved_spec\ndesign_ux_fingerprint=$approved_design"
    mark_failure
elif [ "$actual_spec" != "$approved_spec" ] || [ "$actual_design" != "$approved_design" ]; then
    printf 'MECH spec: FAIL — body fingerprint mismatch\n'
    append_log spec "spec.md expected=$approved_spec actual=$actual_spec\ndesign-ux.md expected=$approved_design actual=$actual_design"
    mark_failure
else
    printf 'MECH spec: PASS\n'
fi

if [ "$overall_status" -ne 0 ]; then
    printf 'MECH log: %s\n' "$log_file"
fi

exit "$overall_status"
