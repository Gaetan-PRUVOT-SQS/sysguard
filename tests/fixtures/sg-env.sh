#!/bin/sh
set -eu
printf 'home=%s rule=%s\n' "${HOME+plein}" "${SG_RULE_NAME}" >tests/fixtures/sortie-env.txt
