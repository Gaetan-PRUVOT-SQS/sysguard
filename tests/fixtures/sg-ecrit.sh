#!/bin/sh
set -eu
printf '%s %s %s\n' "${SG_RULE_NAME}" "${SG_SEVERITY}" "${SG_VALUE}" >tests/fixtures/sortie-exec.txt
